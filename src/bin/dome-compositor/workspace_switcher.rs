use crate::Dome;
use dome::{
    workspace_switcher::{desktops, validate_name, Hit, Layout, Rename, MAX_DESKTOPS},
    Action,
};

impl Dome {
    pub fn open_desktops(&mut self) {
        self.shell_selection = self.scene.lock().unwrap().active_workspace;
        self.desktop_rename = None;
        self.desktop_manager = true;
    }
    fn begin_desktop_rename(&mut self, workspace: usize) {
        let name = self
            .scene
            .lock()
            .unwrap()
            .workspaces
            .get(workspace)
            .map(|w| w.name.clone());
        if let Some(name) = name {
            self.shell_selection = workspace;
            self.desktop_rename = Some(Rename::new(workspace, name));
        }
    }
    fn save_desktop_rename(&mut self) {
        let Some(draft) = &self.desktop_rename else {
            return;
        };
        let result = (|| {
            let name = validate_name(&draft.text)?.to_owned();
            let mut scene = self.scene.lock().unwrap();
            if scene
                .workspaces
                .get(draft.workspace)
                .is_none_or(|w| w.name != draft.original)
            {
                return Err("Desktop changed; cancel and try renaming again".to_string());
            }
            scene.apply(Action::RenameWorkspace {
                workspace: draft.workspace,
                name,
            })
        })();
        match result {
            Ok(()) => self.desktop_rename = None,
            Err(error) => {
                if let Some(draft) = self.desktop_rename.as_mut() {
                    draft.error = error;
                }
            }
        }
    }
    pub fn desktop_rename_key(
        &mut self,
        key: u32,
        character: Option<char>,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        if !self.desktop_manager {
            self.desktop_rename = None;
            return false;
        }
        if self.desktop_rename.is_none() {
            return false;
        }
        match key {
            0xff1b => self.desktop_rename = None,
            0xff0d | 0xff8d => self.save_desktop_rename(),
            _ => self
                .desktop_rename
                .as_mut()
                .unwrap()
                .key(key, character, ctrl, shift),
        }
        true
    }
    pub fn desktop_switcher_key(&mut self, key: u32) {
        let count = self.scene.lock().unwrap().workspaces.len();
        let can_add = count < MAX_DESKTOPS;
        if let Some(row) = crate::menu_navigation::selection(
            self.shell_selection,
            count + usize::from(can_add),
            key,
        ) {
            self.shell_selection = row;
        } else {
            match key {
                0xff1b => {
                    self.desktop_manager = false;
                    self.desktop_rename = None;
                }
                0xffbf => self.begin_desktop_rename(self.shell_selection), // F2
                0xff0d | 0xff8d | 0x20 => {
                    if self.shell_selection < count {
                        self.action(Action::SwitchWorkspace {
                            workspace: self.shell_selection,
                        });
                    } else if can_add {
                        self.action(Action::AddWorkspace {
                            name: format!("Desktop {}", count + 1),
                        });
                    }
                    self.desktop_manager = false;
                }
                _ => (),
            }
        }
    }
    pub fn desktop_switcher_click(&mut self, x: f64, y: f64, width: i32, height: i32) {
        let count = self.scene.lock().unwrap().workspaces.len();
        let layout = Layout::new(width, height, count);
        match layout.hit(x, y, self.desktop_rename.as_ref().map(|r| r.workspace)) {
            Hit::Outside => {
                self.desktop_manager = false;
                self.desktop_rename = None;
            }
            Hit::Switch(workspace) => {
                self.action(Action::SwitchWorkspace { workspace });
                self.desktop_manager = false;
            }
            Hit::Rename(workspace) => self.begin_desktop_rename(workspace),
            Hit::Input => {
                if let Some(draft) = self.desktop_rename.as_mut() {
                    draft.select_all();
                }
            }
            Hit::Save => self.save_desktop_rename(),
            Hit::Cancel => self.desktop_rename = None,
            Hit::Add => {
                self.action(Action::AddWorkspace {
                    name: format!("Desktop {}", count + 1),
                });
                self.desktop_manager = false;
            }
            Hit::None => (),
        }
    }
    pub fn workspace_switcher_json(&self, width: i32, height: i32) -> serde_json::Value {
        let scene = self.scene.lock().unwrap();
        let mut rows = desktops(&scene);
        for window in &scene.windows {
            if self.selection_tooltips.contains_key(&window.id) && scene.popup(window.id).is_none()
            {
                if let Some(row) = rows.get_mut(window.workspace) {
                    row.windows = row.windows.saturating_sub(1);
                }
            }
        }
        serde_json::json!({"layout":Layout::new(width,height,scene.workspaces.len()),
            "rows":rows,"edit":if self.desktop_manager {self.desktop_rename.as_ref()}else{None}})
    }
}
