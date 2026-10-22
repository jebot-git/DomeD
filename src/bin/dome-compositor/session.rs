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
            _ => {
                self.shell_selection = 0;
                self.session_confirm = Some(action);
            }
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
        let (left, top) = ((width - 600.) / 2., (height - 560.) / 2.);
        if x < left || x > left + 600. || y < top || y > top + 560. {
            self.appearance_open = false;
            return;
        }
        if y >= top + 512. {
            self.appearance_activate(if x < left+300. { 8 } else { 9 }, 1);
            return;
        }
        if x < left + 300. || y < top + 60. {
            return;
        }
        let row = ((y - top - 60.) / 48.) as usize;
        if row < 8 {
            self.appearance_activate(row, 1);
        }
    }
    pub fn appearance_activate(&mut self, row: usize, direction: i32) {
        self.shell_selection = row;
        crate::qt::normalize_appearance(&mut self.appearance);
        match row {
            0 => self.appearance.light = !self.appearance.light,
            1 => {
                self.appearance.accent = crate::menu_navigation::cycle(
                    &["#6babff".to_owned(), "#61cfa5".to_owned(), "#e7ac69".to_owned()],
                    &self.appearance.accent, direction,
                ).unwrap();
            }
            2 => {
                self.appearance.opacity = crate::menu_navigation::cycle(
                    &dome::appearance::CHROME_OPACITIES, &self.appearance.opacity, direction,
                ).unwrap();
            }
            3 | 4 => {
                let fonts = crate::qt::bitmap_fonts();
                let current = self.appearance.font.split_once(" / ").map(|v| v.0).unwrap_or("");
                let choices: Vec<_> = if row == 3 {
                    let mut families = std::collections::BTreeMap::new();
                    for font in &fonts {
                        let family = font.id.split_once(" / ").map(|v| v.0).unwrap_or(&font.id);
                        let entry = families.entry(family).or_insert(font);
                        if font.id.ends_with(" / Regular") || font.id.ends_with(" / Sans-Serif") { *entry = font; }
                    }
                    families.into_values().collect()
                } else {
                    fonts.iter().filter(|font| font.id.split_once(" / ").map(|v| v.0) == Some(current)).collect()
                };
                if choices.is_empty() { return; }
                let index = choices.iter().position(|font| if row == 3 {
                    font.id.split_once(" / ").map(|v| v.0) == Some(current)
                } else { font.id == self.appearance.font }).unwrap_or(0);
                let next = (index as i32 + direction).rem_euclid(choices.len() as i32) as usize;
                self.appearance.font = choices[next].id.clone();
                self.appearance.font_size = *choices[next].sizes.iter()
                    .min_by_key(|size| size.abs_diff(self.appearance.font_size)).unwrap();
            }
            5 => {
                let fonts = crate::qt::bitmap_fonts();
                let Some(font) = fonts.iter().find(|f| f.id == self.appearance.font) else { return; };
                if let Some(size) = crate::menu_navigation::cycle(
                    &font.sizes, &self.appearance.font_size, direction,
                ) { self.appearance.font_size = size; }
            }
            6 => self.appearance.font_scale = if self.appearance.font_scale == 1 { 2 } else { 1 },
            7 => self.appearance = Default::default(),
            8 => { self.default_apps_open(); return; }
            9 => { self.appearance_open = false; return; }
            _ => return,
        }
        if let Err(e) = crate::qt::resolved_font(&self.appearance) {
            self.shell_error = e;
            return;
        }
        if let Err(e) = self.appearance.save() {
            self.shell_error = e;
        }
        self.recover_output_layout();
        if let Some(toolkit) = &self.toolkit_appearance { toolkit.update(&self.appearance); }
    }
}
