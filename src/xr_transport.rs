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
    pub scale: f64,
    pub stamp: Instant,
}
pub const TOP_BAR: u64 = 0;
pub const NOTIFICATIONS: u64 = u64::MAX - 3;
pub const TASK_BAR: u64 = u64::MAX - 2;
pub const TOP_PANEL: u64 = u64::MAX - 5;
pub const BOTTOM_PANEL: u64 = u64::MAX - 6;
pub fn is_top(id: u64) -> bool { matches!(id, TOP_BAR | TOP_PANEL | NOTIFICATIONS) }
pub fn is_bottom(id: u64) -> bool { matches!(id, TASK_BAR | BOTTOM_PANEL) }
pub fn is_shell(id: u64) -> bool {
    is_top(id) || is_bottom(id)
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShellPullout {
    /// A dropdown originating in the universal menu or tray stays below the top bar.
    #[serde(default)]
    pub top_attached: bool,
    /// Taskbar previews and context menus extend upward from their taskbar origin.
    #[serde(default)]
    pub bottom_attached: bool,
    pub anchor: crate::Pose,
    pub viewport: [f32; 4],
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ShellLayout {
    pub width: u32,
    pub height: u32,
    pub top: u32,
    pub bottom: u32,
    #[serde(default)]
    pub notifications: Option<[u32; 4]>,
    #[serde(default)]
    pub pullout: Option<ShellPullout>,
}
impl ShellLayout {
    pub fn surface_rect(&self, id: u64) -> Option<[u32; 4]> {
        let separate = self.pullout.is_some();
        match id {
            TOP_BAR => Some([0, 0, self.width, if separate { self.top.min(36) } else { self.top }]),
            TASK_BAR => { let h = if separate { self.bottom.min(60) } else { self.bottom };
                Some([0, self.height.saturating_sub(h), self.width, h]) },
            TOP_PANEL if separate && self.top > 36 => Some([0, 36, self.width, self.top.min(self.height.saturating_sub(60)).saturating_sub(36)]),
            BOTTOM_PANEL if separate && self.bottom > 60 => Some([0, self.height.saturating_sub(self.bottom), self.width, self.bottom - 60]),
            NOTIFICATIONS => self.notifications,
            _ => None,
        }
    }
}
pub struct Bridge {
    pub webxr_input: Option<crate::webxr_focus::Policy>,
    pub webxr_input_updated: Option<Instant>,
    pub screenshot_request: Option<u64>,
    pub screenshot_result: Option<Result<crate::screenshot::Capture,String>>,
    pub gpu_socket: Option<String>,
    #[cfg(feature = "nested")]
    pub gpu_frames: HashMap<u64, Arc<crate::xr_gpu::Published>>,
    pub gpu_requested: HashMap<u64, Instant>,
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
    pub light: bool,
    pub mouse_angles: Option<[f32; 2]>,
    pub last_mouse: Option<Instant>,
    pub mouse_buttons: std::collections::HashSet<u32>,
    pub input_realm: crate::Realm,
}
impl Default for Bridge {
    fn default() -> Self {
        Self {
            webxr_input: None,
            webxr_input_updated: None,
            screenshot_request: None,
            screenshot_result: None,
            gpu_socket: None,
            #[cfg(feature = "nested")]
            gpu_frames: HashMap::new(),
            gpu_requested: HashMap::new(),
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
            light: false,
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
    /// Runtime-predicted display period; zero means an older renderer without timing.
    pub frame_period_ns: u64,
    /// Shared eye viewport angles in radians: left, right, down, up.
    pub mouse_viewport: Option<[f32; 4]>,
    /// Runtime eye poses for in-window stereo clients, in stage coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eyes: Option<[crate::Pose; 2]>,
}
impl Session {
    pub fn stereo_wire_value(&self, timing_supported: bool, stereo_supported: bool) -> serde_json::Value {
        let mut value = self.wire_value(timing_supported);
        if !stereo_supported { value.as_object_mut().unwrap().remove("eyes"); }
        value
    }
    pub fn wire_value(&self, timing_supported: bool) -> serde_json::Value {
        let mut value = serde_json::to_value(self).unwrap();
        // A renderer may be restarted after installation while the previous
        // compositor still owns the desktop; its strict legacy schema must work.
        if !timing_supported { value.as_object_mut().unwrap().remove("frame_period_ns"); }
        value
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub id: u64,
    pub uv: [f32; 2],
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Control {
    /// Native XR is session-only; Scene deliberately omits its lease from JSON.
    #[serde(default)]
    pub native_xr_active: bool,
    /// Absence keeps older desktops in head/stereo-only compatibility mode.
    #[serde(default)]
    pub webxr_input: Option<crate::webxr_focus::Policy>,
    #[serde(default)]
    pub webxr_input_age_ms: Option<u64>,
    /// Shared popup geometry capability; absent on an already-running older desktop.
    #[serde(default)]
    pub centered_popups: bool,
    /// Native rollouts share their owning UI plane instead of popup depth.
    #[serde(default)]
    pub flat_rollouts: bool,
    #[serde(default)]
    pub stereo_views: bool,
    #[serde(default)]
    pub screenshot_request: Option<u64>,
    /// New helpers export scene geometry; old desktops retain SBS PNG replies.
    #[serde(default)]
    pub screenshot_gltf: bool,
    /// Presence advertises support for runtime timing in session heartbeats.
    #[serde(default)]
    pub frame_period_ns: Option<u64>,
    #[serde(default)]
    pub gpu_socket: Option<String>,
    #[serde(default)]
    pub finder_seconds: Option<f32>,
    #[serde(default = "default_accent")]
    pub accent: [u8; 3],
    /// Shell Appearance mode; older compositors default to the dark theme.
    #[serde(default)]
    pub light: bool,
    pub input_realm: crate::Realm,
    pub visible: bool,
    pub mouse: bool,
    pub cursor: Option<Cursor>,
    #[serde(default)]
    pub mouse_angles: Option<[f32; 2]>,
    pub exit_requested: bool,
}
fn default_accent() -> [u8; 3] { crate::visual_feedback::ACCENT }
/// Zero preserves compatibility with renderers that do not report timing.
pub fn frame_interval(period_ns: u64) -> Option<std::time::Duration> {
    (1_000_000..=100_000_000).contains(&period_ns)
        .then(|| std::time::Duration::from_nanos(period_ns))
}
pub fn refresh_millihz(period_ns: u64) -> Option<i32> {
    frame_interval(period_ns).map(|_| ((1_000_000_000_000u64 + period_ns / 2) / period_ns) as i32)
}
/// Keep deadlines on the requested cadence without accumulating render time or
/// replaying missed frames after a stall or a runtime rate change.
pub fn next_frame_deadline(previous: Instant, now: Instant, interval: std::time::Duration) -> Instant {
    let next = previous + interval;
    if next > now { next } else { now + interval }
}
impl Bridge {
    pub fn frame_interval(&self) -> Option<std::time::Duration> {
        self.visible().then(|| frame_interval(self.session.frame_period_ns)).flatten()
    }
    /// Keep identity stable across unrelated shell invalidations. GPU consumers
    /// may retain the corresponding texture until these pixels actually change.
    pub fn update_surface(&mut self, id: u64, surface: Surface) {
        if let Some(old) = self.frames.get_mut(&id) {
            if old.width == surface.width && old.height == surface.height
                && old.scale == surface.scale && old.rgba == surface.rgba {
                old.stamp = surface.stamp;
                return;
            }
        }
        self.frames.insert(id, surface);
    }
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
            native_xr_active: false,
            webxr_input: self.webxr_input.clone(),
            webxr_input_age_ms: self.webxr_input_updated.map(|time|
                u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX)),
            centered_popups: true,
            flat_rollouts: true,
            stereo_views: true,
            screenshot_request: self.screenshot_request,
            screenshot_gltf: true,
            frame_period_ns: Some(self.frame_interval().map_or(0, |period| period.as_nanos() as u64)),
            gpu_socket: self.gpu_socket.clone(),
            finder_seconds: self.finder_started.map(|t| t.elapsed().as_secs_f32())
                .filter(|t| self.input_realm == crate::Realm::Xr && *t < crate::visual_feedback::FINDER_DURATION),
            accent: self.accent,
            light: self.light,
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
        if let Some(eyes) = session.eyes {
            for eye in eyes { eye.validate()?; }
            let separation = glam::Vec3::from_array(eyes[1].position_m)
                .distance(glam::Vec3::from_array(eyes[0].position_m));
            if !(0.035..=0.09).contains(&separation) {
                return Err("Invalid eye separation".into());
            }
        }
        if session.frame_period_ns != 0 && frame_interval(session.frame_period_ns).is_none() {
            return Err("Invalid XR display period".into());
        }
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
    fn gltf_capture_is_advertised_without_breaking_legacy_control() {
        let bridge=Bridge::default();
        assert!(bridge.control().screenshot_gltf);
        let mut legacy=serde_json::to_value(bridge.control()).unwrap();
        legacy.as_object_mut().unwrap().remove("screenshot_gltf");
        assert!(!serde_json::from_value::<Control>(legacy).unwrap().screenshot_gltf);
    }
    #[test]
    fn stereo_eyes_are_validated_and_omitted_for_legacy_compositors() {
        let mut eyes = [crate::Pose::default(); 2];
        eyes[0].position_m[0] = -0.032;
        eyes[1].position_m[0] = 0.032;
        let session = Session { eyes: Some(eyes), ..Default::default() };
        assert!(session.stereo_wire_value(true, false).get("eyes").is_none());
        assert!(session.stereo_wire_value(true, true).get("eyes").is_some());
        let mut bridge = Bridge::default();
        assert!(bridge.control().stereo_views);
        bridge.heartbeat(session.clone()).unwrap();
        for separation in [0., 0.02, 0.1, 10.] {
            let mut bad=session.clone();
            bad.eyes.as_mut().unwrap()[1].position_m[0] = -0.032+separation;
            assert!(bridge.heartbeat(bad).is_err());
        }
    }
    #[test]
    fn cursor_appearance_crosses_transport_and_preserves_reticle_geometry() {
        let mut bridge = Bridge::default();
        let mut legacy = serde_json::to_value(bridge.control()).unwrap();
        legacy.as_object_mut().unwrap().remove("light");
        assert!(!serde_json::from_value::<Control>(legacy).unwrap().light);
        bridge.light = true;
        let control: Control = serde_json::from_value(serde_json::to_value(bridge.control()).unwrap()).unwrap();
        let dark = crate::xr_input::cursor_pixels(false);
        let light = crate::xr_input::cursor_pixels(control.light);
        let center = (15 * 32 + 15) * 4;
        assert_eq!(&dark[center..center + 4], &[245, 245, 245, 255]);
        assert_eq!(&light[center..center + 4], &[20, 20, 20, 255]);
        for (a, b) in dark.chunks_exact(4).zip(light.chunks_exact(4)) {
            assert_eq!(a[3], b[3]);
            if a[3] != 0 {
                assert_eq!(a[0] as u16 + b[0] as u16, 265);
                assert_eq!(a[3], 255);
            }
        }
    }
    #[test]
    fn runtime_timing_tracks_rate_changes_and_expires_with_the_session() {
        assert_eq!(serde_json::from_str::<Session>("{}").unwrap().frame_period_ns, 0);
        assert!(Session::default().wire_value(false).get("frame_period_ns").is_none());
        assert_eq!(Session::default().wire_value(true)["frame_period_ns"], 0);
        let mut b = Bridge::default();
        for (period, rate) in [(13_888_889, 72_000), (11_111_111, 90_000), (8_333_333, 120_000)] {
            b.heartbeat(Session { visible: true, frame_period_ns: period, ..Default::default() }).unwrap();
            assert_eq!(b.frame_interval().unwrap().as_nanos(), period as u128);
            assert_eq!(refresh_millihz(period), Some(rate));
        }
        for period in [1, 999_999, 100_000_001, u64::MAX] {
            assert!(b.heartbeat(Session { frame_period_ns: period, ..Default::default() }).is_err());
        }
        b.last_session = Some(Instant::now() - Duration::from_secs(3));
        assert!(b.frame_interval().is_none());
        let now = Instant::now(); let interval = Duration::from_nanos(8_333_333);
        assert_eq!(next_frame_deadline(now, now + Duration::from_millis(2), interval), now + interval);
        assert_eq!(next_frame_deadline(now, now + Duration::from_secs(1), interval), now + Duration::from_secs(1) + interval);
    }
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

#[cfg(test)]
mod surface_cache_tests {
    use super::*;
    fn surface(width: u32, pixel: u8) -> Surface { Surface {width, height:1,
        rgba:Arc::new(vec![pixel; width as usize*4]), scale:1., stamp:Instant::now()} }
    #[test]
    fn unchanged_shell_keeps_gpu_cache_identity() {
        let mut bridge=Bridge::default();bridge.update_surface(TOP_BAR,surface(2,10));
        let first=bridge.frames[&TOP_BAR].rgba.clone();
        bridge.update_surface(TOP_BAR,surface(2,10));
        assert!(Arc::ptr_eq(&first,&bridge.frames[&TOP_BAR].rgba));
        bridge.update_surface(TOP_BAR,surface(2,11));
        assert!(!Arc::ptr_eq(&first,&bridge.frames[&TOP_BAR].rgba));
        let next=bridge.frames[&TOP_BAR].rgba.clone();
        bridge.update_surface(TOP_BAR,surface(3,11));
        assert!(!Arc::ptr_eq(&next,&bridge.frames[&TOP_BAR].rgba));
    }
}
