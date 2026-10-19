use crate::xr_input::{Packet, SeatEvent};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Instant,
};
#[derive(Clone)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
    pub stamp: Instant,
}
pub const TOP_BAR: u64 = 0;
pub const NOTIFICATIONS: u64 = u64::MAX - 3;
pub const TASK_BAR: u64 = u64::MAX - 2;
pub fn is_shell(id: u64) -> bool {
    id == TOP_BAR || id == TASK_BAR || id == NOTIFICATIONS
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ShellLayout {
    pub width: u32,
    pub height: u32,
    pub top: u32,
    pub bottom: u32,
    #[serde(default)]
    pub notifications: Option<[u32; 4]>,
}
pub struct Bridge {
    pub shell: ShellLayout,
    pub requested: HashMap<u64, Instant>,
    pub frames: HashMap<u64, Surface>,
    pub packets: VecDeque<Packet>,
    pub last_input: Option<Instant>,
    pub session: Session,
    pub last_session: Option<Instant>,
    pub exit_requested: bool,
    pub xr_socket: String,
    pub cursor: Option<Cursor>,
    pub mouse_angles: Option<[f32; 2]>,
    pub last_mouse: Option<Instant>,
    pub mouse_buttons: std::collections::HashSet<u32>,
    pub hand_resume_ms: u64,
    pub input_realm: crate::Realm,
}
impl Default for Bridge {
    fn default() -> Self {
        Self {
            shell: ShellLayout::default(),
            requested: HashMap::new(),
            frames: HashMap::new(),
            packets: VecDeque::new(),
            last_input: None,
            session: Session::default(),
            last_session: None,
            exit_requested: false,
            xr_socket: String::new(),
            cursor: None,
            mouse_angles: None,
            last_mouse: None,
            mouse_buttons: Default::default(),
            hand_resume_ms: 800,
            input_realm: crate::Realm::Desktop,
        }
    }
}
pub type Shared = Arc<Mutex<Bridge>>;
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Session {
    pub id: u64,
    pub visible: bool,
    pub focused: bool,
    pub head: crate::Pose,
    pub horizontal_fov_deg: f32,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub id: u64,
    pub uv: [f32; 2],
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Control {
    pub input_realm: crate::Realm,
    pub visible: bool,
    pub mouse: bool,
    pub cursor: Option<Cursor>,
    #[serde(default)]
    pub mouse_angles: Option<[f32; 2]>,
    pub exit_requested: bool,
}
impl Bridge {
    pub fn visible(&self) -> bool {
        self.session.visible
            && self
                .last_session
                .is_some_and(|t| t.elapsed().as_secs_f32() < 2.)
    }
    pub fn mouse_owned(&self) -> bool {
        !self.mouse_buttons.is_empty()
            || self
                .last_mouse
                .is_some_and(|t| t.elapsed().as_millis() < self.hand_resume_ms as u128)
    }
    pub fn control(&self) -> Control {
        Control {
            input_realm: self.input_realm,
            visible: self.visible(),
            mouse: self.mouse_owned(),
            cursor: self.cursor.clone(),
            mouse_angles: self.mouse_angles,
            exit_requested: self.exit_requested,
        }
    }
    pub fn heartbeat(&mut self, session: Session) -> Result<(), String> {
        session.head.validate()?;
        if !session.horizontal_fov_deg.is_finite()
            || !(0. ..=170.).contains(&session.horizontal_fov_deg)
        {
            return Err("Invalid headset FOV".into());
        }
        // A fresh running session acknowledges a previous stop request.
        if session.id != self.session.id || self.last_session.is_none() {
            self.exit_requested = false;
        }
        self.session = session;
        self.last_session = Some(Instant::now());
        Ok(())
    }
    pub fn push(&mut self, p: Packet) -> Result<(), String> {
        if self.mouse_owned() {
            // Renderer packets cannot interfere with an active physical mouse drag.
            return Ok(());
        }
        if p.events.len() > 32 || p.actions.len() > 16 {
            return Err("XR packet too large".into());
        }
        if self.packets.len() >= 8 {
            self.packets.clear();
            self.packets.push_back(Packet {
                events: vec![SeatEvent::Reset],
                actions: vec![],
            });
            return Err("XR input queue overflow; input reset".into());
        }
        self.last_input = Some(Instant::now());
        self.packets.push_back(p);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn stop_request_belongs_to_its_renderer_generation() {
        let mut b = Bridge::default();
        let session = Session {
            id: 1,
            visible: true,
            ..Default::default()
        };
        b.heartbeat(session.clone()).unwrap();
        b.exit_requested = true;
        b.last_session = Some(Instant::now() - Duration::from_secs(3));
        b.heartbeat(session).unwrap();
        assert!(b.control().exit_requested);
        b.heartbeat(Session {
            id: 2,
            ..Default::default()
        })
        .unwrap();
        assert!(!b.control().exit_requested);
    }
    #[test]
    fn renderer_lease_and_mouse_idle_are_independent() {
        let mut b = Bridge::default();
        b.heartbeat(Session {
            visible: true,
            ..Session::default()
        })
        .unwrap();
        assert!(b.visible());
        b.last_mouse = Some(Instant::now() - Duration::from_secs(1));
        assert!(!b.mouse_owned());
        b.mouse_buttons.insert(0x110);
        assert!(b.mouse_owned());
        b.push(Packet {
            events: vec![SeatEvent::Reset],
            actions: vec![],
        })
        .unwrap();
        assert!(b.packets.is_empty());
        b.mouse_buttons.clear();
        b.last_mouse = Some(Instant::now());
        assert!(b.mouse_owned());
        b.last_session = Some(Instant::now() - Duration::from_secs(3));
        assert!(!b.visible());
        b.heartbeat(Session {
            visible: true,
            ..Session::default()
        })
        .unwrap();
        assert!(b.visible());
    }
}
