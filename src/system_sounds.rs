//! Short, themeable session feedback. Audio work never runs on the compositor thread.
use serde::{Deserialize, Serialize};
use std::{
    ffi::{c_char, c_int, c_void, CString},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self { enabled: true }
    }
}
impl Settings {
    pub fn path() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            })
            .join("dome/sounds.json")
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let parent = path.parent().ok_or("Sound settings have no directory")?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| e.to_string())?;
        let temporary = path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(self)?)?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)?;
            std::fs::File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|e| e.to_string())
    }
}

/// Theme identifiers only: notification-provided filesystem paths are not used.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
#[derive(Clone, Copy)]
pub enum Event {
    Notification,
    Critical,
    Lock,
    Unlock,
    Error,
    Bell,
}
impl Event {
    pub fn name(self) -> &'static str {
        match self {
            Self::Notification => "message-new-instant",
            Self::Critical => "dialog-warning",
            Self::Lock => "desktop-screen-lock",
            Self::Unlock => "desktop-screen-unlock",
            Self::Error => "dialog-error",
            Self::Bell => "bell-window-system",
        }
    }
}
struct Gate {
    enabled: bool,
    generation: u64,
    last: Option<Instant>,
}
impl Gate {
    fn configure(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.generation = self.generation.wrapping_add(1);
            self.last = None;
        }
    }
    fn request(&mut self, name: &str, now: Instant) -> Option<u64> {
        if !self.enabled
            || !valid_name(name)
            || self.last.is_some_and(|last| {
                now.saturating_duration_since(last) < Duration::from_millis(150)
            })
        {
            return None;
        }
        self.last = Some(now);
        Some(self.generation)
    }
}
enum Request {
    Play {
        name: String,
        generation: u64,
        at: Instant,
    },
    Reconcile,
}
#[derive(Clone)]
pub struct Player {
    gate: Arc<Mutex<Gate>>,
    send: SyncSender<Request>,
}
impl Player {
    pub fn new(enabled: bool) -> Self {
        let gate = Arc::new(Mutex::new(Gate {
            enabled,
            generation: 0,
            last: None,
        }));
        let (send, receive) = mpsc::sync_channel(8);
        let shared = gate.clone();
        std::thread::spawn(move || worker(receive, shared));
        Self { gate, send }
    }
    pub fn enabled(&self) -> bool {
        self.gate.lock().unwrap().enabled
    }
    pub fn set_enabled(&self, enabled: bool) {
        self.gate.lock().unwrap().configure(enabled);
        let _ = self.send.try_send(Request::Reconcile);
    }
    pub fn play(&self, event: Event) {
        self.play_name(event.name());
    }
    pub fn play_name(&self, name: &str) {
        let at = Instant::now();
        let generation = self.gate.lock().unwrap().request(name, at);
        if let Some(generation) = generation {
            let _ = self.send.try_send(Request::Play {
                name: name.to_owned(),
                generation,
                at,
            });
        }
    }
}
fn worker(receive: Receiver<Request>, gate: Arc<Mutex<Gate>>) {
    let mut backend = match Canberra::new() {
        Ok(backend) => Some(backend),
        Err(error) => {
            eprintln!("Dome system sounds unavailable: {error}");
            None
        }
    };
    while let Ok(request) = receive.recv() {
        let state = gate.lock().unwrap();
        let enabled = state.enabled;
        let generation = state.generation;
        drop(state);
        let Some(backend) = backend.as_mut() else {
            continue;
        };
        if !enabled {
            backend.stop();
            continue;
        }
        if let Request::Play {
            name,
            generation: requested,
            at,
        } = request
        {
            if requested == generation && at.elapsed() < Duration::from_secs(1) {
                backend.play(&name);
            }
        }
    }
}

type Create = unsafe extern "C" fn(*mut *mut c_void) -> c_int;
type Destroy = unsafe extern "C" fn(*mut c_void) -> c_int;
type Set = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type Props = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type Finish = unsafe extern "C" fn(*mut c_void, u32, c_int, *mut c_void);
type Play =
    unsafe extern "C" fn(*mut c_void, u32, *mut c_void, Option<Finish>, *mut c_void) -> c_int;
