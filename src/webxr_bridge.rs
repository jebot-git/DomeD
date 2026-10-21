//! Same-user local browser projection transport into Dome's existing XR session.
//! No OpenXR objects or runtime connections are created by this module.
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub const MAX_DIMENSION: u32 = 4096;
const MAX_JSON: usize = 65536;
const LEASE_AGE: Duration = Duration::from_secs(5);
const CONTENT_AGE: Duration = Duration::from_millis(500);
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Pose {
    pub position: [f32; 3],
    pub orientation: [f32; 4],
}
impl Default for Pose {
    fn default() -> Self {
        Self {
            position: [0.; 3],
            orientation: [0., 0., 0., 1.],
        }
    }
}
impl Pose {
    fn valid(&self) -> bool {
        self.position
            .iter()
            .all(|v| v.is_finite() && v.abs() < 10000.)
            && self.orientation.iter().all(|v| v.is_finite())
            && (self.orientation.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.01
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Fov {
    pub left: f32,
    pub right: f32,
    pub up: f32,
    pub down: f32,
}
impl Fov {
    fn valid(&self) -> bool {
        [self.left, self.right, self.up, self.down]
            .iter()
            .all(|v| v.is_finite() && v.abs() < 1.57)
            && self.left < self.right
            && self.down < self.up
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub pose: Pose,
    pub fov: Fov,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub view_sizes: [[u32; 2]; 2],
    pub max_width: u32,
    pub max_height: u32,
    pub blend_modes: Vec<String>,
    pub stage: bool,
    pub period: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tracking {
    pub frame_id: u64,
    pub time: i64,
    pub period: i64,
    pub should_render: bool,
    pub visible: bool,
    pub focused: bool,
    pub head_flags: u64,
    pub view_flags: u64,
    pub head: Pose,
    pub views: [View; 2],
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectionView {
    pub rect: Rect,
    pub pose: Pose,
    pub fov: Fov,
}
#[derive(Debug)]
pub struct Submission {
    pub frame_id: u64,
    pub layer_flags: u64,
    pub width: u32,
    pub height: u32,
    pub views: [ProjectionView; 2],
    pub rgba: Vec<u8>,
    generation: u64,
    received: Instant,
    acknowledgement: Option<mpsc::Sender<Result<(), String>>>,
}
impl Submission {
    pub fn complete(mut self, result: Result<(), String>) {
        if let Some(reply) = self.acknowledgement.take() {
            let _ = reply.send(result);
        }
    }
}
impl Drop for Submission {
    fn drop(&mut self) {
        if let Some(reply) = self.acknowledgement.take() {
            let _ = reply.send(Err("Compositor discarded browser frame".into()));
        }
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Stats {
    pub sessions: u64,
    pub received_frames: u64,
    pub composed_frames: u64,
    pub rejected: u64,
}
struct Lease {
    token: String,
    pid: u32,
    touched: Instant,
}
struct State {
    caps: Capabilities,
    tracking: Option<Tracking>,
    active: bool,
    lease: Option<Lease>,
    pending: Option<Submission>,
    content: bool,
    generation: u64,
    stats: Stats,
}
impl State {
    fn authorize(&mut self, token: &str, pid: u32) -> Result<(), String> {
        if self
            .lease
            .as_ref()
            .is_some_and(|l| l.touched.elapsed() > LEASE_AGE)
        {
            self.lease = None;
            self.pending = None;
            self.content = false;
        }
        let lease = self.lease.as_mut().ok_or("No active browser lease")?;
        if lease.pid != pid || lease.token != token {
            return Err("Invalid browser lease".into());
        }
        lease.touched = Instant::now();
        Ok(())
    }
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    stopped: AtomicBool,
    clients: AtomicUsize,
}
/// Bound to one sidecar and one real headset session. Drop closes the endpoint.
pub struct Server {
    shared: Arc<Shared>,
    path: PathBuf,
    identity: (u64, u64),
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub fn bind(path: &Path, caps: Capabilities) -> io::Result<Self> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) => {
                if !meta.file_type().is_socket()
                    || meta.uid() != nix::unistd::getuid().as_raw()
                    || !matches!(UnixStream::connect(path), Err(e)
                        if e.kind() == io::ErrorKind::ConnectionRefused)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "WebXR endpoint is active or is not an owned stale socket",
                    ));
                }
                std::fs::remove_file(path)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        let metadata = std::fs::symlink_metadata(path)?;
        let identity = (metadata.dev(), metadata.ino());
        listener.set_nonblocking(true)?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                caps,
                tracking: None,
                active: false,
                lease: None,
                pending: None,
                content: false,
                generation: 0,
                stats: Stats::default(),
            }),
            changed: Condvar::new(),
            stopped: AtomicBool::new(false),
            clients: AtomicUsize::new(0),
        });
        let service = shared.clone();
        let worker = thread::spawn(move || {
            while !service.stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let valid = nix::sys::socket::getsockopt(
                            &stream,
                            nix::sys::socket::sockopt::PeerCredentials,
                        )
                        .is_ok_and(|peer| peer.uid() == nix::unistd::getuid().as_raw());
                        if !valid || service.clients.fetch_add(1, Ordering::AcqRel) >= 8 {
                            if valid {
                                service.clients.fetch_sub(1, Ordering::AcqRel);
                            }
                            continue;
                        }
                        let client = service.clone();
                        thread::spawn(move || {
                            let _guard = ClientGuard(client.clone());
                            let mut stream = stream;
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(1500)));
                            let answer = handle(&mut stream, &client).unwrap_or_else(
                                |error| serde_json::json!({"ok":false,"error":error}),
                            );
                            if !answer.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                                client.state.lock().unwrap().stats.rejected += 1;
                            }
                            if let Ok(bytes) = serde_json::to_vec(&answer) {
                                let _ = stream.write_all(&(bytes.len() as u32).to_le_bytes());
                                let _ = stream.write_all(&bytes);
                            }
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            shared,
            path: path.into(),
            identity,
            worker: Some(worker),
        })
    }
    pub fn publish(&self, tracking: Tracking) {
        let mut state = self.shared.state.lock().unwrap();
        state.active = true;
        state.caps.period = tracking.period;
        state.tracking = Some(tracking);
        if state
            .lease
            .as_ref()
            .is_some_and(|l| l.touched.elapsed() > LEASE_AGE)
        {
            state.lease = None;
            state.pending = None;
            state.content = false;
            state.generation = state.generation.wrapping_add(1);
        }
        drop(state);
        self.shared.changed.notify_all();
    }
    pub fn suspend(&self) {
        let mut state = self.shared.state.lock().unwrap();
        state.active = false;
        state.pending = None;
        state.lease = None;
        state.content = false;
        state.generation = state.generation.wrapping_add(1);
        drop(state);
        self.shared.changed.notify_all();
    }
    pub fn take_submission(&self) -> Option<Submission> {
        let mut state = self.shared.state.lock().unwrap();
        let frame = state.pending.take()?;
        if frame.received.elapsed() > CONTENT_AGE {
            return None;
        }
        Some(frame)
    }
    pub fn composed(&self, frame: &Submission) -> bool {
        let mut state = self.shared.state.lock().unwrap();
        if !state.active
            || state.generation != frame.generation
            || !state
                .lease
                .as_ref()
                .is_some_and(|l| l.touched.elapsed() < LEASE_AGE)
        {
            return false;
        }
        state.stats.composed_frames += 1;
        state.content = true;
        true
    }
    pub fn content_active(&self, last_frame: Option<Instant>) -> bool {
        let state = self.shared.state.lock().unwrap();
        state.active
            && state.content
            && state
                .lease
                .as_ref()
                .is_some_and(|l| l.touched.elapsed() < LEASE_AGE)
            && last_frame.is_some_and(|t| t.elapsed() < CONTENT_AGE)
    }
    pub fn stats(&self) -> Stats {
        self.shared.state.lock().unwrap().stats.clone()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        self.suspend();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // Another sidecar may bind the same name after our listener closes.
        // Never unlink its socket or a file replacing our endpoint.
        if std::fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.file_type().is_socket() && (metadata.dev(), metadata.ino()) == self.identity
        }) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
