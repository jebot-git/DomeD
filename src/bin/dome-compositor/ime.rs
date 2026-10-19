//! Fcitx5 runs on an explicitly trusted Wayland connection, with Dome-only settings.
use smithay::reexports::rustix::io::{fcntl_setfd, FdFlags};
use std::{
    os::{
        fd::AsRawFd,
        unix::{net::UnixStream, process::CommandExt},
    },
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use zbus::blocking::{Connection, Proxy};
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub method: String,
    pub error: String,
    pub running: bool,
}
enum Request {
    Start(UnixStream),
    Toggle,
}
pub struct Ime {
    send: mpsc::Sender<Request>,
    state: Arc<Mutex<Snapshot>>,
}
impl Ime {
    pub fn new() -> Self {
        let (send, receive) = mpsc::channel();
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let shared = state.clone();
        std::thread::spawn(move || {
            let mut child: Option<std::process::Child> = None;
            loop {
                match receive.recv_timeout(Duration::from_millis(200)) {
                    Ok(Request::Start(stream)) => {
                        let result = (|| -> Result<std::process::Child, String> {
                            let connection = Connection::session().map_err(|e| e.to_string())?;
                            let bus = Proxy::new(
                                &connection,
                                "org.freedesktop.DBus",
                                "/org/freedesktop/DBus",
                                "org.freedesktop.DBus",
                            )
                            .map_err(|e| e.to_string())?;
                            if bus
                                .call::<_, _, bool>("NameHasOwner", &("org.fcitx.Fcitx5",))
                                .unwrap_or(true)
                            {
                                return Err(
                                    "Fcitx5 already runs on this bus; DomeD will not replace it"
                                        .into(),
                                );
                            }
                            let config = std::env::var_os("XDG_CONFIG_HOME")
                                .map(std::path::PathBuf::from)
                                .unwrap_or_else(|| {
                                    std::path::PathBuf::from(
                                        std::env::var_os("HOME").unwrap_or_default(),
                                    )
                                    .join(".config")
                                })
                                .join("dome/fcitx5");
                            std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
                            use std::io::Write;
                            if let Ok(mut profile) = std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(config.join("profile"))
                            {
                                profile.write_all(b"[Groups/0]\nName=Default\nDefault Layout=us\nDefaultIM=anthy\n\n[Groups/0/Items/0]\nName=keyboard-us\n\n[Groups/0/Items/1]\nName=anthy\n\n[GroupOrder]\n0=Default\n").map_err(|e|e.to_string())?;
                            }
                            // Dome owns the switching binding. Keep Fcitx's independent Ctrl+Space
                            // trigger off so applications receive that shortcut normally.
                            if let Ok(mut file) = std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(config.join("config"))
                            {
                                file.write_all(b"[Hotkey/TriggerKeys]\n0=\n[Hotkey/EnumerateForwardKeys]\n0=\n[Hotkey/EnumerateBackwardKeys]\n0=\n").map_err(|e|e.to_string())?;
                            }
                            let mut command = std::process::Command::new("/usr/bin/fcitx5");
                            command
                                .args([
                                    "-D",
                                    "--disable=xcb,ibusfrontend,kimpanel",
                                    "--ui=classicui",
                                ])
                                .env("WAYLAND_SOCKET", stream.as_raw_fd().to_string())
                                .env("FCITX_CONFIG_HOME", config)
                                .env_remove("DISPLAY")
                                .env_remove("IBUS_ADDRESS");
                            unsafe {
                                command.pre_exec(move || {
                                    fcntl_setfd(&stream, FdFlags::empty())
                                        .map_err(std::io::Error::from)
                                });
                            }
                            command.spawn().map_err(|e| e.to_string())
                        })();
                        match result {
                            Ok(p) => child = Some(p),
                            Err(e) => shared.lock().unwrap().error = e,
                        }
                    }
                    Ok(Request::Toggle) => {
                        if child.is_some() {
                            if let Ok(c) = Connection::session() {
                                if let Ok(p) = Proxy::new(
                                    &c,
                                    "org.fcitx.Fcitx5",
                                    "/controller",
                                    "org.fcitx.Fcitx.Controller1",
                                ) {
                                    let current = p
                                        .call::<_, _, String>("CurrentInputMethod", &())
                                        .unwrap_or_default();
                                    let next = if current == "anthy" {
                                        "keyboard-us"
                                    } else {
                                        "anthy"
                                    };
                                    if let Err(e) = p.call::<_, _, ()>("SetCurrentIM", &(next,)) {
                                        shared.lock().unwrap().error = e.to_string();
                                    }
                                }
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(mut p) = child {
                            let _ = p.kill();
                            let _ = p.wait();
                        }
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                }
                if let Some(p) = child.as_mut() {
                    match p.try_wait() {
                        Ok(Some(status)) => {
                            shared.lock().unwrap().error = format!("Fcitx5 exited: {status}");
                            child = None;
                        }
                        _ => (),
                    }
                    if child.is_some() {
                        if let Ok(c) = Connection::session() {
                            let owned = Proxy::new(
                                &c,
                                "org.freedesktop.DBus",
                                "/org/freedesktop/DBus",
                                "org.freedesktop.DBus",
                            )
                            .ok()
                            .and_then(|p| {
                                p.call::<_, _, bool>("NameHasOwner", &("org.fcitx.Fcitx5",))
                                    .ok()
                            })
                            .unwrap_or(false);
                            if !owned {
                                continue;
                            }

                            if let Ok(p) = Proxy::new(
                                &c,
                                "org.fcitx.Fcitx5",
                                "/controller",
                                "org.fcitx.Fcitx.Controller1",
                            ) {
                                if let Ok(method) =
                                    p.call::<_, _, String>("CurrentInputMethod", &())
                                {
                                    let mut s = shared.lock().unwrap();
                                    s.method = method;
                                    s.running = true;
                                }
                            }
                        }
                    }
                } else {
                    shared.lock().unwrap().running = false;
                }
            }
        });
        Self { send, state }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
    pub fn toggle(&self) {
        let _ = self.send.send(Request::Toggle);
    }
    pub fn start(&self, dh: &smithay::reexports::wayland_server::DisplayHandle) {
        if std::env::var("DOME_START_IME").as_deref() != Ok("1")
            && (std::env::var("DOME_LOGIN_SESSION").as_deref() != Ok("1")
                || std::env::var_os("DOME_DRM_TEST_DEVICE").is_some()
                || std::env::var_os("DOME_DRM_TEST_DEVICES").is_some())
        {
            return;
        }
        if std::env::var("DOME_START_IME").as_deref() == Ok("0") {
            return;
        }
        match UnixStream::pair() {
            Ok((server, client)) => {
                let mut handle = dh.clone();
                if handle
                    .insert_client(
                        server,
                        Arc::new(crate::state::ClientState {
                            trusted_ime: true,
                            ..Default::default()
                        }),
                    )
                    .is_ok()
                {
                    let _ = self.send.send(Request::Start(client));
                }
            }
            Err(e) => self.state.lock().unwrap().error = e.to_string(),
        }
    }
}
use crate::Dome;
use smithay::{
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Rectangle},
    wayland::input_method::{InputMethodHandler, PopupSurface},
};
impl InputMethodHandler for Dome {
    fn new_popup(&mut self, popup: PopupSurface) {
        self.ime_popups
            .retain(|p| p.wl_surface() != popup.wl_surface());
        self.ime_popups.push(popup);
    }
    fn dismiss_popup(&mut self, popup: PopupSurface) {
        self.ime_popups
            .retain(|p| p.wl_surface() != popup.wl_surface());
    }
    fn popup_repositioned(&mut self, popup: PopupSurface) {
        self.new_popup(popup);
    }
    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        self.registry
            .iter()
            .find(|(_, w)| w.toplevel().unwrap().wl_surface() == parent)
            .map(|(_, w)| w.geometry())
            .unwrap_or_default()
    }
}
smithay::delegate_text_input_manager!(Dome);
smithay::delegate_input_method_manager!(Dome);
smithay::delegate_virtual_keyboard_manager!(Dome);
