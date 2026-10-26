//! Version-one WiVRn/WayVR control protocol. All queues are bounded; renderers
//! exchange head-relative rays so independent OpenXR reference spaces agree.
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub const VERSION: u32 = 1;
pub const LEASE: Duration = Duration::from_millis(200);
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ray {
    pub tracked: bool,
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub buttons: u8,
    pub scroll: [f64; 2],
    pub max_distance: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub version: u32,
    pub sequence: u64,
    pub time_ns: u64,
    pub rays: [Ray; 2],
}
impl Frame {
    pub fn valid(&self, now: u64) -> bool {
        self.version == VERSION
            && self.sequence > 0
            && self.time_ns <= now.saturating_add(50_000_000)
            && now.saturating_sub(self.time_ns) <= 150_000_000
            && self.rays.iter().all(|r| {
                r.origin.iter().all(|x| x.is_finite() && x.abs() < 5.)
                    && r.direction.iter().all(|x| x.is_finite())
                    && (!r.tracked
                        || (0.9..1.1).contains(&r.direction.iter().map(|x| x * x).sum::<f32>()))
                    && r.buttons & !7 == 0
                    && r.scroll.iter().all(|x| x.is_finite() && x.abs() <= 20.)
                    && r.max_distance.is_finite()
                    && (0.0..=100.).contains(&r.max_distance)
            })
    }
}
pub fn monotonic_ns() -> u64 {
    nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC)
        .map(|t| t.tv_sec() as u64 * 1_000_000_000 + t.tv_nsec() as u64)
        .unwrap_or(0)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    ToggleDesktop,
    Launcher,
    Screenshot,
    Terminal,
    Stop,
}
impl Command {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "toggle_desktop" => Self::ToggleDesktop,
            "launcher" => Self::Launcher,
            "screenshot" => Self::Screenshot,
            "terminal" => Self::Terminal,
            "stop" => Self::Stop,
            _ => return None,
        })
    }
}
#[derive(Clone)]
pub struct Notice {
    pub id: String,
    pub title: String,
    pub body: String,
    pub timeout: i32,
}
pub struct Control {
    pub notices: VecDeque<Notice>,
    pub owns_bus: bool,
    pub commands: VecDeque<Command>,
    pub frames: VecDeque<(String, Frame)>,
    pub status: serde_json::Value,
    pub hits: [Option<f32>; 2],
    pub sequence: u64,
    pub owner: String,
    pub received: Option<Instant>,
    pub hand: Option<usize>,
    pub buttons: u8,
    pub armed: bool,
    pub target: Option<u64>,
    pub last_sequence: u64,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            notices: VecDeque::new(),
            owns_bus: false,
            commands: VecDeque::new(),
            frames: VecDeque::new(),
            status: serde_json::json!({"version":VERSION,"ready":false}),
            hits: [None; 2],
            sequence: 0,
            owner: String::new(),
            received: None,
            hand: None,
            buttons: 0,
            armed: false,
            target: None,
            last_sequence: 0,
        }
    }
}
pub type Shared = Arc<Mutex<Control>>;
impl Control {
    pub fn enqueue(&mut self, command: Command) -> bool {
        if self.commands.len() >= 32 {
            return false;
        }
        self.commands.push_back(command);
        true
    }
    pub fn submit(&mut self, owner: String, frame: Frame, now: u64) -> bool {
        if !frame.valid(now) || self.frames.len() >= 32 {
            return false;
        }
        // A second connection cannot steal a currently held pointer lease.
        if !self.owner.is_empty()
            && self.owner != owner
            && self.buttons != 0
            && self.received.is_some_and(|t| t.elapsed() < LEASE)
        {
            return false;
        }
        self.frames.push_back((owner, frame));
        true
    }
    pub fn feedback(&self) -> String {
        serde_json::json!({"version":VERSION,"sequence":self.sequence,"hits":self.hits}).to_string()
    }
}
pub fn stop_path() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|p| std::path::PathBuf::from(p).join("dome/explicit-stop"))
}
pub fn explicit_stop() {
    if let Some(path) = stop_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, b"explicit compositor stop\n");
    }
}
pub fn resumed() {
    if let Some(path) = stop_path() {
        let _ = std::fs::remove_file(path);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Frame {
        let ray = Ray {
            tracked: true,
            origin: [0.; 3],
            direction: [0., 0., -1.],
            buttons: 0,
            scroll: [0.; 2],
            max_distance: 100.,
        };
        Frame {
            version: 1,
            sequence: 1,
            time_ns: 1_000_000_000,
            rays: [ray.clone(), ray],
        }
    }
    #[test]
    fn protocol_rejects_stale_nonfinite_and_invalid_input() {
        let mut f = frame();
        assert!(f.valid(f.time_ns));
        assert!(!f.valid(f.time_ns + 200_000_000));
        f.rays[0].direction[0] = f32::NAN;
        assert!(!f.valid(f.time_ns));
        f = frame();
        f.rays[1].buttons = 8;
        assert!(!f.valid(f.time_ns));
        f = frame();
        f.rays[1].direction = [0.; 3];
        assert!(!f.valid(f.time_ns));
    }
    #[test]
    fn bounded_queues_and_held_owner() {
        let mut c = Control::default();
        for _ in 0..32 {
            assert!(c.enqueue(Command::Launcher));
        }
        assert!(!c.enqueue(Command::Stop));
        c.owner = "first".into();
        c.buttons = 1;
        c.received = Some(Instant::now());
        assert!(!c.submit("second".into(), frame(), 1_000_000_000));
        assert!(c.submit("first".into(), frame(), 1_000_000_000));
        assert!(Command::parse("shell arbitrary").is_none());
    }
}
