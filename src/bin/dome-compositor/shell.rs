use crate::Dome;
use dome::{service, Action};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use smithay::{
    desktop::Window,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::SERIAL_COUNTER,
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};
impl Dome {
    pub fn register(&mut self, window: Window) {
        let id = self.scene.lock().unwrap().add("Untitled", "Application");
        self.registry.push((id, window));
    }
    pub fn id_for(&self, surface: &WlSurface) -> Option<u64> {
        self.registry
            .iter()
            .find(|(_, w)| w.toplevel().unwrap().wl_surface() == surface)
            .map(|(id, _)| *id)
    }
    pub fn action(&mut self, a: Action) {
        if let Err(e) = self.scene.lock().unwrap().apply(a) {
            self.shell_error = e;
        }
    }
    pub fn metadata(&mut self, surface: &WlSurface) {
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
            let mut scene = self.scene.lock().unwrap();
            if let Some(w) = scene.windows.iter_mut().find(|w| w.id == id) {
                w.title = title.unwrap_or("Untitled".into());
                w.app_id = app.unwrap_or("Application".into());
                scene.revision += 1;
            }
        }
    }
    pub fn sync_scene(&mut self) {
        let scene = self.scene.lock().unwrap().clone();
        if scene.revision == self.applied_revision {
            return;
        }
        self.applied_revision = scene.revision;
        let size = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
            .map(|r| r.size)
            .unwrap_or((1280, 800).into());
        let mut closed = vec![];
        for (id, window) in &self.registry {
            let Some(w) = scene.window(*id) else {
                window.toplevel().unwrap().send_close();
                self.space.unmap_elem(window);
                closed.push(*id);
                continue;
            };
            if w.minimized || w.workspace != scene.active_workspace {
                self.space.unmap_elem(window);
                continue;
            }
            let pos = if w.maximized {
                (0, 36)
            } else {
                (w.desktop.x as i32, w.desktop.y as i32)
            };
            self.space
                .map_element(window.clone(), pos, scene.focused == Some(*id));
            let toplevel = window.toplevel().unwrap();
            let ready = with_states(toplevel.wl_surface(), |s| {
                s.data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });
            if ready {
                toplevel.with_pending_state(|s|{s.size=Some(if w.maximized{(size.w,(size.h-96).max(100)).into()}else{(w.desktop.w as i32,w.desktop.h as i32).into()});
                    use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State;
                    if w.maximized{s.states.set(State::Maximized);}else{s.states.unset(State::Maximized);}
                });
                window.set_activated(scene.focused == Some(*id));
                toplevel.send_pending_configure();
            }
        }
        self.registry.retain(|(id, _)| !closed.contains(id));
        let focused = scene
            .focused
            .and_then(|id| self.registry.iter().find(|(i, _)| *i == id))
            .and_then(|(_, w)| {
                self.space
                    .element_location(w)
                    .map(|_| w.toplevel().unwrap().wl_surface().clone())
            });
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, focused, SERIAL_COUNTER.next_serial());
        }
    }
    pub fn launch_selected(&mut self) {
        let apps = self.matching_apps();
        if let Some(app) = apps.first() {
            if let Err(e) = service::launch(app, &self.socket_name.to_string_lossy()) {
                self.shell_error = e;
            }
        }
        self.launcher = false;
        self.launch_query.clear();
    }
    pub fn matching_apps(&self) -> Vec<service::App> {
        let query = self.launch_query.to_lowercase();
        self.apps
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&query))
            .take(8)
            .cloned()
            .collect()
    }
    pub fn shell_at(&self, x: f64, y: f64) -> bool {
        let size = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
            .map(|r| r.size)
            .unwrap_or((1280, 800).into());
        y < 36.
            || y >= (size.h - 60) as f64
            || self.launcher
            || self.desktop_manager
            || (self.hover_app.is_some() && y > (size.h - 210) as f64 && x > 90.)
    }
    pub fn shell_click(&mut self, x: f64, y: f64) {
        let scene = self.scene.lock().unwrap().clone();
        let size = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
            .map(|r| r.size)
            .unwrap_or((1280, 800).into());
        let (width, height) = (size.w as f64, size.h as f64);
        if self.launcher {
            if (100. ..420.).contains(&y) {
                let index = ((y - 100.) / 36.) as usize;
                if let Some(a) = self.matching_apps().get(index) {
                    if let Err(e) = service::launch(a, &self.socket_name.to_string_lossy()) {
                        self.shell_error = e;
                    }
                }
            }
            self.launcher = false;
            return;
        }
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
                self.launcher = true;
                self.launch_query.clear();
                return;
            }
            if x >= width - 160. {
                self.desktop_manager = true;
                return;
            }
            let windows: Vec<_> = scene
                .windows
                .iter()
                .filter(|w| w.workspace == scene.active_workspace)
                .collect();
            let index = ((x - 110.) / 130.) as usize;
            if let Some(w) = windows.get(index) {
                self.action(Action::Focus { id: w.id });
            }
            return;
        }
        if y < 36. {
            if let Some(id) = scene.focused {
                match x as i32 {
                    350..=449 => self.action(Action::Minimize { id }),
                    450..=549 => self.action(Action::Maximize { id }),
                    550..=649 => self.action(Action::Close { id }),
                    _ => (),
                }
            }
        }
    }
    pub fn shell_pixels(&self, width: i32, height: i32) -> Vec<u8> {
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let scene = self.scene.lock().unwrap().clone();
        let mut rect = |x: i32, y: i32, w: i32, h: i32, color: [u8; 4]| {
            for py in y.max(0)..(y + h).min(height) {
                for px in x.max(0)..(x + w).min(width) {
                    let p = (py as usize * width as usize + px as usize) * 4;
                    pixels[p..p + 4].copy_from_slice(&color);
                }
            }
        };
        rect(0, 0, width, 36, [38, 30, 23, 246]);
        rect(0, height - 60, width, 60, [38, 30, 23, 246]);
        if self.launcher {
            rect(20, 56, 600, 400, [45, 35, 26, 255]);
        }
        if self.desktop_manager {
            rect(width - 330, height - 390, 320, 320, [45, 35, 26, 255]);
        }
        // ARGB8888 byte order on little-endian hosts is BGRA; alpha is premultiplied.
        fn label(pixels: &mut [u8], width: i32, height: i32, x: i32, y: i32, text: &str) {
            let mut dx = x;
            for ch in text.chars() {
                if dx + 16 >= width {
                    break;
                }
                if let Some(glyph) = BASIC_FONTS.get(ch) {
                    for (row, bits) in glyph.iter().enumerate() {
                        for col in 0..8 {
                            if bits & (1 << col) != 0 {
                                for oy in 0..2 {
                                    for ox in 0..2 {
                                        let (px, py) = (dx + col * 2 + ox, y + row as i32 * 2 + oy);
                                        if px >= 0 && px < width && py >= 0 && py < height {
                                            let p =
                                                (py as usize * width as usize + px as usize) * 4;
                                            pixels[p..p + 4].copy_from_slice(&[223, 236, 238, 255]);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                dx += 16;
            }
        }
        let mut text = |x, y, s: &str| label(&mut pixels, width, height, x, y, s);
        let app = scene
            .focused
            .and_then(|id| scene.window(id))
            .map(|w| w.app_id.chars().take(12).collect::<String>())
            .unwrap_or("Desktop".into());
        text(12, 10, &format!("DOME  {app}"));
        text(350, 10, "Min");
        text(450, 10, "Max");
        text(550, 10, "Close");
        text(width - 280, 10, "Tray: pending");
        text(12, height - 36, "Launch");
        text(width - 150, height - 36, "Desktops");
        for (i, w) in scene
            .windows
            .iter()
            .filter(|w| w.workspace == scene.active_workspace)
            .enumerate()
        {
            if 110 + i as i32 * 130 > width - 280 {
                break;
            }
            text(
                110 + i as i32 * 130,
                height - 36,
                &format!(
                    "{}{}",
                    if w.minimized { "_" } else { "" },
                    w.title.chars().take(7).collect::<String>()
                ),
            );
        }
        if self.launcher {
            text(36, 72, &format!("Search: {}", self.launch_query));
            for (i, app) in self.matching_apps().iter().enumerate() {
                text(36, 110 + i as i32 * 36, &app.name);
            }
        }
        if self.desktop_manager {
            for (i, ws) in scene.workspaces.iter().enumerate() {
                text(
                    width - 310,
                    height - 378 + i as i32 * 32,
                    &format!("{} {}", i + 1, ws.name),
                );
            }
            text(
                width - 310,
                height - 378 + scene.workspaces.len() as i32 * 32,
                "+ New desktop",
            );
        }
        pixels
    }
}
