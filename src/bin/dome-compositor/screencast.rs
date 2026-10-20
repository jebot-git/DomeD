//! ScreenCast v3 backend. Consent is per session; only the portal frontend can
//! call it. GPU readback and PipeWire work occur only for explicitly shared outputs.
use smithay::{
    backend::renderer::{
        damage::OutputDamageTracker,
        element::{
            utils::{Relocate, RelocateRenderElement},
            RenderElement,
        },
        gles::{GlesRenderer, GlesTarget},
        Bind, ExportMem, Offscreen, TextureMapping,
    },
    output::Output,
    utils::{Logical, Point, Rectangle},
};
use std::{
    collections::HashMap,
    ffi::{c_void, CString},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use zbus::{
    message::Header,
    zvariant::{OwnedObjectPath, OwnedValue, Str, Value},
};

const BUS: &str = "org.freedesktop.impl.portal.desktop.dome.screencast";
const PATH: &str = "/org/freedesktop/portal/desktop";
type Dict = HashMap<String, OwnedValue>;
type Reply = (u32, Dict);
type SharedSession = Arc<Mutex<SessionData>>;
static STATE: OnceLock<Arc<Mutex<Manager>>> = OnceLock::new();
unsafe extern "C" {
    fn dome_cast_create(name: *const std::ffi::c_char, width: u32, height: u32) -> *mut c_void;
    fn dome_cast_destroy(stream: *mut c_void);
    fn dome_cast_node(stream: *mut c_void) -> u32;
    fn dome_cast_frame(stream: *mut c_void, bytes: *const u8, length: usize, flipped: i32) -> i32;
}
struct Stream(*mut c_void);
// The C implementation serializes access with its pw_thread_loop lock.
unsafe impl Send for Stream {}
impl Drop for Stream {
    fn drop(&mut self) {
        unsafe { dome_cast_destroy(self.0) }
    }
}
impl Stream {
    fn new(output: &Monitor) -> Result<Self, String> {
        let name = CString::new(format!("DomeD {}", output.name)).map_err(|e| e.to_string())?;
        let raw =
            unsafe { dome_cast_create(name.as_ptr(), output.width as u32, output.height as u32) };
        if raw.is_null() {
            Err("PipeWire stream creation failed (is PipeWire running?)".into())
        } else {
            Ok(Self(raw))
        }
    }
    fn node(&self) -> u32 {
        unsafe { dome_cast_node(self.0) }
    }
}
#[derive(Clone, serde::Serialize, PartialEq, Eq)]
struct Monitor {
    name: String,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
}
struct Sharing {
    monitor: Monitor,
    stream: Stream,
    cursor: u32,
    last: Instant,
}
#[derive(Default)]
struct Manager {
    monitors: Vec<Monitor>,
    sessions: HashMap<String, SharedSession>,
}
struct SessionData {
    app: String,
    owner: String,
    selected: bool,
    multiple: bool,
    cursor: u32,
    starting: bool,
    closed: bool,
    sharing: Vec<Sharing>,
    picker: Option<std::process::Child>,
}
impl SessionData {
    fn close(&mut self) {
        self.closed = true;
        if let Some(mut child) = self.picker.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.sharing.clear();
    }
}
impl Drop for SessionData {
    fn drop(&mut self) {
        self.close();
    }
}
fn denied() -> (u32, Dict) {
    (2, Dict::new())
}
fn ok() -> (u32, Dict) {
    (0, Dict::new())
}
fn uint(options: &Dict, name: &str, default: u32) -> u32 {
    options
        .get(name)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(default)
}
async fn trusted(conn: &zbus::Connection, header: &Header<'_>) -> bool {
    let Ok(proxy) = zbus::Proxy::new(
        conn,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await
    else {
        return false;
    };
    proxy
        .call::<_, _, String>("GetNameOwner", &("org.freedesktop.portal.Desktop",))
        .await
        .is_ok_and(|owner| header.sender().is_some_and(|s| s.as_str() == owner))
}
#[derive(Clone)]
struct Backend(Arc<Mutex<Manager>>);
struct SessionObject {
    data: SharedSession,
}
#[zbus::interface(name = "org.freedesktop.impl.portal.Session")]
impl SessionObject {
    fn close(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        if header
            .sender()
            .is_none_or(|s| s.as_str() != self.data.lock().unwrap().owner)
        {
            return Err(zbus::fdo::Error::AccessDenied(
                "Portal session owner required".into(),
            ));
        }
        self.data.lock().unwrap().close();
        Ok(())
    }
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}
struct Request {
    data: SharedSession,
}
#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl Request {
    fn close(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        let mut data = self.data.lock().unwrap();
        if header.sender().is_none_or(|s| s.as_str() != data.owner) {
            return Err(zbus::fdo::Error::AccessDenied(
                "Portal request owner required".into(),
            ));
        }
        data.close();
        Ok(())
    }
}
impl Backend {
    fn session(&self, path: &str, app: &str, header: &Header<'_>) -> Option<SharedSession> {
        let session = self.0.lock().unwrap().sessions.get(path)?.clone();
        let data = session.lock().unwrap();
        let valid = !data.closed
            && data.app == app
            && header.sender().is_some_and(|s| s.as_str() == data.owner);
        drop(data);
        valid.then_some(session)
    }
}
#[zbus::interface(name = "org.freedesktop.impl.portal.ScreenCast")]
impl Backend {
    #[zbus(out_args("response", "results"))]
    async fn create_session(
        &self,
        handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        options: Dict,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (u32, Dict) {
        let _ = (handle, options);
        if !trusted(conn, &header).await {
            return denied();
        }
        let path = session_handle.to_string();
        {
            let manager = self.0.lock().unwrap();
            if manager.sessions.contains_key(&path) || manager.sessions.len() >= 32 {
                return denied();
            }
        }
        let data = Arc::new(Mutex::new(SessionData {
            app: app_id,
            owner: header.sender().unwrap().to_string(),
            selected: false,
            multiple: false,
            cursor: 1,
            starting: false,
            closed: false,
            sharing: Vec::new(),
            picker: None,
        }));
        if conn
            .object_server()
            .at(session_handle, SessionObject { data: data.clone() })
            .await
            .is_err()
        {
            return denied();
        }
        self.0.lock().unwrap().sessions.insert(path, data);
        ok()
    }
    #[zbus(out_args("response", "results"))]
    fn select_sources(
        &self,
        handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        options: Dict,
        #[zbus(header)] header: Header<'_>,
    ) -> (u32, Dict) {
        let _ = handle;
        let Some(session) = self.session(session_handle.as_str(), &app_id, &header) else {
            return denied();
        };
        let types = uint(&options, "types", 1);
        let cursor = uint(&options, "cursor_mode", 1);
        let mut data = session.lock().unwrap();
        if data.selected || data.starting || types & 1 == 0 || !matches!(cursor, 1 | 2) {
            return denied();
        }
        data.multiple = options
            .get("multiple")
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(false);
        data.cursor = cursor;
        data.selected = true;
        ok()
    }
    #[zbus(out_args("response", "results"))]
    async fn start(
        &self,
        handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        options: Dict,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (u32, Dict) {
        let _ = (parent_window, options);
        let Some(session) = self.session(session_handle.as_str(), &app_id, &header) else {
            return denied();
        };
        let monitors = self.0.lock().unwrap().monitors.clone();
        let (multiple, cursor) = {
            let mut data = session.lock().unwrap();
            if !data.selected || data.starting || monitors.is_empty() {
                return denied();
            }
            data.starting = true;
            (data.multiple, data.cursor)
        };
        if conn
            .object_server()
            .at(
                handle.clone(),
                Request {
                    data: session.clone(),
                },
            )
            .await
            .is_err()
        {
            return denied();
        }
        let (send, receive) = async_channel::bounded(1);
        let backend = self.clone();
        let capture_session = session.clone();
        std::thread::spawn(move || {
            let session = capture_session;
            let result = (|| -> Result<Reply, String> {
                let picker = std::env::current_exe()
                    .map_err(|e| e.to_string())?
                    .with_file_name("dome-portal-picker");
                let mut child = std::process::Command::new(picker)
                    .arg(
                        serde_json::json!({"app_id":app_id,"multiple":multiple,"outputs":monitors})
                            .to_string(),
                    )
                    .env("QT_QPA_PLATFORM", "wayland")
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                let stdout = child.stdout.take().ok_or("No picker output")?;
                let (send, receive) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    use std::io::{BufRead, Read};
                    let mut line = String::new();
                    let result = std::io::BufReader::new(stdout.take(65536))
                        .read_line(&mut line)
                        .map(|_| line)
                        .map_err(|e| e.to_string());
                    let _ = send.send(result);
                });
                {
                    let mut data = session.lock().unwrap();
                    if data.closed {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok((1, Dict::new()));
                    }
                    data.picker = Some(child);
                }
                let deadline = Instant::now() + Duration::from_secs(120);
                let bytes = loop {
                    if let Ok(result) = receive.try_recv() {
                        break result?;
                    }
                    {
                        let mut data = session.lock().unwrap();
                        if data.closed {
                            return Ok((1, Dict::new()));
                        }
                        if let Some(_status) = data
                            .picker
                            .as_mut()
                            .unwrap()
                            .try_wait()
                            .map_err(|e| e.to_string())?
                        {
                            return Ok((1, Dict::new()));
                        }
                        if Instant::now() > deadline {
                            data.close();
                            return Ok((1, Dict::new()));
                        }
                    }
                    std::thread::sleep(Duration::from_millis(20));
                };
                if bytes.trim().is_empty() {
                    return Ok((1, Dict::new()));
                }
                let names: Vec<String> = serde_json::from_str(&bytes).map_err(|e| e.to_string())?;
                if names.is_empty() || (!multiple && names.len() != 1) {
                    return Err("Invalid monitor selection".into());
                }
                let mut sharing = Vec::new();
                let mut streams = Vec::new();
                for name in names {
                    if sharing.iter().any(|s: &Sharing| s.monitor.name == name) {
                        return Err("Duplicate monitor".into());
                    }
                    let monitor = monitors
                        .iter()
                        .find(|m| m.name == name)
                        .ok_or("Unknown monitor")?
                        .clone();
                    if !backend.0.lock().unwrap().monitors.contains(&monitor) {
                        return Err("Monitor was disconnected".into());
                    }
                    let stream = Stream::new(&monitor)?;
                    let properties = Dict::from([
                        (
                            "position".into(),
                            OwnedValue::try_from(Value::from((monitor.x, monitor.y))).unwrap(),
                        ),
                        (
                            "size".into(),
                            OwnedValue::try_from(Value::from((monitor.width, monitor.height)))
                                .unwrap(),
                        ),
                        ("source_type".into(), OwnedValue::from(1u32)),
                        (
                            "id".into(),
                            OwnedValue::from(Str::from(monitor.name.clone())),
                        ),
                    ]);
                    streams.push((stream.node(), properties));
                    sharing.push(Sharing {
                        monitor,
                        stream,
                        cursor,
                        last: Instant::now() - Duration::from_secs(1),
                    });
                }
                let mut data = session.lock().unwrap();
                if data.closed {
                    return Ok((1, Dict::new()));
                }
                data.sharing = sharing;
                Ok((
                    0,
                    Dict::from([(
                        "streams".into(),
                        OwnedValue::try_from(Value::from(streams)).map_err(|e| e.to_string())?,
                    )]),
                ))
            })();
            let _ = send.send_blocking(result);
        });
        let result = receive
            .recv()
            .await
            .unwrap_or_else(|_| Err("Capture worker exited".into()));
        let _ = conn.object_server().remove::<Request, _>(handle).await;
        match result {
            Ok(reply) => {
                if reply.0 != 0 {
                    session.lock().unwrap().close();
                }
                reply
            }
            Err(error) => {
                eprintln!("Dome ScreenCast: {error}");
                session.lock().unwrap().close();
                denied()
            }
        }
    }
    #[zbus(property)]
    fn available_source_types(&self) -> u32 {
        1
    }
    #[zbus(property)]
    fn available_cursor_modes(&self) -> u32 {
        3
    }
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        3
    }
}

