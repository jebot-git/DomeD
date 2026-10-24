use crate::Dome;
use dome::{service, Action};

use smithay::{
    desktop::Window,
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::SERIAL_COUNTER,
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};
impl Dome {
    pub fn register(&mut self, window: Window) {
        let x11_bridge = self.is_x11_bridge(window.toplevel().unwrap().wl_surface());
        let previous_focus = self.scene.lock().unwrap().focused;
        let id = self.scene.lock().unwrap().add("Untitled", "Application");
        let origin = window
            .toplevel()
            .unwrap()
            .wl_surface()
            .client()
            .and_then(|c| c.get_data::<crate::state::ClientState>().map(|s| s.origin))
            .unwrap_or_default();
        let visible = self.xr_bridge.lock().unwrap().visible();
        let primary = self.primary_output_geometry();
        let mut scene = self.scene.lock().unwrap();
        let w = scene.windows.iter_mut().find(|w| w.id == id).unwrap();
        w.desktop.x += primary.loc.x as f32;
        w.desktop.y += primary.loc.y as f32;
        w.origin = origin;
        if x11_bridge {
            w.xr_visible = false;
        }
        w.desktop_visible = origin == dome::Realm::Desktop || !visible;
        if x11_bridge && self.input_xr_active {
            self.focus_history[0] = Some(Some(id));
            scene.focused = previous_focus;
        }
        if origin == dome::Realm::Xr && visible && !self.input_xr_active {
            self.focus_history[1] = Some(Some(id));
            scene.focused = previous_focus;
        }
        drop(scene);
        self.notification_windows.previous_focus.insert(id, previous_focus);
        self.registry.push((id, window));
    }
    pub fn id_for(&self, surface: &WlSurface) -> Option<u64> {
        self.registry
            .iter()
            .find(|(_, w)| w.toplevel().unwrap().wl_surface() == surface)
            .map(|(id, _)| *id)
    }
    pub fn action(&mut self, mut a: Action) {
        if let Action::Focus{id}= &mut a {
            if let Some(anchor)=self.selection_tooltips.get(id) {
                if let Some(parent)=anchor.parent {*id=parent;}else{return;}
            }
        }
        if let Action::Focus { id } = &a {
            self.pending_raise = Some(*id);
            self.no_raise_focus = None;
        }
        if let Err(e) = self.scene.lock().unwrap().apply(a) {
            self.shell_error = e;
        }
    }
    pub fn metadata(&mut self, surface: &WlSurface) {
        if self.capture_notification_window(surface) { return; }
        self.selection_tooltip_metadata(surface);
        let x11_bridge = self.is_x11_bridge(surface);
        if let Some(id) = self.id_for(surface) {
            let (title, app) = with_states(surface, |s| {
                let a = s
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap();
                (a.title.clone(), a.app_id.clone())
            });
            let client_pid = surface.client()
                .and_then(|client|client.get_credentials(&self.display_handle).ok())
                .map_or(0,|cred|cred.pid as u32);
            let mut scene = self.scene.lock().unwrap();
            if let Some(w) = scene.windows.iter_mut().find(|w| w.id == id) {
                let entering_gmsh = w.app_id != dome::stereo::GMSH_SBS_APP_ID
                    && app.as_deref() == Some(dome::stereo::GMSH_SBS_APP_ID);
                w.client_pid = client_pid;
                w.title = title.unwrap_or("Untitled".into());
                w.app_id = app.unwrap_or("Application".into());
                w.stereo = dome::stereo::Stereo::for_app_id(&w.app_id);
                // Gmsh's live mesh view shares its GUI's Wayland connection,
                // but is a dedicated XR-only top level. Apply this on the
                // identity transition, preserving later user presentation edits.
                if entering_gmsh && !x11_bridge {
                    w.desktop_visible = false;
                    w.xr_visible = true;
                }
                scene.revision += 1;
            }
        }
    }
    pub fn sync_scene(&mut self) {
        self.persist_geometry();
        let scene = {
            let mut scene = self.scene.lock().unwrap();
            if self.last_applied_focus.is_some_and(|id| scene.window(id).is_none()) &&
                scene.focused.is_some_and(|id| scene.window(id).is_some_and(|w| !w.visible_in(self.input_realm))) {
                let restored = scene.recent_focus(Some(self.input_realm));
                if scene.focused != restored {
                    scene.focused = restored;
                    scene.revision += 1;
                }
            }
            scene.clone()
        };
        if scene.revision == self.applied_revision {
            return;
        }
        self.applied_revision = scene.revision;
        let popup_rects = self.desktop_popup_rects(&scene);
        let mut closed = vec![];
        for (id, window) in &self.registry {
            if self.notification_windows.entries.contains_key(id) { continue; }
            let Some(w) = scene.window(*id) else {
                window.toplevel().unwrap().send_close();
                self.space.unmap_elem(window);
                closed.push(*id);
                continue;
            };
            let mapped = w.desktop_visible
                && !w.minimized
                && w.workspace == scene.active_workspace
                && scene
                    .popup(*id)
                    .is_none_or(|p| scene.popup_visible(p, true));
            if !mapped {
                self.space.unmap_elem(window);
            }
            let output_geo = self.output_geometry_at(
                (
                    w.desktop.x as f64 + w.desktop.w as f64 / 2.,
                    w.desktop.y as f64 + w.desktop.h as f64 / 2.,
                )
                    .into(),
            );
            let size = output_geo.size;
            let pos = if let Some((_, rect)) = popup_rects.iter().find(|(i, _)| i == id) {
                (rect.x.round() as i32, rect.y.round() as i32)
            } else if w.maximized {
                (output_geo.loc.x, output_geo.loc.y + 36)
            } else {
                (w.desktop.x as i32, w.desktop.y as i32)
            };
            // map_element raises even an already mapped window. Repeating it
            // for every client commit would replace click stacking with registry order.
            let pos = pos.into();
            if mapped && self.space.element_location(window) != Some(pos) {
                self.space.map_element(window.clone(), pos, false);
            }
            let toplevel = window.toplevel().unwrap();
            let transient = self.selection_tooltips.contains_key(id) || with_states(toplevel.wl_surface(), |s| {
                s.data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .parent
                    .is_some()
            });
            let ready = with_states(toplevel.wl_surface(), |s| {
                s.data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });
            if ready {
                toplevel.with_pending_state(|s| {
                    use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State;
                    s.size=Some(if w.maximized && !(w.xr_media() && w.xr_visible) {
                        (size.w,(size.h-96).max(100)).into()
                    } else {
                        (w.desktop.w as i32,w.desktop.h as i32).into()
                    });
                    if transient { s.size = None; }
                    // XR stereo media treats compositor zoom as fullscreen too,
                    // allowing FLTK/mpv controls to reflect the actual presentation.
                    if w.xr_media() && w.maximized && w.xr_visible { s.states.set(State::Fullscreen); }
                    else { s.states.unset(State::Fullscreen); }
                    if w.maximized { s.states.set(State::Maximized); }
                    else { s.states.unset(State::Maximized); }
                });
                window.set_activated(scene.focused == Some(*id));
                toplevel.send_pending_configure();
            }
        }
        let restoring = std::mem::take(&mut self.restore_stacking);
        if restoring {
            for saved in &scene.windows {
                if let Some((_, window)) = self.registry.iter().find(|(id, _)| *id == saved.id) {
                    if self.space.element_location(window).is_some() { self.space.raise_element(window, false); }
                }
            }
        }
        let raise = self.pending_raise.take().or_else(|| {
            (scene.focused != self.last_applied_focus && scene.focused != self.no_raise_focus)
                .then_some(scene.focused)
                .flatten()
        });
        self.no_raise_focus = None;
        self.last_applied_focus = scene.focused;
        if let Some((_, window)) = self.registry.iter().find(|(id, _)| !restoring && Some(*id) == raise) {
            self.space.raise_element(window, true);
        }
        self.workbench_zoom
            .retain(|id, _| scene.window(*id).is_some());
        self.registry.retain(|(id, _)| !closed.contains(id));
        self.notification_windows.previous_focus.retain(|id, _| !closed.contains(id));
        let focused = (!scene.locked).then_some(scene.focused).flatten()
            .and_then(|id| self.registry.iter().find(|(i, _)| *i == id))
            .and_then(|(_, w)| {
                scene
                    .window(scene.focused.unwrap())
                    .filter(|w| {
                        !w.minimized
                            && w.workspace == scene.active_workspace
                            && scene
                                .popup(w.id)
                                .is_none_or(|p| scene.popup_visible(p, !self.input_xr_active))
                            && w.visible_in(self.input_realm)
                    })
                    .map(|_| w.toplevel().unwrap().wl_surface().clone())
            });
        if let Some(keyboard) = self.seat.get_keyboard() {
            use smithay::wayland::input_method::InputMethodKeyboardGrab;
            // IME grabs must follow desktop/XR focus changes. Check the active
            // grab itself so an application popup can still retain its focus.
            let follows_focus = keyboard
                .with_grab(|_, grab| grab.is::<InputMethodKeyboardGrab>())
                .unwrap_or(true);
            if scene.locked {
                keyboard.unset_grab(self);
                keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
            } else if follows_focus {
                let focused = self.context_focus_surface().or(focused);
                keyboard.set_focus(self, focused, SERIAL_COUNTER.next_serial());
            }
        }
    }
    pub fn launch_socket(&self) -> String {
        if self.input_realm == dome::Realm::Xr {
            &self.xr_socket_name
        } else {
            &self.socket_name
        }
        .to_string_lossy()
        .into_owned()
    }
    pub fn shell_scene(&self) -> dome::Scene {
        let scene = self.scene.lock().unwrap();
        let mut shell=if self.input_realm == dome::Realm::Desktop {
            scene.desktop_scene()
        } else {
            scene.realm_scene(dome::Realm::Xr)
        };
        shell.windows.retain(|w|!self.selection_tooltips.contains_key(&w.id));
        shell
    }
    pub fn launch_selected(&mut self) {
        if let Some(index)=self.launch_navigation.quick_focus {self.launch_quick(index);return;}

        let apps = self.matching_apps();
        let executable = (!self.launch_navigation.navigated || apps.is_empty())
            .then(|| dome::desktop_entries::find_executable(self.launch_query.trim()))
            .flatten();
        let result = if let Some(path) = executable {
            dome::desktop_entries::launch_executable(&path, &self.launch_socket())
        } else if let Some(app) = apps.get(self.launch_index) {
            service::launch_focused(app, &self.launch_socket(), &self.scene)
        } else {
            Err(format!("No application or executable found: {}", self.launch_query.trim()))
        };
        self.finish_launcher_launch(result);
    }
    pub fn matching_apps(&self) -> Vec<service::App> {
        let query = self.launch_query.to_lowercase();
        self.apps
            .iter()
            .filter(|a| a.search(&query))
            .cloned()
            .collect()
    }
    pub fn shell_modal_open(&self) -> bool {
        self.clock.open || self.audio_open || self.history_panel.is_some() || self.about
            || self.default_apps_open || self.appearance_open || self.xr_settings_open
            || self.session_confirm.is_some()
    }
    pub fn shell_at(&self, x: f64, y: f64) -> bool {
        let global = (x,y);
        let geometry = self.output_geometry_at((x, y).into());
        let origin = if self.input_realm == dome::Realm::Xr {
            (0, 0).into()
        } else {
            geometry.loc
        };
        let (x, y) = (x - origin.x as f64, y - origin.y as f64);
        let size = if self.input_realm == dome::Realm::Xr {
            self.shell_geometry().size
        } else {
            geometry.size
        };
        self.notifications
            .layout(size.w, size.h)
            .iter()
            .any(|n| n.contains(x, y))
            || y < 36.
            || y >= (size.h - 60) as f64
            || self.clock.open
            || self.audio_open
            || self.history_panel.is_some()
            || self.launcher
            || self.desktop_manager
            || self.menu.is_some()
            || self.about
            || self.xr_settings_open
            || self.default_apps_open
            || self.appearance_open
            || self.session_confirm.is_some()
            || self.tray.state().menu.is_some()
            || self
                .task_popup
                .is_some_and(|i| self.task_popup_contains(i, x, y, size.w, size.h))
            || (self.popup_under(global.into()).is_none()
                && self.decoration_at(global.0, global.1).is_some())
    }
    pub fn shell_click(&mut self, x: f64, y: f64) {
        let origin = if self.input_realm == dome::Realm::Xr {
            (0, 0).into()
        } else {
            self.output_geometry_at((x, y).into()).loc
        };
        let (x, y) = (x - origin.x as f64, y - origin.y as f64);
        let size = self.shell_geometry().size;
        if y >= 0. && y < 36. && x >= (size.w - self.clock_bar_width()) as f64 && x < size.w as f64 {
            self.toggle_clock(); return;
        }
        if self.clock_click(x,y,size.w,size.h) { return; }
        if self.default_apps_click(x,y,size.w,size.h) { return; }
        if self.audio_click(x, y, size.w, size.h) {
            return;
        }
        if self.history_click(x, y, size.w, size.h) {
            return;
        }
        if let Some(toast) = self
            .notifications
            .layout(size.w, size.h)
            .into_iter()
            .filter(|_| !self.shell_modal_open())
            .find(|n| n.contains(x, y))
        {
            self.notifications.click(&toast, x, y);
            return;
        }
        if self.appearance_open && self.keyboard_draft.is_some() {
            let size = self.shell_geometry().size;
            self.keyboard_settings_click(x, y, size.w, size.h);
            return;
        }
        if !self.shell_modal_open() && y < 36.
            && x >= (self.shell_geometry().size.w - self.clock_reserve()) as f64
            && x < (self.shell_geometry().size.w - self.clock_bar_width()) as f64
        {
            if self.keyboard_config.ime {
                self.history_panel = None;
                self.close_audio();
                self.open_menu(None);
                self.tray.keyboard();
            } else {
                self.keyboard_switch += 1;
            }
            return;
        }
        if self.xr_settings_open {
            let size = self.shell_geometry().size;
            self.xr_settings_click(x, y, size.w, size.h);
            return;
        }
        if self.appearance_open || self.session_confirm.is_some() {
            let size = self.shell_geometry().size;
            if self.appearance_open {
                self.appearance_click(x, y, size.w as f64, size.h as f64);
            } else {
                let (left, top) = ((size.w as f64 - 440.) / 2., (size.h as f64 - 200.) / 2.);
                if x >= left + 230. && x < left + 416. && y >= top + 140. && y < top + 180. {
                    self.confirm_session();
                } else {
                    self.session_confirm = None;
                }
            }
            return;
        }
        if self.about {
            self.about = false;
            return;
        }
        let scene = self.scene.lock().unwrap().clone();
        let size = self.shell_geometry().size;
        let (width, height) = (size.w as f64, size.h as f64);
        if self.task_popup.is_some() && self.task_popup_click(x, y, size.w, size.h) {
            return;
        }

        let tray = self.visible_tray(size.w);
        if let Some(index) = tray.menu {
            let x0 = if tray.keyboard_menu { width - 284. }
                else { width - self.clock_reserve() as f64 - (tray.items.len() - index) as f64 * 28. }
                .clamp(4., (width - 284.).max(4.));
            if x >= x0 && x < x0 + 280. && y >= 40. && y < 40. + tray.rows.len() as f64 * 28. {
                self.tray.select(((y - 40.) / 28.) as usize);
            } else {
                self.tray.close();
            }
            return;
        }
        if y < 36. {
            let x0 = width - self.clock_reserve() as f64 - tray.items.len() as f64 * 28.;
            if x >= x0 && x < width - self.clock_reserve() as f64 {
                self.activate_tray(((x - x0) / 28.) as usize, x as i32, y as i32, false);
                return;
            }
            let next = self.menu_index(x);
            self.open_menu(if self.menu == next { None } else { next });
            self.task_popup = None;
            self.task_context = false;
            self.menu_hover = -1;
            return;
        }
        if self.appmenu_click(x, y, size.w, size.h) {
            return;
        }
        if let Some(menu) = self.menu {
            let x0 = self.menu_x(menu);
            let viewport = self.menu_viewport(self.menu_items(menu).len(), 0, self.menu_hover, size.h);
            if x >= x0 && x < x0 + 380. && y >= 40. && y < 40. + viewport.height as f64 {
                if let Some(direction) = viewport.control((y - 40.) as f32) {
                    self.menu_scroll(x, y, direction as f64 * 28., size.w, size.h);
                    return;
                }
                let i = (viewport.content_y((y - 40.) as f32) / 28.) as usize;
                self.open_menu(None);
                if let Some((_, _, command)) = self.menu_items(menu).get(i) {
                    let command = *command;
                    self.command(command);
                }
            }
            self.open_menu(None);
            return;
        }
        if self.launcher {self.launcher_button(x,y,0x110,true);return;}
        if self.desktop_manager {
            if x > width - 320. && y > height - 380. && y < height - 100. {
                let i = ((y - (height - 380.)) / 32.) as usize;
                if i < scene.workspaces.len() {
                    self.action(Action::SwitchWorkspace { workspace: i });
                } else if i == scene.workspaces.len() {
                    self.action(Action::AddWorkspace {
                        name: format!("Desktop {}", i + 1),
                    });
                }
            }
            self.desktop_manager = false;
            return;
        }
        if y >= height - 60. {
            if x < 100. {
                self.open_launcher();
                return;
            }
            if x >= width - 90. {
                self.toggle_desktop_xr();
                return;
            }
            if x >= width - 250. {
                self.shell_selection = scene.active_workspace;
                self.desktop_manager = true;
                return;
            }
            if x >= 110. {
                self.activate_task(((x - 110.) / 130.) as usize, 0x110);
            }
            return;
        }
    }
    pub fn shell_pixels(&self, width: i32, height: i32) -> Vec<u8> {
        let ime = self.ime.snapshot();
        let scene = self.shell_scene();
        let focused = scene.focused.and_then(|id| scene.window(id));
        let entry = focused.and_then(|w| dome::desktop_entries::resolve(&self.apps, &w.app_id));
        let app = focused.map(|w| dome::desktop_entries::display_name(entry, &w.title))
            .unwrap_or("Desktop");
        let tasks = dome::taskbar::tasks(&scene, &self.apps, &self.pins);
        let items:Vec<_>=self.menu.map(|m|self.menu_items(m)).unwrap_or_default().iter().map(|(text,shortcut,command)|{let bindings=&scene.config.keybindings;let chord=if dome::shortcuts::defaults().iter().any(|b|b.command==*command){bindings.iter().find(|b|b.enabled&&b.command==*command).map(|b|b.label()).unwrap_or_default()}else{shortcut.to_string()};serde_json::json!({"text":text,"shortcut":chord,"enabled":self.command_enabled(*command),"separator":matches!(command,dome::shortcuts::Command::Separator)})}).collect();
        let popup = self.task_popup_json(width, height);
        let mut model = serde_json::json!({"kind":"shell","menu_headers":self.menu_headers(),"appmenu_panels":self.appmenu_panels(width,height),"keyboard":if self.keyboard_config.ime {dome::keyboard::input_method_label(&ime.method).map(str::to_owned).unwrap_or_else(||self.keyboard_config.label())}else{self.keyboard_config.label()},"keyboard_flag":dome::keyboard::input_method_flag(if self.keyboard_config.ime { &ime.method } else { self.keyboard_config.layout.split(',').nth(self.keyboard_config.active).unwrap_or("us") }),"keyboard_settings":self.keyboard_draft,"keyboard_field":self.keyboard_field,"keyboard_error":if self.keyboard_error.is_empty(){self.ime.snapshot().error}else{self.keyboard_error.clone()},"keyboard_shortcut":self.keyboard_draft.as_ref().map(|d|d.switch_shortcut.label()),"audio":self.audio_json(width,height),"history":self.history_json(width,height),"notifications": if !self.audio_open && self.history_panel.is_none() && self.input_realm == dome::Realm::Desktop {self.notifications.layout(width,height)} else {vec![]},"xr_settings":self.xr_settings_json(width,height),"default_apps":self.default_apps_json(width,height),"appearance":self.appearance,"appearance_open":self.appearance_open,"shell_selection":self.shell_selection,"session_confirm":self.session_confirm.map(|s|s.label()),"session_error":self.session.state().error,"about":self.about,"about_is_app":self.shell_error=="@app-info","width":width,"height":height,"app":app,"app_info":entry,"menu":self.menu.map(|m|m as i32).unwrap_or(-1),"hover":self.menu_hover,"items":items,"tasks":tasks,"tray":self.visible_tray(width),"task_popup":popup,"launcher":self.launcher,"launch_index":self.launch_index.saturating_sub(self.launch_navigation.offset),"launch_offset":self.launch_navigation.offset,"launch_total":self.matching_apps().len(),"launch_error":self.launch_navigation.error,"xr_active":self.input_xr_active,"manager":self.desktop_manager,"query":self.launch_query,"apps":self.matching_apps().into_iter().skip(self.launch_navigation.offset).take(crate::launcher::ROWS).collect::<Vec<_>>(),"desktops":scene.workspaces.iter().map(|w|w.name.clone()).collect::<Vec<_>>()});
        model["quick_launch"] = self.quick_launch_json();
        if self.appearance_open && self.keyboard_draft.is_none() {
            model["appearance_font"] = serde_json::to_value(self.appearance_font_description()).unwrap();
        }
        model["menu_viewport"] = serde_json::to_value(self.menu_viewport(items.len(), 0, self.menu_hover, height)).unwrap();
        model["clock"] = self.clock_json(width, height);
        crate::qt::paint(model)
    }
}
