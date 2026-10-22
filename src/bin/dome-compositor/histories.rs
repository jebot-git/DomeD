//! Shell-owned clipboard and notification history panes, shared by desktop and XR.
use crate::Dome;
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Clipboard,
    Notifications,
}
impl Dome {
    fn history_rows(&self) -> Vec<serde_json::Value> {
        match self.history_panel {
            Some(Kind::Clipboard)=>self.clipboard.entries.iter().map(|e|serde_json::json!({"id":e.id,"text":e.text(),"detail":e.description(),"timestamp":e.timestamp})).collect(),
            Some(Kind::Notifications)=>self.notifications.history().iter().map(|e|serde_json::json!({"id":e.id,"text":format!("{}\n{}",e.summary,e.body),"detail":e.app,"timestamp":e.timestamp,"image":e.image})).collect(),
            None=>vec![],
        }
    }
    pub fn history_rect(&self, width: i32, height: i32) -> [f64; 4] {
        let w = 760.min((width - 24).max(1)) as f64;
        let h = 620.min((height - 108).max(1)) as f64;
        [(width as f64 - w) / 2., 42., w, h]
    }
    pub fn history_json(&self, width: i32, height: i32) -> serde_json::Value {
        let Some(kind) = self.history_panel else {
            return serde_json::Value::Null;
        };
        let rows = self.history_rows();
        let [x, y, w, h] = self.history_rect(width, height);
        let visible = ((h - 108.) / 64.).floor().max(1.) as usize;
        let selected = self.history_selected.min(rows.len().saturating_sub(1));
        let first = selected
            .saturating_sub(visible - 1)
            .min(rows.len().saturating_sub(visible));
        serde_json::json!({"kind":kind,"title":if kind==Kind::Clipboard {"Clipboard history"}else{"Notification history"},"x":x,"y":y,"w":w,"h":h,"rows":rows,"first":first,"visible":visible,"selected":selected})
    }
    pub fn open_history(&mut self, kind: Kind) {
        self.clock.open = false;
        self.close_audio();
        self.history_panel = if self.history_panel == Some(kind) {
            None
        } else {
            Some(kind)
        };
        self.history_selected = 0;
        self.menu = None;
        self.launcher = false;
        self.desktop_manager = false;
        self.task_popup = None;
        self.default_apps_open = false;
        self.appearance_open = false;
        self.xr_settings_open = false;
        self.binding_capture = None;
        self.tray.close();
        if kind == Kind::Notifications {
            self.notifications.mark_read();
        }
    }
    fn history_activate(&mut self) {
        if self.history_panel == Some(Kind::Clipboard) {
            let id = self
                .history_rows()
                .get(self.history_selected)
                .and_then(|r| r["id"].as_u64());
            if let Some(id) = id {
                self.restore_clipboard(id);
                self.history_panel = None;
            }
        }
    }
    fn history_remove(&mut self) {
        if let Some(id) = self
            .history_rows()
            .get(self.history_selected)
            .and_then(|r| r["id"].as_u64())
        {
            match self.history_panel {
                Some(Kind::Clipboard) => self.clipboard.remove(id),
                Some(Kind::Notifications) => self.notifications.remove_history(id as u32),
                None => (),
            };
        }
        self.history_selected = self
            .history_selected
            .min(self.history_rows().len().saturating_sub(1));
    }
    pub fn history_key(&mut self, key: u32) -> bool {
        if self.history_panel.is_none() {
            return false;
        }
        let last = self.history_rows().len().saturating_sub(1);
        match key {
            0xff1b => self.history_panel = None,
            0xff52 => self.history_selected = self.history_selected.saturating_sub(1),
            0xff54 => self.history_selected = (self.history_selected + 1).min(last),
            0xff50 => self.history_selected = 0,
            0xff57 => self.history_selected = last,
            0xff55 => self.history_selected = self.history_selected.saturating_sub(5),
            0xff56 => self.history_selected = (self.history_selected + 5).min(last),
            0xff0d => self.history_activate(),
            0xffff | 0xff08 => self.history_remove(),
            _ => (),
        };
        true
    }
    pub fn history_scroll(&mut self, delta: f64) -> bool {
        if self.history_panel.is_none() {
            return false;
        }
        if delta != 0. {
            let last = self.history_rows().len().saturating_sub(1);
            self.history_selected = if delta > 0. {
                (self.history_selected + 3).min(last)
            } else {
                self.history_selected.saturating_sub(3)
            };
        }
        true
    }
    pub fn history_click(&mut self, x: f64, y: f64, width: i32, height: i32) -> bool {
        if self.history_panel.is_none() {
            return false;
        }
        let [left, top, w, h] = self.history_rect(width, height);
        if x < left || x >= left + w || y < top || y >= top + h {
            self.history_panel = None;
            return true;
        }
        let visible = ((h - 108.) / 64.).floor().max(1.) as usize;
        let count = self.history_rows().len();
        let first = self
            .history_selected
            .saturating_sub(visible - 1)
            .min(count.saturating_sub(visible));
        if y >= top + 44. && y < top + 44. + visible as f64 * 64. {
            let index = first + ((y - top - 44.) / 64.) as usize;
            if index < count {
                self.history_selected = index;
                if x >= left + w - 38. {
                    self.history_remove();
                } else {
                    self.history_activate();
                }
            }
        } else if y >= top + h - 54. {
            if x < left + w / 2. {
                match self.history_panel {
                    Some(Kind::Clipboard) => self.clipboard.clear(),
                    Some(Kind::Notifications) => self.notifications.clear_history(),
                    None => (),
                };
                self.history_selected = 0;
            } else {
                self.history_panel = None;
            }
        }
        true
    }
    pub fn activate_tray(&mut self, index: usize, x: i32, y: i32, context: bool) {
        match index {
            0 => self.toggle_audio(),
            1 => self.open_history(Kind::Clipboard),
            2 => self.open_history(Kind::Notifications),
            3 if !self.tray.state().screenshot_managed => self.start_screenshot(),
            _ => {
                self.history_panel = None;
                self.close_audio();
                self.tray
                    .activate(index - if self.tray.state().screenshot_managed { 3 } else { 4 }, x, y, context);
            }
        }
    }
}
