//! Desktop-specific MIME defaults, loaded and saved outside the input/render loop.
use crate::Dome;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString},
    sync::{mpsc, Arc, Mutex},
};
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Application {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Row {
    pub mime: String,
    pub label: String,
    pub current: Option<Application>,
    pub apps: Vec<Application>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Snapshot {
    pub rows: Vec<Row>,
    pub error: String,
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub revision: u64,
}
enum Request {
    Refresh,
    Set(String, String),
}
pub struct Defaults {
    send: mpsc::Sender<Request>,
    state: Arc<Mutex<Snapshot>>,
}
unsafe extern "C" {
    fn dome_default_apps_list() -> *const std::ffi::c_char;
    fn dome_default_app_set(
        mime: *const std::ffi::c_char,
        id: *const std::ffi::c_char,
    ) -> *const std::ffi::c_char;
}
impl Defaults {
    pub fn new() -> Self {
        let (send, receive) = mpsc::channel();
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let shared = state.clone();
        std::thread::spawn(move || {
            for request in receive {
                let error = match request {
                    Request::Refresh => String::new(),
                    Request::Set(mime, id) => match (CString::new(mime), CString::new(id)) {
                        (Ok(mime), Ok(id)) => unsafe {
                            CStr::from_ptr(dome_default_app_set(mime.as_ptr(), id.as_ptr()))
                                .to_string_lossy()
                                .into_owned()
                        },
                        _ => "Invalid application identifier".into(),
                    },
                };
                let loaded = unsafe { CStr::from_ptr(dome_default_apps_list()).to_bytes() };
                let mut next =
                    serde_json::from_slice::<Snapshot>(loaded).unwrap_or_else(|error| Snapshot {
                        error: error.to_string(),
                        ..Default::default()
                    });
                if !error.is_empty() {
                    next.error = error;
                }
                let mut current = shared.lock().unwrap();
                next.revision = current.revision + 1;
                *current = next;
            }
        });
        let result = Self { send, state };
        result.request(Request::Refresh);
        result
    }
    fn request(&self, request: Request) {
        let mut state = self.state.lock().unwrap();
        state.busy = true;
        state.error.clear();
        state.revision += 1;
        let _ = self.send.send(request);
    }
    pub fn state(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
    pub fn revision(&self) -> u64 {
        self.state.lock().unwrap().revision
    }
}
impl Dome {
    pub fn default_apps_open(&mut self) {
        self.close_audio();
        self.history_panel = None;
        self.menu = None;
        self.launcher = false;
        self.desktop_manager = false;
        self.task_popup = None;
        self.appearance_open = false;
        self.xr_settings_open = false;
        self.tray.close();
        self.default_apps_open = true;
        self.default_apps_row = 0;
        self.default_apps_choice = None;
        self.default_apps.request(Request::Refresh);
    }
    pub fn default_apps_json(&self, width: i32, height: i32) -> serde_json::Value {
        if !self.default_apps_open {
            return serde_json::Value::Null;
        }
        let state = self.default_apps.state();
        let rows = self.default_apps_rows(&state);
        let selected = self
            .default_apps_choice
            .unwrap_or(self.default_apps_row)
            .min(rows.len().saturating_sub(1));
        let w = 800.min((width - 24).max(1));
        let h = 650.min((height - 108).max(1));
        let visible = ((h - 152) / 44).max(1) as usize;
        let first = selected
            .saturating_sub(visible - 1)
            .min(rows.len().saturating_sub(visible));
        let title = if self.default_apps_choice.is_some() {
            state
                .rows
                .get(self.default_apps_row)
                .map(|row| format!("Choose application · {}", row.label))
                .unwrap_or_default()
        } else {
            "Default Applications".into()
        };
        serde_json::json!({"x":(width-w)/2,"y":42,"w":w,"h":h,"rows":rows,"selected":selected,"first":first,"visible":visible,"chooser":self.default_apps_choice.is_some(),"title":title,"busy":state.busy,"error":state.error})
    }
    fn default_apps_rows(&self, state: &Snapshot) -> Vec<serde_json::Value> {
        if self.default_apps_choice.is_some() {
            state.rows.get(self.default_apps_row).map(|row|row.apps.iter().map(|app|serde_json::json!({"label":app.name,"detail":app.id,"current":row.current.as_ref().is_some_and(|current|current.id==app.id)})).collect()).unwrap_or_default()
        } else {
            state.rows.iter().map(|row|serde_json::json!({"label":row.label,"detail":row.current.as_ref().map(|app|app.name.as_str()).unwrap_or("No default application"),"available":!row.apps.is_empty()})).collect()
        }
    }
    fn default_apps_activate(&mut self) {
        let state = self.default_apps.state();
        if state.busy {
            return;
        }
        let Some(row) = state.rows.get(self.default_apps_row) else {
            return;
        };
        if let Some(choice) = self.default_apps_choice {
            if let Some(app) = row.apps.get(choice) {
                self.default_apps
                    .request(Request::Set(row.mime.clone(), app.id.clone()));
                self.default_apps_choice = None;
            }
        } else if !row.apps.is_empty() {
            self.default_apps_choice = Some(
                row.current
                    .as_ref()
                    .and_then(|current| row.apps.iter().position(|app| app.id == current.id))
                    .unwrap_or(0),
            );
        }
    }
    fn default_apps_move(&mut self, delta: i32) {
        let state = self.default_apps.state();
        let count = self.default_apps_rows(&state).len();
        let selected = self
            .default_apps_choice
            .as_mut()
            .unwrap_or(&mut self.default_apps_row);
        *selected = (*selected as i32).saturating_add(delta)
            .clamp(0, count.saturating_sub(1) as i32) as usize;
    }
    pub fn default_apps_key(&mut self, key: u32) -> bool {
        if !self.default_apps_open {
            return false;
        }
        match key {
            0xff1b => {
                if self.default_apps_choice.is_some() {
                    self.default_apps_choice = None
                } else {
                    self.default_apps_open = false
                }
            }
            0xff52 => self.default_apps_move(-1),
            0xff54 => self.default_apps_move(1),
            0xff55 => self.default_apps_move(-5),
            0xff56 => self.default_apps_move(5),
            0xff50 => {
                if let Some(choice) = &mut self.default_apps_choice {
                    *choice = 0
                } else {
                    self.default_apps_row = 0
                }
            }
            0xff57 => self.default_apps_move(i32::MAX),
            0xff51 => self.default_apps_choice = None,
            0xff53 | 0xff0d | 0xff8d => self.default_apps_activate(),
            _ => (),
        }
        true
    }
    pub fn default_apps_scroll(&mut self, delta: f64) -> bool {
        if !self.default_apps_open {
            return false;
        }
        if delta != 0. {
            self.default_apps_move(if delta > 0. { 3 } else { -3 });
        }
        true
    }
    pub fn default_apps_click(&mut self, x: f64, y: f64, width: i32, height: i32) -> bool {
        if !self.default_apps_open {
            return false;
        }
        let panel = self.default_apps_json(width, height);
        let left = panel["x"].as_f64().unwrap();
        let top = 42.;
        let w = panel["w"].as_f64().unwrap();
        let h = panel["h"].as_f64().unwrap();
        if x < left || x >= left + w || y < top || y >= top + h {
            self.default_apps_open = false;
            return true;
        }
        if y >= top + h - 54. && y < top + h - 18. {
            if x < left + w / 2. {
                if self.default_apps_choice.is_some() {
                    self.default_apps_choice = None
                } else if !self.default_apps.state().busy {
                    self.default_apps.request(Request::Refresh);
                }
            } else if self.default_apps_choice.is_some() {
                self.default_apps_activate()
            } else {
                self.default_apps_open = false;
            }
        } else if y >= top + 64. && y < top + 64. + panel["visible"].as_u64().unwrap() as f64 * 44.
        {
            let index =
                panel["first"].as_u64().unwrap() as usize + ((y - top - 64.) / 44.) as usize;
            if index < panel["rows"].as_array().unwrap().len() {
                if let Some(choice) = &mut self.default_apps_choice {
                    *choice = index
                } else {
                    self.default_apps_row = index;
                    self.default_apps_activate();
                }
            }
        }
        true
    }
}
