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
    pub finder_started: Option<Instant>,
    pub accent: [u8; 3],
    pub mouse_angles: Option<[f32; 2]>,
    pub last_mouse: Option<Instant>,
    pub mouse_buttons: std::collections::HashSet<u32>,
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
            finder_started: None,
            accent: crate::visual_feedback::ACCENT,
            mouse_angles: None,
            last_mouse: None,
            mouse_buttons: Default::default(),
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
    /// Shared eye viewport angles in radians: left, right, down, up.
    pub mouse_viewport: Option<[f32; 4]>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub id: u64,
    pub uv: [f32; 2],
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Control {
    #[serde(default)]
    pub finder_seconds: Option<f32>,
    #[serde(default = "default_accent")]
    pub accent: [u8; 3],
    pub input_realm: crate::Realm,
    pub visible: bool,
    pub mouse: bool,
    pub cursor: Option<Cursor>,
    #[serde(default)]
    pub mouse_angles: Option<[f32; 2]>,
    pub exit_requested: bool,
}
fn default_accent() -> [u8; 3] { crate::visual_feedback::ACCENT }
impl Bridge {
    pub fn visible(&self) -> bool {
        self.session.visible
            && self
                .last_session
                .is_some_and(|t| t.elapsed().as_secs_f32() < 2.)
    }
    pub fn mouse_owned(&self) -> bool {
        // Mouse ownership lasts for this input realm, including idle periods.
        self.last_mouse.is_some() || !self.mouse_buttons.is_empty()
    }
    pub fn control(&self) -> Control {
        Control {
            finder_seconds: self.finder_started.map(|t| t.elapsed().as_secs_f32())
                .filter(|t| self.input_realm == crate::Realm::Xr && *t < crate::visual_feedback::FINDER_DURATION),
            accent: self.accent,
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
        if let Some([left, right, down, up]) = session.mouse_viewport {
            if ![left, right, down, up].iter().all(|a| a.is_finite() && a.abs() < 1.57)
                || left >= 0. || right <= 0. || down >= 0. || up <= 0.
            {
                return Err("Invalid mouse viewport".into());
            }
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
            // Synthetic input cannot move the physical mouse or interrupt its drag.
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
    fn mouse_viewport_accepts_legacy_sessions_and_rejects_invalid_bounds() {
        let legacy: Session = serde_json::from_str("{}").unwrap();
        assert!(legacy.mouse_viewport.is_none());
        let mut b = Bridge::default();
        b.heartbeat(legacy).unwrap();
        for bounds in [
            [-0.7, 0.9, -0.6, 0.8],
            [0.2, 0.9, -0.6, 0.8],
            [-0.7, 0.9, 0.8, -0.6],
            [-0.7, f32::NAN, -0.6, 0.8],
            [-0.7, 1.6, -0.6, 0.8],
        ] {
            assert_eq!(b.heartbeat(Session {
                mouse_viewport: Some(bounds),
                ..Default::default()
            }).is_ok(), bounds == [-0.7, 0.9, -0.6, 0.8]);
        }
    }
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
    fn physical_mouse_rejects_synthetic_pointer_events_and_actions() {
        let mut b = Bridge::default();
        b.last_mouse = Some(Instant::now());
        for held in [false, true] {
            if held {
                b.last_mouse = Some(Instant::now() - Duration::from_secs(2));
                b.mouse_buttons.insert(0x110);
            }
            b.push(Packet {
                events: vec![SeatEvent::Motion { id: 7, uv: [0.5, 0.5] },
                    SeatEvent::Button { button: 0x110, pressed: true }, SeatEvent::Reset],
                actions: vec![crate::Action::Focus { id: 7 }],
            }).unwrap();
            assert!(b.packets.is_empty());
            assert!(b.last_input.is_none());
        }
        b.mouse_buttons.clear();
        b.last_mouse = None;
        b.push(Packet { events: vec![SeatEvent::Motion { id: 7, uv: [0.5, 0.5] }], actions: vec![] }).unwrap();
        assert_eq!(b.packets.len(), 1);
    }
    #[test]
    fn mouse_stays_owned_when_idle_and_renderer_lease_is_independent() {
        let mut b = Bridge::default();
        b.heartbeat(Session {
            visible: true,
            ..Session::default()
        })
        .unwrap();
        assert!(b.visible());
        b.last_mouse = Some(Instant::now() - Duration::from_secs(1));
        assert!(b.mouse_owned());
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
