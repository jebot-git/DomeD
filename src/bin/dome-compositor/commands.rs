use crate::Dome;
use dome::{shortcuts::Command, Action};
use smithay::{
    backend::input::{KeyState, Keycode},
    input::keyboard::FilterResult,
    utils::SERIAL_COUNTER,
};
impl Dome {
    pub fn menu_index(&self, x: f64) -> Option<usize> {
        [46., 230., 280., 332., 390., 470., 528.]
            .iter()
            .position(|end| x < *end)
    }
    pub fn menu_items(&self, m: usize) -> Vec<(String, &'static str, Command)> {
        use Command::*;
        if m == 1 {
            let scene = self.shell_scene();
            let w = scene.focused.and_then(|id| scene.window(id));
            let app = w.and_then(|w| self.apps.iter().find(|a| a.matches(&w.app_id)));
            let name = app
                .map(|a| a.name.as_str())
                .or_else(|| w.map(|w| w.app_id.as_str()))
                .unwrap_or("Desktop");
            let mut items = vec![(format!("About {name}"), "", AppInfo)];
            if let Some(app) = app {
                if !app.actions.is_empty() {
                    items.push((String::new(), "", Separator));
                }
                for (i, a) in app.actions.iter().enumerate() {
                    items.push((
                        a.name.clone(),
                        "",
                        DesktopAction {
                            app: app.id,
                            action: i,
                        },
                    ));
                }
            }
            items.extend([
                (String::new(), "", Separator),
                (format!("Hide {name}"), "⌘H", HideApp),
                ("Hide Others".into(), "⌥⌘H", HideOthers),
                ("Show All".into(), "", ShowAll),
                (String::new(), "", Separator),
                (format!("Quit {name}"), "⌘Q", QuitApp),
            ]);
            return items;
        }
        let items: Vec<(&str, &str, Command)> = match m {
            0 => vec![
                ("About Dome", "", About),
                ("", "", Separator),
                ("Launch Application…", "⇧⌘Space", Launch),
                ("Open Terminal", "⌘T", Terminal),
                ("Virtual Desktops…", "", Desktops),
                ("Appearance…", "", Appearance),
                ("", "", Separator),
                ("Start XR", "", StartXr),
                ("Stop XR", "", StopXr),
                ("", "", Separator),
                (
                    "Lock Screen",
                    "",
                    Session(dome::appearance::SessionAction::Lock),
                ),
                (
                    "Switch User…",
                    "",
                    Session(dome::appearance::SessionAction::SwitchUser),
                ),
                (
                    "Sleep…",
                    "",
                    Session(dome::appearance::SessionAction::Suspend),
                ),
                (
                    "Log Out of Dome…",
                    "",
                    Session(dome::appearance::SessionAction::Logout),
                ),
                (
                    "Restart…",
                    "",
                    Session(dome::appearance::SessionAction::Restart),
                ),
                (
                    "Shut Down…",
                    "",
                    Session(dome::appearance::SessionAction::Shutdown),
                ),
                ("Keyboard Layouts…", "", KeyboardSettings),
            ],
            1 => vec![("Close Application Windows", "⌘Q", QuitApp)],
            2 => vec![("Close Window", "⌘W", Close)],
            3 => vec![
                ("Copy", "⌘C", Copy),
                ("Paste", "⌘V", Paste),
                ("Select All", "⌘A", SelectAll),
            ],
            4 => vec![
                ("Increase XR Window Size", "⌘+", Scale(1.1)),
                ("Decrease XR Window Size", "⌘−", Scale(1. / 1.1)),
                ("Return to Dome Layout", "⇧⌘R", ResetSpatial),
                ("Toggle Dome / Direct View", "⌘P", Projection),
                ("", "", Separator),
                ("Show / Hide XR Bars", "⇧⌘B", ToggleXrBars),
                ("Show / Hide Upper XR Bar", "⌥⌘T", ToggleXrTop),
                ("Show / Hide XR Taskbar", "⌥⌘B", ToggleXrBottom),
                ("Show / Hide XR Desktop", "⇧⌘D", ToggleXrDesktop),
                ("Displays & XR Configuration…", "⌘,", XrSettings),
                ("Anchor Dome at Headset", "⌃⌘R", AnchorHere),
            ],
            5 => vec![
                ("Minimize", "⌘M", Minimize),
                ("Zoom", "⌘Return", Maximize),
                ("Next Window", "Alt Tab", Cycle(false)),
                ("Previous Window", "⇧Alt Tab", Cycle(true)),
                ("Virtual Desktops…", "", Desktops),
                ("New Desktop", "⇧⌘N", NewWorkspace),
            ],
            _ => vec![("About Dome", "", About)],
        };
        items
            .into_iter()
            .map(|(a, b, c)| (a.into(), b, c))
            .collect()
    }
    pub fn command_enabled(&self, c: Command) -> bool {
        use Command::*;
        let s = self.shell_scene();
        match c {
            Session(action) => {
                let cap = self.session.state();
                match action {
                    dome::appearance::SessionAction::Lock => cap.lock,
                    dome::appearance::SessionAction::SwitchUser => cap.switch_user,
                    dome::appearance::SessionAction::Suspend => cap.suspend,
                    dome::appearance::SessionAction::Logout => true,
                    dome::appearance::SessionAction::Restart => cap.restart,
                    dome::appearance::SessionAction::Shutdown => cap.shutdown,
                }
            }
            StartXr => !self.xr_bridge.lock().unwrap().visible(),
            StopXr => self.xr_bridge.lock().unwrap().visible(),
            Separator => false,
            Close | QuitApp | Minimize | Maximize | Scale(_) | ResetSpatial | Copy | Paste
            | SelectAll | HideApp | HideOthers | AppInfo => s.focused.is_some(),
            Cycle(_) => s
                .windows
                .iter()
                .any(|w| w.workspace == s.active_workspace && !w.minimized),
            _ => true,
        }
    }
    pub fn command(&mut self, c: Command) {
        if !self.command_enabled(c) {
            return;
        }

        let s = self.shell_scene();
        let id = s.focused;
        use Command::*;
        match c {
            ToggleXrBars | ToggleXrTop | ToggleXrBottom | ToggleXrDesktop => {
                let p = s.presentation;
                let both = !(p.top || p.bottom);
                self.action(Action::XrPresentation {
                    top: if c == ToggleXrBars {
                        Some(both)
                    } else if c == ToggleXrTop {
                        Some(!p.top)
                    } else {
                        None
                    },
                    bottom: if c == ToggleXrBars {
                        Some(both)
                    } else if c == ToggleXrBottom {
                        Some(!p.bottom)
                    } else {
                        None
                    },
                    desktop: if c == ToggleXrDesktop {
                        Some(!p.desktop)
                    } else {
                        None
                    },
                });
            }
            Appearance => {
                self.keyboard_draft = None;
                self.appearance_open = true;
            }
            KeyboardSettings => self.keyboard_settings_open(),
            XrSettings => {
                self.xr_settings_open = true;
                self.xr_settings_row = 0;
                self.action(Action::XrPresentation {
                    top: Some(true),
                    bottom: None,
                    desktop: None,
                });
            }
            AnchorHere => self.anchor_here(),
            StartXr => self.start_xr(),
            StopXr => self.xr_bridge.lock().unwrap().exit_requested = true,
            Session(action) => self.session_command(action),
            DesktopAction { app, action } => {
                if let Some(a) = self.apps.iter().find(|a| a.id == app) {
                    if let Some(action) = a.actions.get(action) {
                        if let Err(e) =
                            dome::service::launch_action(a, &action.id, &self.launch_socket())
                        {
                            self.shell_error = e;
                        }
                    }
                }
            }
            LaunchApp(app) => {
                if let Some(a) = self.apps.iter().find(|a| a.id == app) {
                    if let Err(e) = dome::service::launch(a, &self.launch_socket()) {
                        self.shell_error = e;
                    }
                }
            }
            AppInfo => {
                self.about = true;
                self.shell_error = "@app-info".into();
            }
            HideApp | HideOthers => {
                if let Some(w) = id.and_then(|id| s.window(id)) {
                    let app = &w.app_id;
                    for w in s.windows.iter().filter(|w| {
                        w.workspace == s.active_workspace && ((w.app_id == *app) == (c == HideApp))
                    }) {
                        self.action(Action::Minimize { id: w.id });
                    }
                }
            }
            ShowAll => {
                for w in s
                    .windows
                    .iter()
                    .filter(|w| w.workspace == s.active_workspace && w.minimized)
                {
                    self.action(Action::Focus { id: w.id });
                }
                if let Some(id) = id {
                    self.action(Action::Focus { id });
                }
            }
            Terminal => {
                if let Err(e) = dome::terminal::launch(&self.launch_socket()) {
                    self.shell_error = format!("Cannot launch Ghostty XR: {e}");
                }
            }
            Launch => {
                self.launcher = true;
                self.launch_query.clear();
            }
            Desktops => {
                self.desktop_manager = true;
            }
            Menu => {
                self.menu = Some(0);
                self.menu_hover = 0;
            }
            Close => {
                if let Some(id) = id {
                    self.action(Action::Close { id });
                }
            }
            Minimize => {
                if let Some(id) = id {
                    self.action(Action::Minimize { id });
                }
            }
            Maximize => {
                if let Some(id) = id {
                    self.action(Action::Maximize { id });
                }
            }
            QuitApp => {
                if let Some(w) = id.and_then(|i| s.window(i)) {
                    for i in s
                        .windows
                        .iter()
                        .filter(|a| a.app_id == w.app_id)
                        .map(|w| w.id)
                        .collect::<Vec<_>>()
                    {
                        self.action(Action::Close { id: i });
                    }
                }
            }
            Cycle(reverse) => {
                let ids: Vec<_> = s
                    .windows
                    .iter()
                    .filter(|w| w.workspace == s.active_workspace && !w.minimized)
                    .map(|w| w.id)
                    .collect();
                if !ids.is_empty() {
                    let i = ids.iter().position(|i| Some(*i) == id).unwrap_or(0);
                    let next = if reverse {
                        (i + ids.len() - 1) % ids.len()
                    } else {
                        (i + 1) % ids.len()
                    };
                    self.action(Action::Focus { id: ids[next] });
                }
            }
            Workspace(workspace) if workspace < s.workspaces.len() => {
                self.action(Action::SwitchWorkspace { workspace })
            }
            CycleWorkspace(delta) => self.action(Action::SwitchWorkspace {
                workspace: (s.active_workspace as i32 + delta).rem_euclid(s.workspaces.len() as i32)
                    as usize,
            }),
            MoveWorkspace(workspace) if workspace < s.workspaces.len() => {
                if let Some(w) = id.and_then(|i| s.window(i)) {
                    self.action(Action::Move {
                        id: w.id,
                        lane: w.lane,
                        workspace,
                    });
                }
            }
            MoveLane(delta) => {
                if let Some(w) = id.and_then(|i| s.window(i)) {
                    self.action(Action::Move {
                        id: w.id,
                        lane: (w.lane as i32 + delta).clamp(0, 2) as usize,
                        workspace: w.workspace,
                    });
                }
            }
            NewWorkspace => self.action(Action::AddWorkspace {
                name: format!("Desktop {}", s.workspaces.len() + 1),
            }),
            ResetSpatial => {
                if let Some(id) = id {
                    self.action(Action::ReturnToDome { id });
                    self.action(Action::DomeOffset {
                        id,
                        yaw_deg: 0.,
                        pitch_deg: 0.,
                    });
                    self.action(Action::SpatialTransform {
                        id,
                        pose: None,
                        scale: 1.,
                    });
                }
            }
            Scale(f) => {
                if let Some(w) = id.and_then(|i| s.window(i)) {
                    self.action(Action::SpatialTransform {
                        id: w.id,
                        pose: w.xr_pose,
                        scale: (w.xr_scale * f).clamp(0.25, 4.),
                    });
                }
            }
            Copy => self.client_chord(54),
            Paste => self.client_chord(55),
            SelectAll => self.client_chord(38),
            Projection => {
                let mut config = s.config;
                config.projection = if config.projection == dome::Projection::RoomDome {
                    dome::Projection::InView
                } else {
                    dome::Projection::RoomDome
                };
                self.action(Action::Configure { config });
            }
            About => {
                self.about = true;
                self.shell_error = "Dome — experimental Rust / Smithay / Qt6 desktop".into();
                eprintln!("{}", self.shell_error);
            }
            _ => (),
        }
    }
    fn client_chord(&mut self, key: u32) {
        if let Some(k) = self.seat.get_keyboard() {
            let time = self.start_time.elapsed().as_millis() as u32;
            let held = k.pressed_keys();
            // Translate the command modifier to Ctrl for applications, then restore
            // the physical modifier state so its eventual release remains balanced.
            let modifiers: Vec<_> = [50, 62, 64, 108, 133, 134]
                .into_iter()
                .filter(|code| held.contains(&Keycode::new(*code)))
                .collect();
            let ctrl = held.contains(&Keycode::new(37)) || held.contains(&Keycode::new(105));
            let mut sequence: Vec<_> = modifiers
                .iter()
                .map(|code| (*code, KeyState::Released))
                .collect();
            if !ctrl {
                sequence.push((37, KeyState::Pressed));
            }
            sequence.extend([(key, KeyState::Pressed), (key, KeyState::Released)]);
            if !ctrl {
                sequence.push((37, KeyState::Released));
            }
            sequence.extend(modifiers.into_iter().map(|code| (code, KeyState::Pressed)));
            for (code, state) in sequence {
                k.input::<(), _>(
                    self,
                    Keycode::new(code),
                    state,
                    SERIAL_COUNTER.next_serial(),
                    time,
                    |_, _, _| FilterResult::Forward,
                );
            }
        }
    }
}