struct ClientGuard(Arc<Shared>);
impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.clients.fetch_sub(1, Ordering::AcqRel);
    }
}
fn fail_io(error: io::Error) -> String {
    error.to_string()
}
fn handle(stream: &mut UnixStream, shared: &Shared) -> Result<serde_json::Value, String> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).map_err(fail_io)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_JSON {
        return Err("Invalid request size".into());
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).map_err(fail_io)?;
    let request: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let peer = nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerCredentials)
        .map_err(|e| e.to_string())?;
    let pid = peer.pid() as u32;
    let op = request["op"].as_str().ok_or("Missing operation")?;
    if op == "hello" {
        if request["version"].as_u64() != Some(1) {
            return Err("Unsupported protocol version".into());
        }
        let state = shared.state.lock().unwrap();
        let mut reply = serde_json::to_value(&state.caps).map_err(|e| e.to_string())?;
        reply["ok"] = true.into();
        reply["version"] = 1.into();
        reply["transports"] = serde_json::json!(["rgba"]);
        return Ok(reply);
    }
    if op == "begin_session" {
        let mut state = shared.state.lock().unwrap();
        if !state.active {
            return Err("Dome XR session is not running".into());
        }
        if state
            .lease
            .as_ref()
            .is_some_and(|l| l.touched.elapsed() < LEASE_AGE)
        {
            return Err("Another browser session is active".into());
        }
        let mut random = [0u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .map_err(fail_io)?;
        let token = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        state.pending = None;
        state.content = false;
        state.generation = state.generation.wrapping_add(1);
        state.lease = Some(Lease {
            token: token.clone(),
            pid,
            touched: Instant::now(),
        });
        state.stats.sessions += 1;
        return Ok(serde_json::json!({"ok":true,"token":token}));
    }
    let token = request["token"].as_str().ok_or("Missing session token")?;
    {
        let mut state = shared.state.lock().unwrap();
        state.authorize(token, pid)?;
        if !state.active {
            return Err("Dome XR session stopped".into());
        }
    }
    match op {
        "end_session" => {
            let mut state = shared.state.lock().unwrap();
            state.authorize(token, pid)?;
            state.lease = None;
            state.pending = None;
            state.content = false;
            state.generation = state.generation.wrapping_add(1);
            drop(state);
            shared.changed.notify_all();
            Ok(serde_json::json!({"ok":true}))
        }
        "wait_frame" => {
            let after = request["after"].as_u64().unwrap_or(0);
            let mut state = shared.state.lock().unwrap();
            let until = Instant::now() + Duration::from_secs(1);
            loop {
                state.authorize(token, pid)?;
                if !state.active {
                    return Err("Dome XR session stopped".into());
                }
                if let Some(frame) = state.tracking.as_ref().filter(|f| f.frame_id > after) {
                    let mut reply = serde_json::to_value(frame).map_err(|e| e.to_string())?;
                    reply["ok"] = true.into();
                    return Ok(reply);
                }
                let remaining = until.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err("Dome frame timeout".into());
                }
                state = shared.changed.wait_timeout(state, remaining).unwrap().0;
            }
        }
        "end_frame" => {
            if request["empty"].as_bool() == Some(true) {
                let mut state = shared.state.lock().unwrap();
                state.authorize(token, pid)?;
                state.pending = None;
                state.content = false;
                state.generation = state.generation.wrapping_add(1);
                return Ok(serde_json::json!({"ok":true}));
            }
            if request["transport"].as_str() != Some("rgba") {
                return Err("Only RGBA transport is supported by this host".into());
            }
            let u32_field = |key: &str| {
                request[key]
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| format!("Invalid {key}"))
            };
            let width = u32_field("width")?;
            let height = u32_field("height")?;
            if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
                return Err("Invalid projection dimensions".into());
            }
            let length =
                usize::try_from(width as u64 * height as u64 * 4).map_err(|_| "Frame too large")?;
            if request["bytes"].as_u64() != Some(length as u64) {
                return Err("Projection byte count mismatch".into());
            }
            let views: [ProjectionView; 2] =
                serde_json::from_value(request["views"].clone()).map_err(|e| e.to_string())?;
            for view in &views {
                let r = view.rect;
                if !view.pose.valid()
                    || !view.fov.valid()
                    || r.width == 0
                    || r.height == 0
                    || r.x.checked_add(r.width).is_none_or(|v| v > width)
                    || r.y.checked_add(r.height).is_none_or(|v| v > height)
                {
                    return Err("Invalid projection view".into());
                }
            }
            let layer_flags = request["layer_flags"].as_u64().unwrap_or(0);
            if layer_flags & !7 != 0 {
                return Err("Unsupported composition flags".into());
            }
            let frame_id = request["frame_id"].as_u64().ok_or("Missing frame id")?;
            {
                let state = shared.state.lock().unwrap();
                if state.tracking.as_ref().is_none_or(|f| {
                    frame_id > f.frame_id || f.frame_id.saturating_sub(frame_id) > 120
                }) {
                    return Err("Invalid projection frame id".into());
                }
            }
            let mut rgba = vec![0; length];
            stream.read_exact(&mut rgba).map_err(fail_io)?;
            let (send, receive) = mpsc::channel();
            {
                let mut state = shared.state.lock().unwrap();
                state.authorize(token, pid)?;
                state.pending = Some(Submission {
                    frame_id,
                    layer_flags,
                    width,
                    height,
                    views,
                    rgba,
                    generation: state.generation,
                    received: Instant::now(),
                    acknowledgement: Some(send),
                });
                state.stats.received_frames += 1;
            }
            match receive.recv_timeout(Duration::from_millis(1200)) {
                Ok(Ok(())) => Ok(serde_json::json!({"ok":true})),
                Ok(Err(error)) => Err(error),
                Err(_) => Err("Dome did not consume projection frame".into()),
            }
        }
        _ => Err("Unknown operation".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn caps() -> Capabilities {
        Capabilities {
            name: "Test session".into(),
            width: 128,
            height: 64,
            view_sizes: [[64, 64]; 2],
            max_width: 4096,
            max_height: 4096,
            blend_modes: vec!["opaque".into()],
            stage: false,
            period: 13_888_888,
        }
    }
    fn tracking(id: u64) -> Tracking {
        let view = View {
            pose: Pose::default(),
            fov: Fov {
                left: -0.7,
                right: 0.7,
                up: 0.7,
                down: -0.7,
            },
        };
        Tracking {
            frame_id: id,
            time: id as i64 * 13_888_888,
            period: 13_888_888,
            should_render: true,
            visible: true,
            focused: true,
            head_flags: 15,
            view_flags: 15,
            head: Pose::default(),
            views: [view.clone(), view],
        }
    }
    fn request(path: &Path, value: serde_json::Value, payload: &[u8]) -> serde_json::Value {
        let mut socket = UnixStream::connect(path).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let bytes = serde_json::to_vec(&value).unwrap();
        socket
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .unwrap();
        socket.write_all(&bytes).unwrap();
        socket.write_all(payload).unwrap();
        let mut size = [0; 4];
        socket.read_exact(&mut size).unwrap();
        let mut reply = vec![0; u32::from_le_bytes(size) as usize];
        socket.read_exact(&mut reply).unwrap();
        serde_json::from_slice(&reply).unwrap()
    }
    #[test]
    fn endpoint_restart_preserves_live_sockets_and_regular_files() {
        let path =
            std::env::temp_dir().join(format!("dome-webxr-restart-{}.sock", std::process::id()));
        std::fs::write(&path, b"keep this file").unwrap();
        assert!(Server::bind(&path, caps()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep this file");
        std::fs::remove_file(&path).unwrap();
        let stale = UnixListener::bind(&path).unwrap();
        drop(stale);
        let server = Server::bind(&path, caps()).unwrap();
        assert!(Server::bind(&path, caps()).is_err());
        drop(server);
        assert!(!path.exists());
        let server = Server::bind(&path, caps()).unwrap();
        std::fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        drop(server);
        assert!(
            path.exists(),
            "Shutdown must preserve a replacement endpoint"
        );
        assert!(UnixStream::connect(&path).is_ok());
        drop(replacement);
        std::fs::remove_file(&path).unwrap();
        let server = Server::bind(&path, caps()).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"replacement file").unwrap();
        drop(server);
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement file");
        std::fs::remove_file(&path).unwrap();
    }
    #[test]
    fn actual_frame_lease_and_safe_upload_acknowledgement() {
        let path =
            std::env::temp_dir().join(format!("dome-webxr-test-{}.sock", std::process::id()));
        let server = Server::bind(&path, caps()).unwrap();
        assert_eq!(
            request(&path, serde_json::json!({"op":"hello","version":1}), &[])["name"],
            "Test session"
        );
        assert_eq!(
            request(&path, serde_json::json!({"op":"begin_session"}), &[])["ok"],
            false
        );
        server.publish(tracking(42));
        let token = request(&path, serde_json::json!({"op":"begin_session"}), &[])["token"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            request(&path, serde_json::json!({"op":"begin_session"}), &[])["ok"],
            false
        );
        let actual = request(
            &path,
            serde_json::json!({"op":"wait_frame","token":token,"after":0}),
            &[],
        );
        assert_eq!(actual["frame_id"], 42);
        assert_eq!(actual["head_flags"], 15);
        let view = ProjectionView {
            rect: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            pose: Pose::default(),
            fov: tracking(42).views[0].fov,
        };
        let message = serde_json::json!({"op":"end_frame","token":token,"frame_id":42,"width":1,"height":1,"bytes":4,"transport":"rgba","views":[view.clone(),view]});
        for invalid in [
            serde_json::json!({"width":MAX_DIMENSION + 1}),
            serde_json::json!({"bytes":8}),
            serde_json::json!({"frame_id":43}),
            serde_json::json!({"layer_flags":8}),
        ] {
            let mut bad = message.clone();
            for (key, value) in invalid.as_object().unwrap() {
                bad[key] = value.clone();
            }
            assert_eq!(request(&path, bad, &[])["ok"], false);
        }
        let later_message = message.clone();
        let target = path.clone();
        let client = thread::spawn(move || request(&target, message, &[23, 45, 67, 255]));
        let until = Instant::now() + Duration::from_secs(1);
        let frame = loop {
            if let Some(frame) = server.take_submission() {
                break frame;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(frame.rgba, [23, 45, 67, 255]);
        assert!(!client.is_finished());
        assert!(server.composed(&frame));
        frame.complete(Ok(()));
        assert_eq!(client.join().unwrap()["ok"], true);
        assert!(server.content_active(Some(Instant::now())));
        let target = path.clone();
        let client = thread::spawn(move || request(&target, later_message, &[23, 45, 67, 255]));
        let until = Instant::now() + Duration::from_secs(1);
        let frame = loop {
            if let Some(frame) = server.take_submission() {
                break frame;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(
            request(
                &path,
                serde_json::json!({"op":"end_frame","token":token,"frame_id":42,"empty":true}),
                &[]
            )["ok"],
            true
        );
        assert!(
            !server.composed(&frame),
            "An empty frame must invalidate an upload already in progress"
        );
        frame.complete(Err("Projection was cleared during upload".into()));
        assert_eq!(client.join().unwrap()["ok"], false);
        assert!(!server.content_active(Some(Instant::now())));
        assert_eq!(
            request(
                &path,
                serde_json::json!({"op":"end_session","token":"invalid"}),
                &[]
            )["ok"],
            false
        );
        assert_eq!(
            request(
                &path,
                serde_json::json!({"op":"end_session","token":token}),
                &[]
            )["ok"],
            true
        );
        assert!(!server.content_active(Some(Instant::now())));
        assert_eq!(server.stats().composed_frames, 1);
    }
}
