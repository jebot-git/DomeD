//! Session integration stays off the rendering thread. The nested shell never logs out its host.
use dome::appearance::SessionAction;
use std::sync::{mpsc, Arc, Mutex};
use zbus::blocking::{Connection, Proxy};
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub lock: bool,
    pub switch_user: bool,
    pub suspend: bool,
    pub restart: bool,
    pub shutdown: bool,
    pub error: String,
}
pub struct Session {
    state: Arc<Mutex<Capabilities>>,
    send: mpsc::SyncSender<SessionAction>,
}
fn allowed(p: &Proxy<'_>, method: &str) -> bool {
    p.call::<_, _, String>(method, &())
        .is_ok_and(|v| v == "yes" || v == "challenge")
}
impl Session {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(Capabilities::default()));
        let shared = state.clone();
        let (send, recv) = mpsc::sync_channel(4);
        std::thread::spawn(move || {
            let bus = Connection::session().ok();
            let system = Connection::system().ok();
            let locker = bus.as_ref().and_then(|b| {
                Proxy::new(
                    b,
                    "org.freedesktop.ScreenSaver",
                    "/ScreenSaver",
                    "org.freedesktop.ScreenSaver",
                )
                .ok()
            });
            let login = system.as_ref().and_then(|b| {
                Proxy::new(
                    b,
                    "org.freedesktop.login1",
                    "/org/freedesktop/login1",
                    "org.freedesktop.login1.Manager",
                )
                .ok()
            });
            let seat_path = std::env::var("XDG_SEAT_PATH").ok();
            let greeter = system.as_ref().zip(seat_path.as_ref()).and_then(|(b, p)| {
                Proxy::new(
                    b,
                    "org.freedesktop.DisplayManager",
                    p.as_str(),
                    "org.freedesktop.DisplayManager.Seat",
                )
                .ok()
            });
            // Introspect to avoid offering a lock command with no screen locker.
            let lock = bus
                .as_ref()
                .and_then(|b| {
                    Proxy::new(
                        b,
                        "org.freedesktop.ScreenSaver",
                        "/ScreenSaver",
                        "org.freedesktop.DBus.Introspectable",
                    )
                    .ok()
                })
                .is_some_and(|p| p.call::<_, _, String>("Introspect", &()).is_ok());
            *shared.lock().unwrap() = Capabilities {
                lock,
                switch_user: greeter
                    .as_ref()
                    .is_some_and(|p| p.get_property::<bool>("CanSwitch").unwrap_or(false)),
                suspend: login.as_ref().is_some_and(|p| allowed(p, "CanSuspend")),
                restart: login.as_ref().is_some_and(|p| allowed(p, "CanReboot")),
                shutdown: login.as_ref().is_some_and(|p| allowed(p, "CanPowerOff")),
                error: String::new(),
            };
            while let Ok(action) = recv.recv() {
                let result: Result<(), String> = match action {
                    SessionAction::Lock => locker
                        .as_ref()
                        .ok_or("No screen locker".to_string())
                        .and_then(|p| p.call("Lock", &()).map_err(|e| e.to_string())),
                    SessionAction::SwitchUser => greeter
                        .as_ref()
                        .ok_or("No display manager seat".to_string())
                        .and_then(|p| p.call("SwitchToGreeter", &()).map_err(|e| e.to_string())),
                    SessionAction::Suspend | SessionAction::Restart | SessionAction::Shutdown => {
                        login
                            .as_ref()
                            .ok_or("No logind service".to_string())
                            .and_then(|p| {
                                p.call(
                                    match action {
                                        SessionAction::Suspend => "Suspend",
                                        SessionAction::Restart => "Reboot",
                                        _ => "PowerOff",
                                    },
                                    &(true,),
                                )
                                .map_err(|e| e.to_string())
                            })
                    }
                    SessionAction::Logout => Ok(()),
                };
                shared.lock().unwrap().error = result.err().unwrap_or_default();
            }
        });
        Self { state, send }
    }
    pub fn state(&self) -> Capabilities {
        self.state.lock().unwrap().clone()
    }
    pub fn request(&self, action: SessionAction) {
        let _ = self.send.try_send(action);
    }
}
impl crate::Dome {
    pub fn session_command(&mut self, action: SessionAction) {
        match action {
            SessionAction::Lock | SessionAction::SwitchUser => {
                self.xr_bridge.lock().unwrap().exit_requested = true;
                self.session.request(action);
            }
            _ => self.session_confirm = Some(action),
        }
    }
    pub fn confirm_session(&mut self) {
        if let Some(action) = self.session_confirm.take() {
            if matches!(action, SessionAction::Logout | SessionAction::Restart | SessionAction::Shutdown) {
                self.save_session_on_exit();
            }
            if action == SessionAction::Logout {
                self.xr_bridge.lock().unwrap().exit_requested = true;
                self.loop_signal.stop();
            } else {
                self.xr_bridge.lock().unwrap().exit_requested = true;
                self.session.request(action);
            }
        }
    }
    pub fn start_xr(&mut self) {
        if self.xr_bridge.lock().unwrap().visible() {
            return;
        }
        if self
            .xr_child
            .as_mut()
            .is_some_and(|child| child.try_wait().ok().flatten().is_none())
        {
            return;
        }
        let binary = std::env::current_exe().unwrap().with_file_name("dome-xr");
        self.xr_bridge.lock().unwrap().exit_requested = false;
        match std::process::Command::new(binary)
            .args([
                "--server",
                &format!("http://127.0.0.1:{}", self.control_port),
            ])
            .spawn()
        {
            Ok(child) => self.xr_child = Some(child),
            Err(e) => self.shell_error = format!("Start XR: {e}"),
        }
    }
    pub fn appearance_click(&mut self, x: f64, y: f64, width: f64, height: f64) {
        let (left, top) = ((width - 600.) / 2., (height - 500.) / 2.);
        if x < left || x > left + 600. || y < top || y > top + 500. {
            self.appearance_open = false;
            return;
        }
        if y >= top + 452. {
            if x >= left+24. && x < left+300. { self.default_apps_open(); return; }
            self.appearance_open = false;
            return;
        }
        if x < left + 300. || y < top + 60. {
            return;
        }
        match ((y - top - 60.) / 48.) as usize {
            0 => self.appearance.light = !self.appearance.light,
            1 => {
                self.appearance.accent = if self.appearance.accent == "#6babff" {
                    "#61cfa5"
                } else if self.appearance.accent == "#61cfa5" {
                    "#e7ac69"
                } else {
                    "#6babff"
                }
                .into();
            }
            2 => {
                self.appearance.background = if self.appearance.background[0] < 0.1 {
                    [0.16, 0.18, 0.22]
                } else if self.appearance.background[0] < 0.3 {
                    [0.52, 0.56, 0.62]
                } else {
                    [0.025, 0.045, 0.06]
                }
            }
            3 => {
                self.appearance.opacity = if self.appearance.opacity == 255 {
                    208
                } else if self.appearance.opacity < 240 {
                    248
                } else {
                    255
                }
            }
            4 => {
                let fonts = crate::qt::bitmap_fonts();
                if fonts.is_empty() { return; }
                let next = fonts.iter().position(|f| f.id == self.appearance.font)
                    .map_or(0, |index| (index + 1) % fonts.len());
                self.appearance.font = fonts[next].id.clone();
                if !fonts[next].sizes.contains(&self.appearance.font_size) {
                    self.appearance.font_size = fonts[next].sizes[0];
                }
            }
            5 => {
                let fonts = crate::qt::bitmap_fonts();
                let Some(font) = fonts.iter().find(|f| f.id == self.appearance.font) else { return; };
                let next = font.sizes.iter().position(|size| *size == self.appearance.font_size)
                    .map_or(0, |index| (index + 1) % font.sizes.len());
                self.appearance.font_size = font.sizes[next];
            }
            6 => {
                self.appearance.hand_resume_ms = if self.appearance.hand_resume_ms >= 1500 {
                    500
                } else {
                    self.appearance.hand_resume_ms + 500
                }
            }
            7 => self.appearance = Default::default(),
            _ => return,
        }
        self.xr_bridge.lock().unwrap().hand_resume_ms = self.appearance.hand_resume_ms;
        if let Err(e) = self.appearance.save() {
            self.shell_error = e;
        }
        if let Some(toolkit) = &self.toolkit_appearance { toolkit.update(&self.appearance); }
    }
}