type Cancel = unsafe extern "C" fn(*mut c_void, u32) -> c_int;
struct Canberra {
    _library: libloading::Library,
    context: *mut c_void,
    destroy: Destroy,
    create_props: Create,
    destroy_props: Destroy,
    set: Set,
    play: Play,
    cancel: Cancel,
}
impl Canberra {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        unsafe {
            let library = libloading::Library::new("libcanberra.so.0")?;
            let create: Create = *library.get(b"ca_context_create\0")?;
            let destroy = *library.get(b"ca_context_destroy\0")?;
            let create_props = *library.get(b"ca_proplist_create\0")?;
            let destroy_props = *library.get(b"ca_proplist_destroy\0")?;
            let set = *library.get(b"ca_proplist_sets\0")?;
            let play = *library.get(b"ca_context_play_full\0")?;
            let cancel = *library.get(b"ca_context_cancel\0")?;
            let change: Props = *library.get(b"ca_context_change_props_full\0")?;
            let mut context = std::ptr::null_mut();
            if create(&mut context) < 0 || context.is_null() {
                return Err("Cannot create event sound context".into());
            }
            let backend = Self {
                _library: library,
                context,
                destroy,
                create_props,
                destroy_props,
                set,
                play,
                cancel,
            };
            if let Some(props) = backend.properties(&[
                ("application.name", "DomeD"),
                ("application.id", "org.domed.Session"),
                ("media.role", "event"),
            ]) {
                change(context, props);
                (backend.destroy_props)(props);
            }
            Ok(backend)
        }
    }
    fn properties(&self, values: &[(&str, &str)]) -> Option<*mut c_void> {
        unsafe {
            let mut props = std::ptr::null_mut();
            if (self.create_props)(&mut props) < 0 || props.is_null() {
                return None;
            }
            for (key, value) in values {
                let (Ok(key), Ok(value)) = (CString::new(*key), CString::new(*value)) else {
                    (self.destroy_props)(props);
                    return None;
                };
                if (self.set)(props, key.as_ptr(), value.as_ptr()) < 0 {
                    (self.destroy_props)(props);
                    return None;
                }
            }
            Some(props)
        }
    }
    fn play(&mut self, name: &str) {
        self.stop();
        self.play_once(name);
    }
    fn play_once(&self, name: &str) {
        if let Some(props) = self.properties(&[
            ("event.id", name),
            ("event.description", "DomeD session feedback"),
            ("canberra.cache-control", "volatile"),
        ]) {
            unsafe {
                let result = (self.play)(self.context, 1, props, None, std::ptr::null_mut());
                (self.destroy_props)(props);
                // Minimal themes may omit the optional screen-lock sounds.
                if result < 0 && name.starts_with("desktop-screen-") {
                    self.play_once("dialog-information");
                }
            }
        }
    }
    fn stop(&self) {
        unsafe {
            (self.cancel)(self.context, 1);
        }
    }
}
impl Drop for Canberra {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)(self.context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_profiles_default_to_sound_enabled() {
        assert_eq!(
            serde_json::from_str::<Settings>("{}").unwrap(),
            Settings::default()
        );
        assert!(
            !serde_json::from_str::<Settings>("{\"enabled\":false}")
                .unwrap()
                .enabled
        );
        assert!(serde_json::from_str::<Settings>("{\"enabled\":\"no\"}").is_err());
    }
    #[test]
    fn requests_are_bounded_theme_names() {
        for name in [
            "message-new-instant",
            "dialog-warning",
            "bell-window-system",
            "custom_1",
        ] {
            assert!(valid_name(name));
        }
        for name in [
            "",
            "../a",
            "/tmp/a.wav",
            "file:audio",
            "a\0b",
            &"x".repeat(65),
        ] {
            assert!(!valid_name(name));
        }
    }
    #[test]
    fn mute_and_reenable_invalidate_pending_audio() {
        let now = Instant::now();
        let mut gate = Gate {
            enabled: true,
            generation: 0,
            last: None,
        };
        let original = gate.request("dialog-error", now).unwrap();
        gate.configure(false);
        assert!(gate
            .request("dialog-error", now + Duration::from_secs(1))
            .is_none());
        gate.configure(true);
        assert_ne!(
            gate.request("dialog-error", now + Duration::from_secs(2))
                .unwrap(),
            original
        );
    }
    #[test]
    fn burst_notifications_are_coalesced_without_backlog() {
        let now = Instant::now();
        let mut gate = Gate {
            enabled: true,
            generation: 0,
            last: None,
        };
        assert!(gate.request("message-new-instant", now).is_some());
        assert!(gate
            .request("message-new-instant", now + Duration::from_millis(149))
            .is_none());
        assert!(gate
            .request("dialog-warning", now + Duration::from_millis(150))
            .is_some());
    }
    #[test]
    fn private_atomic_settings_survive_restart_and_bad_edits_are_retained() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "dome-sound-settings-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("sounds.json");
        let _ = std::fs::remove_dir_all(&root);
        assert!(Settings::load(&path).unwrap().enabled);
        Settings { enabled: false }.save(&path).unwrap();
        assert!(!Settings::load(&path).unwrap().enabled);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::write(&path, "bad edit").unwrap();
        assert!(Settings::load(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "bad edit");
        std::fs::remove_dir_all(root).unwrap();
    }
}
