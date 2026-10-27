use crate::Dome;
use dome::{shortcuts::Command, Action};
use smithay::{
    backend::input::{KeyState, Keycode},
    input::keyboard::FilterResult,
    utils::SERIAL_COUNTER,
};
impl Dome {
    pub fn menu_index(&self, x: f64) -> Option<usize> {
        if x < 0. {
            return None;
        }
        let viewport = self.menu_header_viewport(self.shell_geometry().size.w);
        self.menu_headers().iter().enumerate()
            .find(|(i, h)| (*i < 2 && x < viewport.left as f64 || *i >= 2 && viewport.contains(x))
                && x >= h.x as f64 && x < (h.x + h.width) as f64 && h.enabled)
            .map(|(i, _)| i)
    }
    pub fn menu_items(&self, m: usize) -> Vec<(String, &'static str, Command)> {
        if self.panel_menu(m) {
            return Vec::new();
        }
        self.shell_menu_items(m)
    }
    pub fn shell_menu_items(&self, m: usize) -> Vec<(String, &'static str, Command)> {
        use Command::*;
        if m == 1 {
            let scene = self.shell_scene();
            let w = scene.focused.and_then(|id| scene.window(id));
            let app = w.and_then(|w| dome::desktop_entries::resolve(&self.apps, &w.app_id));
            let name = w.map(|w| dome::desktop_entries::display_name(app, &w.title))
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
                ("Open Files", "⌘E", FileManager),
                ("Open Terminal", "⌘T", Terminal),
                ("Take Screenshot", "PrintScreen", Screenshot),
                ("Settings…", "⌘,", XrSettings),
                ("Keybinding Help…", "", KeybindingsHelp),
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
            ],
            1 => vec![("Close Application Windows", "⌘Q", QuitApp)],
            2 => vec![("Close Window", "⌘W", Close)],
            3 => vec![
                ("Undo", "Ctrl+Z", Undo),
                ("Redo", "Ctrl+Shift+Z", Redo),
                ("", "", Separator),
                ("Cut", "Ctrl+X", Cut),
                ("Copy", "⌘C", Copy),
                ("Paste", "⌘V", Paste),
                ("Select All", "⌘A", SelectAll),
            ],
            4 => vec![
                ("Increase XR Window Size", "⌘+", Scale(1.1)),
                ("Decrease XR Window Size", "⌘−", Scale(1. / 1.1)),
                ("Pull Window Out / Return to Dome", "⌘P", PullOut),
                ("Return to Dome Layout", "⇧⌘R", ResetSpatial),
                ("", "", Separator),
                ("Show / Hide XR Bars", "⇧⌘B", ToggleXrBars),
                ("Show / Hide Upper XR Bar", "⌥⌘T", ToggleXrTop),
                ("Show / Hide XR Taskbar", "⌥⌘B", ToggleXrBottom),
                ("Show / Hide XR Desktop", "⇧⌘D", ToggleXrDesktop),
                ("Settings…", "⌘,", XrSettings),
                ("Anchor Dome at Headset", "⌃⌘R", AnchorHere),
                ("", "", Separator),
                ("Focus Previous Lane", "⌃⌥↑", CycleLane(-1)),
                ("Focus Next Lane", "⌃⌥↓", CycleLane(1)),
                ("Move Window Up a Lane", "⌥⌘↑", MoveLane(-1)),
                ("Move Window Down a Lane", "⌥⌘↓", MoveLane(1)),
                ("Move Window Left in Lane", "⌥⌘←", MoveHorizontal(-1)),
                ("Move Window Right in Lane", "⌥⌘→", MoveHorizontal(1)),
            ],
            5 => vec![
                ("Send to Desktop", "", PresentWindow { desktop: true, xr: false }),
                ("Send Window to XR", "", PresentWindow { desktop: false, xr: true }),
                ("Show Window on Both", "", PresentWindow { desktop: true, xr: true }),
                ("", "", Separator),
                ("Minimize", "⌘M", Minimize),
                ("Zoom", "⌘Return", Maximize),
                ("Next Window in Lane", "Alt Tab", Cycle(false)),
                ("Previous Window in Lane", "⇧Alt Tab", Cycle(true)),
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
            SwitchInputRealm => self.xr_bridge.lock().unwrap().visible(),
            SendToOtherRealm => {
                s.focused.and_then(|id| s.window(id)).is_some_and(|w| !w.minimized)
                    && (self.input_realm == dome::Realm::Xr || self.xr_bridge.lock().unwrap().visible())
            }
            PresentWindow { desktop, xr } => (desktop || xr)
                && s.focused.and_then(|id| s.window(id)).is_some_and(|w| !w.minimized)
                && (!xr || self.xr_bridge.lock().unwrap().visible()),
            PullOut => {
                self.xr_bridge.lock().unwrap().visible()
                    && s.focused
                        .and_then(|id| s.window(id))
                        .is_some_and(|w| !w.minimized)
            }
            CycleLane(delta) => dome::input_routing::lane_navigation_available(&s, self.input_realm)
                && dome::input_routing::cycle_lane(&s, delta).is_some(),
            MoveLane(delta) => s.focused.and_then(|id| s.window(id)).is_some_and(|w| {
                !w.minimized && (0..3).contains(&(w.lane as i32 + delta))
            }),
            MoveHorizontal(_) => s.focused.and_then(|id| s.window(id)).is_some_and(|w| {
                let order = s.carousel_order(w.id);
                order.len() > 1 && order.contains(&w.id)
            }),
            Separator => false,
            Close | QuitApp | Minimize | Maximize | Scale(_) | ResetSpatial | Undo | Redo | Cut | Copy | Paste
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
            Screenshot => self.start_screenshot(),
            SwitchInputRealm => {
                self.input_focus_xr = Some(!self.input_xr_active);
                self.sync_input_route();
            }
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
            DefaultApplications => self.default_apps_open(),
            Appearance => {
                self.shell_selection = 0;
                self.default_apps_open = false;
                self.keyboard_draft = None;
                self.appearance_open = true;
            }
            KeyboardSettings => self.keyboard_settings_open(),
            XrSettings | KeybindingsHelp => {
                self.gpu_choices = dome::gpu_settings::discover();
                self.xr_settings_open = true;
                self.settings_page = if c == KeybindingsHelp { crate::xr_settings::SettingsPage::Keybindings } else { crate::xr_settings::SettingsPage::General };
                self.xr_settings_row = 0;
                self.binding_capture = None;
                self.appearance_open = false;
                self.keyboard_draft = None;
                self.default_apps_open = false;
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
                    if let Err(e) =
                        dome::service::launch_focused(a, &self.launch_socket(), &self.scene)
                    {
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
            FileManager => {
                if let Err(e) = dome::file_manager::launch(&self.launch_socket(), &[]) {
                    self.shell_error = format!("Cannot launch file manager: {e}");
                }
            }
            Terminal => {
                if let Err(e) = dome::terminal::launch(&self.launch_socket(), self.input_realm) {
                    self.shell_error = format!("Cannot launch Ghostty XR: {e}");
                }
            }
            Launch => {
                self.open_launcher();
            }
            Desktops => {
                self.open_desktops();
            }
            Menu => {
                self.menu_has_ui_origin = false;
                self.open_menu(Some(0));
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
                let ids = dome::input_routing::cycle_windows(&s, self.input_realm);
                if !ids.is_empty() {
                    let i = ids.iter().position(|i| Some(*i) == id).unwrap_or(0);
                    let next = if reverse {
                        (i + ids.len() - 1) % ids.len()
                    } else {
                        (i + 1) % ids.len()
                    };
                    self.action(Action::Focus { id: ids[next] });
                    if self.input_realm == dome::Realm::Xr {
                        self.action(Action::Recenter { id: ids[next] });
                    }
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
            CycleLane(delta) => {
                if dome::input_routing::lane_navigation_available(&s, self.input_realm) {
                    if let Some(id) = dome::input_routing::cycle_lane(&s, delta) {
                        self.action(Action::Focus { id });
                        self.action(Action::Recenter { id });
                    }
                }
            }
            MoveHorizontal(delta) => {
                if let Some(id) = id { self.action(Action::Reorder { id, delta }); }
            }
            MoveLane(delta) => {
                if let Some(w) = id.and_then(|i| s.window(i)) {
                    if w.pullout_slot.is_some() {
                        self.action(Action::ReturnToDome { id: w.id });
                    }
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
            SendToOtherRealm | PresentWindow { .. } => {
                if let Some(id) = id {
                    let (desktop, xr) = match c {
                        PresentWindow { desktop, xr } => (desktop, xr),
                        _ => (self.input_realm == dome::Realm::Xr, self.input_realm != dome::Realm::Xr),
                    };
                    let root = s.popup_owner(id);
                    // Release held pointer buttons against the old surface before it disappears.
                    let pointer = self.seat.get_pointer().unwrap();
                    let time = self.start_time.elapsed().as_millis() as u32;
                    self.ignored_buttons.extend(self.physical_buttons.drain());
                    let mut buttons = self.desktop_buttons.drain()
                        .collect::<std::collections::HashSet<_>>();
                    buttons.extend(self.xr_buttons.drain().filter(|b| !self.xr_shell_buttons.contains(b)));
                    self.xr_shell_buttons.clear();
                    for button in buttons {
                        pointer.button(self, &smithay::input::pointer::ButtonEvent {
                            button, state: smithay::backend::input::ButtonState::Released,
                            serial: SERIAL_COUNTER.next_serial(), time,
                        });
                    }
                    self.dismiss_wayland_popups();
                    pointer.motion(
                        self,
                        None,
                        &smithay::input::pointer::MotionEvent {
                            location: pointer.current_location(),
                            serial: SERIAL_COUNTER.next_serial(),
                            time,
                        },
                    );
                    pointer.frame(self);
                    self.chrome_drag = None;
                    self.xr_target = None;
                    self.xr_shell_pos = None;
                    self.task_popup = None;
                    self.menu = None;
                    self.action(Action::WindowVisibility { id, desktop, xr });
                    if desktop { self.focus_history[0] = Some(Some(id)); }
                    if xr { self.focus_history[1] = Some(Some(id)); }
                    {
                        let mut scene = self.scene.lock().unwrap();
                        scene.focused = dome::input_routing::restore_focus(&scene, Some(Some(id)), self.input_realm);
                        scene.revision += 1;
                    }
                    let mut bridge = self.xr_bridge.lock().unwrap();
                    bridge.cursor = None;
                    bridge.mouse_buttons.clear();
                    bridge.packets.clear();
                    bridge.requested.remove(&root);
                    drop(bridge);
                    self.applied_revision = u64::MAX;
                    self.sync_scene();
                }
            }
            PullOut => {
                if let Some(w) = id.and_then(|id| s.window(id)) {
                    if w.pullout_slot.is_some() {
                        self.action(Action::ReturnToDome { id: w.id });
                    } else {
                        let head = self.xr_bridge.lock().unwrap().session.head;
                        self.action(Action::PullOut { id: w.id, head });
                    }
                    // The next XR tick rebases the existing cursor UV through
                    // the new surface, outside the keyboard input callback.
                }
            }
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
            Undo => self.client_chord(52),
            Redo => self.client_chord_with_shift(52, true),
            Cut => self.client_chord(53),
            Copy => self.client_chord(54),
            Paste => self.client_chord(55),
            SelectAll => self.client_chord(38),
            About => {
                self.about = true;
                self.shell_error = "Dome — experimental Rust / Smithay / FLTK desktop".into();
                eprintln!("{}", self.shell_error);
            }
            _ => (),
        }
    }
    fn client_chord(&mut self, key: u32) {
        self.client_chord_with_shift(key, false);
    }
    fn client_chord_with_shift(&mut self, key: u32, shift: bool) {
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
            if shift { sequence.push((50, KeyState::Pressed)); }
            sequence.extend([(key, KeyState::Pressed), (key, KeyState::Released)]);
            if shift { sequence.push((50, KeyState::Released)); }
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
