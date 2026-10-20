use crate::Dome;
use base64::Engine;
use dome::{
    taskbar::{self, Activation, Task},
    Action,
};
fn pins_path() -> std::path::PathBuf {
    let root = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    std::path::PathBuf::from(root).join("dome/taskbar.json")
}
pub fn load_pins() -> Vec<String> {
    std::fs::read(pins_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .take(32)
        .collect()
}
impl Dome {
    pub fn task_list(&self) -> Vec<Task> {
        taskbar::tasks(&self.shell_scene(), &self.apps, &self.pins)
    }
    pub fn popup_rect(&self, index: usize, width: i32, height: i32) -> (f64, f64, f64, f64) {
        let count = self
            .task_list()
            .get(index)
            .map(|t| {
                if self.task_context {
                    t.app
                        .and_then(|id| self.apps.iter().find(|a| a.id == id))
                        .map(|a| a.actions.len())
                        .unwrap_or(0)
                        + 3
                } else {
                    t.windows.len().min(4)
                }
            })
            .unwrap_or(0);
        if self.task_context {
            let work = dome::Rect {
                x: 0.,
                y: 36.,
                w: width as f32,
                h: (height - 96).max(1) as f32,
            };
            let view = dome::context_menu::Viewport::new(
                count as f32 * 32. + 12.,
                work.h,
                self.task_context_offset,
            );
            let click = self
                .task_context_at
                .unwrap_or([110. + index as f32 * 130., height as f32 - 30.]);
            let rect = dome::context_menu::rect(
                dome::Rect {
                    x: 0.,
                    y: 0.,
                    w: width as f32,
                    h: height as f32,
                },
                click,
                [320., view.height],
                work,
            );
            return (rect.x as f64, rect.y as f64, rect.w as f64, rect.h as f64);
        }
        let row = 88.;
        let x = (105. + index as f64 * 130.)
            .min((width - 330) as f64)
            .max(4.);
        let h = count as f64 * row + 12.;
        (x, (height - 60) as f64 - h, 320., h)
    }
    pub fn task_popup_contains(&self, index: usize, x: f64, y: f64, w: i32, h: i32) -> bool {
        let (px, py, pw, ph) = self.popup_rect(index, w, h);
        x >= px && x < px + pw && y >= py && y < py + ph
    }
    pub fn task_motion(&mut self, x: f64, y: f64, w: i32, h: i32) {
        if self.audio_open
            || self.history_panel.is_some()
            || self.launcher
            || self.menu.is_some()
            || self.desktop_manager
            || self.about
            || self.appearance_open
            || self.xr_settings_open
            || self.session_confirm.is_some()
        {
            return;
        }
        if self.task_context {
            return;
        }
        if y >= (h - 60) as f64 && x >= 110. && x < (w - 250) as f64 {
            let i = ((x - 110.) / 130.) as usize;
            if self
                .task_list()
                .get(i)
                .is_some_and(|t| !t.windows.is_empty())
            {
                self.task_popup = Some(i);
            } else {
                self.task_popup = None;
            }
        } else if !self
            .task_popup
            .is_some_and(|i| self.task_popup_contains(i, x, y, w, h))
        {
            self.task_popup = None;
        }
    }
    pub fn activate_task(&mut self, index: usize, button: u32) {
        let Some(task) = self.task_list().get(index).cloned() else {
            return;
        };
        self.task_context = button == 0x111;
        if self.task_context {
            let pos = self.xr_shell_pos.unwrap_or_else(|| {
                let p = self.seat.get_pointer().unwrap().current_location();
                let p = p - self.shell_geometry().loc.to_f64();
                [p.x, p.y]
            });
            self.task_context_at = Some([pos[0] as f32, pos[1] as f32]);
            self.task_context_offset = 0.;
            self.task_context_selected = 0;
            self.task_popup = Some(index);
            return;
        }
        if button == 0x112 {
            if let Some(id) = task.app {
                self.command(dome::shortcuts::Command::LaunchApp(id));
            }
            return;
        }
        match taskbar::activate(&task) {
            Activation::Launch(id) => self.command(dome::shortcuts::Command::LaunchApp(id)),
            Activation::Focus(id) => self.action(Action::Focus { id }),
            Activation::Minimize(id) => self.action(Action::Minimize { id }),
            Activation::Group => {
                self.task_popup = Some(index);
                return;
            }
        }
        self.task_popup = None;
    }
    pub fn task_popup_click(&mut self, x: f64, y: f64, w: i32, h: i32) -> bool {
        let Some(index) = self.task_popup else {
            return false;
        };
        if y >= (h - 60) as f64 {
            return false;
        }
        let rect = self.popup_rect(index, w, h);
        let inside = self.task_popup_contains(index, x, y, w, h);
        let context = self.task_context;
        let viewport = self.task_context_viewport(h);
        if inside && context {
            if let Some(dir) = viewport.control((y - rect.1) as f32) {
                self.task_context_offset =
                    (self.task_context_offset + dir as f32 * 96.).clamp(0., viewport.max_offset());
                return true;
            }
        }
        self.task_popup = None;
        self.task_context = false;
        if !inside {
            return context;
        }
        let Some(task) = self.task_list().get(index).cloned() else {
            return true;
        };
        let local_y = if context {
            viewport.content_y((y - rect.1) as f32) as f64
        } else {
            y - rect.1
        };
        let row = ((local_y - 6.) / if context { 32. } else { 88. }).max(0.) as usize;
        if context {
            let app = task
                .app
                .and_then(|id| self.apps.iter().find(|a| a.id == id))
                .cloned();
            let actions = app.as_ref().map(|a| a.actions.len()).unwrap_or(0);
            if row == 0 {
                if let Some(id) = task.app {
                    self.command(dome::shortcuts::Command::LaunchApp(id));
                }
            } else if row <= actions {
                let app = app.unwrap();
                self.command(dome::shortcuts::Command::DesktopAction {
                    app: app.id,
                    action: row - 1,
                });
            } else if row == actions + 1 {
                if task.app.is_some() {
                    if task.pinned {
                        self.pins.retain(|p| p != &task.key);
                    } else {
                        self.pins.push(task.key);
                    }
                    let path = pins_path();
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Ok(data) = serde_json::to_vec(&self.pins) {
                        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
                        if std::fs::write(&temp, data).is_ok() {
                            let _ = std::fs::rename(temp, path);
                        }
                    }
                }
            } else if row == actions + 2 {
                for id in task.windows {
                    self.action(Action::Close { id });
                }
            }
        } else if let Some(id) = task.windows.get(row) {
            self.action(Action::Focus { id: *id });
        }
        true
    }
    pub fn task_scroll(&mut self, delta: f64) -> bool {
        if delta == 0. {
            return false;
        }
        let pointer = self.seat.get_pointer().unwrap().current_location();
        if self.task_context {
            let viewport = self.task_context_viewport(0);
            self.task_context_offset =
                (self.task_context_offset + delta as f32 * 3.).clamp(0., viewport.max_offset());
            return true;
        }
        let size = self.shell_geometry().size;
        let pointer = pointer - self.shell_geometry().loc.to_f64();
        if pointer.y < (size.h - 60) as f64
            || pointer.x < 110.
            || pointer.x >= (size.w - 250) as f64
        {
            return false;
        }
        let tasks = self.task_list();
        let Some(task) = tasks.get(((pointer.x - 110.) / 130.) as usize) else {
            return true;
        };
        if task.windows.is_empty() {
            return true;
        }
        let focused = self.scene.lock().unwrap().focused;
        let index = task
            .windows
            .iter()
            .position(|id| Some(*id) == focused)
            .unwrap_or(0);
        let index = if delta < 0. {
            (index + 1) % task.windows.len()
        } else {
            (index + task.windows.len() - 1) % task.windows.len()
        };
        self.action(Action::Focus {
            id: task.windows[index],
        });
        true
    }
    pub fn task_popup_json(&self, w: i32, h: i32) -> serde_json::Value {
        let Some(index) = self.task_popup else {
            return serde_json::Value::Null;
        };
        let Some(task) = self.task_list().get(index).cloned() else {
            return serde_json::Value::Null;
        };
        let (x, y, width, height) = self.popup_rect(index, w, h);
        if self.task_context {
            let app = task
                .app
                .and_then(|id| self.apps.iter().find(|a| a.id == id));
            let mut rows = vec![serde_json::json!({"text":"New Window","enabled":app.is_some()})];
            if let Some(app) = app {
                rows.extend(
                    app.actions
                        .iter()
                        .map(|a| serde_json::json!({"text":a.name,"enabled":true})),
                );
            }
            rows.push(serde_json::json!({"text":if task.pinned{"Unpin from Taskbar"}else{"Pin to Taskbar"},"enabled":app.is_some()}));
            rows.push(
                serde_json::json!({"text":"Close All Windows","enabled":!task.windows.is_empty()}),
            );
            let viewport = self.task_context_viewport(h);
            return serde_json::json!({"x":x,"y":y,"w":width,"h":height,"context":true,"rows":rows,
                "scroll_band":viewport.band(),"offset":viewport.offset,"can_up":viewport.offset>0.,"can_down":viewport.offset<viewport.max_offset(),"selected":self.task_context_selected});
        }
        let scene = self.scene.lock().unwrap();
        let mut bridge = self.xr_bridge.lock().unwrap();
        let rows:Vec<_>=task.windows.iter().take(4).filter_map(|id|{let window=scene.window(*id)?;bridge.requested.insert(*id,std::time::Instant::now());let frame=bridge.frames.get(id);Some(serde_json::json!({"text":window.title,"minimized":window.minimized,"active":scene.focused==Some(*id),"width":frame.map(|f|f.width),"height":frame.map(|f|f.height),"rgba":frame.map(|f|base64::engine::general_purpose::STANDARD.encode(&*f.rgba))}))}).collect();
        serde_json::json!({"x":x,"y":y,"w":width,"h":height,"context":false,"rows":rows})
    }
}

impl Dome {
    pub fn task_context_viewport(&self, _height: i32) -> dome::context_menu::Viewport {
        let count = self
            .task_popup
            .and_then(|index| self.task_list().get(index).cloned())
            .map(|t| {
                t.app
                    .and_then(|id| self.apps.iter().find(|a| a.id == id))
                    .map_or(0, |a| a.actions.len())
                    + 3
            })
            .unwrap_or(0);
        dome::context_menu::Viewport::new(
            count as f32 * 32. + 12.,
            self.popup_work_area().h,
            self.task_context_offset,
        )
    }
    pub fn task_context_key(&mut self, key: u32) -> bool {
        if !self.task_context || self.task_popup.is_none() {
            return false;
        }
        let mut view = self.task_context_viewport(0);
        let count = ((view.content_height - 12.) / 32.).max(0.) as usize;
        match key {
            0xff52 => self.task_context_selected = self.task_context_selected.saturating_sub(1),
            0xff54 => {
                self.task_context_selected =
                    (self.task_context_selected + 1).min(count.saturating_sub(1))
            }
            0xff55 => view.scroll(-view.body()),
            0xff56 => view.scroll(view.body()),
            0xff50 => self.task_context_selected = 0,
            0xff57 => self.task_context_selected = count.saturating_sub(1),
            0xff0d => {
                if let Some(index) = self.task_popup {
                    let size = self.popup_work_area();
                    let rect = self.popup_rect(index, size.w as i32, (size.h + 96.) as i32);
                    let y = rect.1
                        + view.band() as f64
                        + 6.
                        + self.task_context_selected as f64 * 32.
                        + 16.
                        - view.offset as f64;
                    self.task_popup_click(rect.0 + 20., y, size.w as i32, (size.h + 96.) as i32);
                }
                return true;
            }
            _ => return false,
        }
        if !matches!(key, 0xff55 | 0xff56) {
            view.reveal(6. + self.task_context_selected as f32 * 32., 32.);
        }
        self.task_context_offset = view.offset;
        true
    }
}