pub struct Service {
    connection: zbus::blocking::Connection,
    stop: Arc<std::sync::atomic::AtomicBool>,
    watcher: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(watcher) = self.watcher.take() {
            watcher.thread().unpark();
            let _ = watcher.join();
        }
        if let Some(manager) = STATE.get() {
            for session in manager.lock().unwrap().sessions.values() {
                session.lock().unwrap().close();
            }
        }
    }
}
pub fn start() -> Result<Option<Service>, Box<dyn std::error::Error>> {
    if std::env::var("DOME_LOGIN_SESSION").as_deref() != Ok("1")
        && std::env::var("DOME_SCREENCAST").as_deref() != Ok("1")
    {
        return Ok(None);
    }
    let manager = Arc::new(Mutex::new(Manager::default()));
    let connection = zbus::blocking::connection::Builder::session()?
        .name(BUS)?
        .serve_at(PATH, Backend(manager.clone()))?
        .build()?;
    STATE
        .set(manager)
        .map_err(|_| "ScreenCast service already started")?;
    eprintln!("Dome ScreenCast backend ready");
    // A crashed/restarted frontend must not leave streams or consent windows alive.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    let bus = connection.clone();
    let manager = STATE.get().unwrap().clone();
    let watcher = std::thread::spawn(move || {
        while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
            let owner = zbus::blocking::Proxy::new(
                &bus,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
            )
            .and_then(|p| {
                p.call::<_, _, String>("GetNameOwner", &("org.freedesktop.portal.Desktop",))
            })
            .ok();
            let sessions = manager
                .lock()
                .unwrap()
                .sessions
                .values()
                .cloned()
                .collect::<Vec<_>>();
            for session in sessions {
                let mut data = session.lock().unwrap();
                if owner.as_deref() != Some(data.owner.as_str()) {
                    data.close();
                }
            }
            std::thread::park_timeout(Duration::from_secs(1));
        }
    });
    Ok(Some(Service {
        connection,
        stop,
        watcher: Some(watcher),
    }))
}
/// Called from the compositor loop, also on output hotplug/resize. Closing a
/// session destroys nodes before notifying the frontend; old geometry never leaks.
pub fn update(state: &crate::Dome, service: &Option<Service>) {
    let (Some(manager), Some(service)) = (STATE.get(), service.as_ref()) else {
        return;
    };
    let monitors: Vec<_> = state
        .space
        .outputs()
        .filter_map(|output| {
            let geometry = state.space.output_geometry(output)?;
            let size = output.current_mode()?.size;
            Some(Monitor {
                name: output.name(),
                width: size.w,
                height: size.h,
                x: geometry.loc.x,
                y: geometry.loc.y,
            })
        })
        .collect();
    let sessions = {
        let mut manager = manager.lock().unwrap();
        manager.monitors = monitors.clone();
        manager
            .sessions
            .iter()
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect::<Vec<_>>()
    };
    for (path, session) in sessions {
        let mut data = session.lock().unwrap();
        if !data.closed
            && !data.sharing.is_empty()
            && data
                .picker
                .as_mut()
                .is_some_and(|p| p.try_wait().ok().flatten().is_some())
        {
            data.close();
        }
        if !data.closed && data.sharing.iter().any(|s| !monitors.contains(&s.monitor)) {
            data.close();
        }
        if data.closed {
            drop(data);
            let _ = service.connection.emit_signal(
                None::<&str>,
                path.as_str(),
                "org.freedesktop.impl.portal.Session",
                "Closed",
                &(),
            );
            manager.lock().unwrap().sessions.remove(&path);
            let _ = service
                .connection
                .object_server()
                .remove::<SessionObject, _>(path);
        }
    }
}
/// Capture with a separate render target so cursor modes and GPU readback cannot
/// alter scanout or host-window presentation. Rate limited to 30 Hz per stream.
pub fn render<E: RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    output: &Output,
    origin: Point<i32, Logical>,
    elements: &[E],
    cursor: u32,
    background: [f32; 4],
) {
    let Some(manager) = STATE.get() else { return };
    let sessions = manager
        .lock()
        .unwrap()
        .sessions
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for session in sessions {
        // Start/Close use short locks; PipeWire creation and consent never hold this lock.
        let Ok(mut data) = session.try_lock() else {
            continue;
        };
        for share in data
            .sharing
            .iter_mut()
            .filter(|s| s.monitor.name == output.name() && s.cursor == cursor)
        {
            if share.last.elapsed() < Duration::from_millis(33) {
                continue;
            }
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let size: smithay::utils::Size<i32, smithay::utils::Physical> =
                    (share.monitor.width, share.monitor.height).into();
                let mut texture: smithay::backend::renderer::gles::GlesTexture = renderer
                    .create_buffer(
                        smithay::backend::allocator::Fourcc::Abgr8888,
                        (size.w, size.h).into(),
                    )?;
                let mut target: GlesTarget<'_> = renderer.bind(&mut texture)?;
                let relocated: Vec<_> = elements
                    .iter()
                    .map(|e| {
                        RelocateRenderElement::from_element(
                            e,
                            (-origin.x, -origin.y),
                            Relocate::Relative,
                        )
                    })
                    .collect();
                let mut damage =
                    OutputDamageTracker::new(size, 1., smithay::utils::Transform::Flipped180);
                damage
                    .render_output(renderer, &mut target, 0, &relocated, background)?
                    .sync
                    .wait()?;
                let mapping = renderer.copy_framebuffer(
                    &target,
                    Rectangle::from_size((size.w, size.h).into()),
                    smithay::backend::allocator::Fourcc::Abgr8888,
                )?;
                let flipped = mapping.flipped();
                let bytes = renderer.map_texture(&mapping)?;
                if unsafe {
                    dome_cast_frame(
                        share.stream.0,
                        bytes.as_ptr(),
                        bytes.len(),
                        i32::from(flipped),
                    )
                } < 0
                {
                    return Err("PipeWire stream disconnected".into());
                }
                Ok(())
            })();
            share.last = Instant::now();
            if let Err(error) = result {
                eprintln!("Dome ScreenCast frame: {error}");
                data.close();
                break;
            }
        }
    }
}
