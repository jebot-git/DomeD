use crate::Dome;
use dome::{geometry_settings, Action, Config};
impl Dome {
    pub fn persist_geometry(&mut self) {
        let config = self.scene.lock().unwrap().config.clone();
        if config != self.geometry_saved {
            self.geometry_error = geometry_settings::save(&geometry_settings::path(), &config)
                .err()
                .unwrap_or_default();
            self.geometry_saved = config;
        }
    }
    pub fn xr_settings_rect(&self, width: i32, height: i32) -> [f64; 5] {
        let w = 800f64.min((width - 16).max(1) as f64);
        let h = 650f64.min((height - 100).max(1) as f64);
        [
            (width as f64 - w) / 2.,
            36. + ((height - 96) as f64 - h).max(0.) / 2.,
            w,
            h,
            34.,
        ]
    }
    pub fn xr_settings_json(&self, width: i32, height: i32) -> serde_json::Value {
        if !self.xr_settings_open {
            return serde_json::Value::Null;
        }
        let c = self.scene.lock().unwrap().config.clone();
        let rows: Vec<_> = self
            .settings_rows(&c)
            .into_iter()
            .map(|(label, value)| serde_json::json!({"label":label,"value":value}))
            .collect();
        let [x, y, w, h, row_height] = self.xr_settings_rect(width, height);
        let visible = ((h - 150.) / row_height).floor().max(1.) as usize;
        let first = self
            .xr_settings_row
            .saturating_sub(visible - 1)
            .min(rows.len().saturating_sub(visible));
        serde_json::json!({"x":x,"y":y,"w":w,"h":h,"row_height":row_height,"rows":rows,"first":first,"visible":visible,"selected":self.xr_settings_row,"error":self.geometry_error})
    }
    pub fn adjust_xr_setting(&mut self, row: usize, direction: f32) {
        let c = self.scene.lock().unwrap().config.clone();
        let adjusted = if row < geometry_settings::rows(&c).len() {
            geometry_settings::adjust(&c, row, direction)
        } else {
            self.adjust_display_setting(&c, row - geometry_settings::rows(&c).len(), direction)
        };
        match adjusted {
            Ok(config) => {
                self.geometry_error.clear();
                self.action(Action::Configure { config });
                self.persist_geometry();
            }
            Err(e) => self.geometry_error = e,
        }
    }
    pub fn anchor_here(&mut self) {
        let bridge = self.xr_bridge.lock().unwrap();
        if !bridge.visible() {
            drop(bridge);
            self.geometry_error =
                "Start XR with a tracked headset before anchoring at the headset".into();
            return;
        }
        let head = bridge.session.head;
        drop(bridge);
        let c = self.scene.lock().unwrap().config.clone();
        match geometry_settings::anchor_here(&c, head) {
            Ok(config) => {
                self.action(Action::Configure { config });
                self.persist_geometry();
            }
            Err(e) => self.geometry_error = e,
        }
    }
    pub fn xr_settings_click(&mut self, x: f64, y: f64, width: i32, height: i32) {
        let [left, top, w, h, row] = self.xr_settings_rect(width, height);
        let count = self.settings_rows(&self.scene.lock().unwrap().config).len();
        let visible = ((h - 150.) / row).floor().max(1.) as usize;
        let first = self
            .xr_settings_row
            .saturating_sub(visible - 1)
            .min(count.saturating_sub(visible));
        if y >= top + 40.
            && y < top + 40. + visible as f64 * row
            && x >= left + 12.
            && x < left + w - 12.
        {
            self.xr_settings_row = (first + ((y - top - 40.) / row) as usize).min(count - 1);
            if x >= left + w - 270. {
                self.adjust_xr_setting(
                    self.xr_settings_row,
                    if x < left + w - 200. { -1. } else { 1. },
                );
            }
        } else if y >= top + 40. + visible as f64 * row && y < top + h - 70. {
            self.xr_settings_scroll(if x < left + w / 2. { -15. } else { 15. });
        } else if y >= top + h - 66. && y < top + h - 30. {
            if x >= left + 20. && x < left + 260. {
                self.anchor_here();
            } else if x >= left + 280. && x < left + 520. {
                self.action(Action::Configure {
                    config: Config::default(),
                });
                self.persist_geometry();
            } else if x >= left + w - 140. && x < left + w - 20. {
                self.xr_settings_open = false;
            }
        }
    }
    pub fn xr_settings_key(&mut self, key: u32) -> bool {
        if !self.xr_settings_open {
            return false;
        }
        let last = self
            .settings_rows(&self.scene.lock().unwrap().config)
            .len()
            .saturating_sub(1);
        match key {
            0xff1b => self.xr_settings_open = false,
            0xff52 => self.xr_settings_row = self.xr_settings_row.saturating_sub(1),
            0xff54 => self.xr_settings_row = (self.xr_settings_row + 1).min(last),
            0xff51 => self.adjust_xr_setting(self.xr_settings_row, -1.),
            0xff53 | 0xff0d => self.adjust_xr_setting(self.xr_settings_row, 1.),
            0xff55 => self.xr_settings_row = self.xr_settings_row.saturating_sub(8),
            0xff56 => self.xr_settings_row = (self.xr_settings_row + 8).min(last),
            0xff50 => self.xr_settings_row = 0,
            0xff57 => self.xr_settings_row = last,
            _ => (),
        }
        true
    }
}

impl Dome {
    pub fn xr_settings_scroll(&mut self, delta: f64) -> bool {
        if !self.xr_settings_open {
            return false;
        }
        let last = self
            .settings_rows(&self.scene.lock().unwrap().config)
            .len()
            .saturating_sub(1);
        if delta > 0. {
            self.xr_settings_row = (self.xr_settings_row + 3).min(last);
        } else if delta < 0. {
            self.xr_settings_row = self.xr_settings_row.saturating_sub(3);
        }
        true
    }
    fn settings_rows(&self, c: &Config) -> Vec<(String, String)> {
        let mut rows: Vec<_> = geometry_settings::rows(c)
            .into_iter()
            .map(|(a, b)| (a.to_string(), b))
            .collect();
        for name in self.display_modes.keys() {
            let profile = c.displays.iter().find(|d| d.name == *name);
            let output = self.space.outputs().find(|o| o.name() == *name);
            let mode = output.and_then(|o| o.current_mode());
            let pos = output
                .and_then(|o| self.space.output_geometry(o))
                .map(|g| g.loc)
                .unwrap_or_default();
            for (label, value) in [
                (
                    "Enabled",
                    if profile.is_none_or(|d| d.enabled) {
                        "Yes".into()
                    } else {
                        "No".into()
                    },
                ),
                (
                    "Resolution / refresh",
                    mode.map(|m| {
                        format!(
                            "{}×{} {:.2}Hz",
                            m.size.w,
                            m.size.h,
                            m.refresh as f64 / 1000.
                        )
                    })
                    .unwrap_or_else(|| "Disabled".into()),
                ),
                ("Position X", format!("{} px", pos.x)),
                ("Position Y", format!("{} px", pos.y)),
                (
                    "Automatic arrangement",
                    if profile.is_none_or(|d| d.x.is_none()) {
                        "Yes".into()
                    } else {
                        "No".into()
                    },
                ),
                (
                    "Primary display",
                    if profile.is_some_and(|d| d.primary) {
                        "Yes".into()
                    } else {
                        "No".into()
                    },
                ),
            ] {
                rows.push((format!("{name} · {label}"), value));
            }
        }
        rows
    }
    fn adjust_display_setting(
        &self,
        c: &Config,
        row: usize,
        direction: f32,
    ) -> Result<Config, String> {
        let (name, modes) = self
            .display_modes
            .iter()
            .nth(row / 6)
            .ok_or("Unknown display")?;
        let mut c = c.clone();
        let index = if let Some(i) = c.displays.iter().position(|d| d.name == *name) {
            i
        } else {
            c.displays.push(dome::DisplayConfig {
                name: name.clone(),
                ..Default::default()
            });
            c.displays.len() - 1
        };
        let output = self.space.outputs().find(|o| o.name() == *name);
        let pos = output
            .and_then(|o| self.space.output_geometry(o))
            .map(|g| g.loc)
            .unwrap_or_default();
        match row % 6 {
            0 => {
                if c.displays[index].enabled && self.space.outputs().count() <= 1 {
                    return Err("Keep at least one display enabled".into());
                }
                c.displays[index].enabled = !c.displays[index].enabled;
            }
            1 => {
                if modes.is_empty() {
                    return Err("No display modes".into());
                }
                let mode = output.and_then(|o| o.current_mode());
                let current = mode
                    .and_then(|m| modes.iter().position(|v| *v == m))
                    .unwrap_or(0);
                let next = (current as isize + if direction > 0. { 1 } else { -1 })
                    .rem_euclid(modes.len() as isize) as usize;
                let m = modes[next];
                let d = &mut c.displays[index];
                d.width = m.size.w;
                d.height = m.size.h;
                d.refresh = m.refresh;
            }
            2 => c.displays[index].x = Some(pos.x + (direction * 100.) as i32),
            3 => {
                c.displays[index].x = Some(pos.x);
                c.displays[index].y = pos.y + (direction * 100.) as i32;
            }
            4 => {
                let d = &mut c.displays[index];
                d.x = if d.x.is_some() { None } else { Some(pos.x) };
                d.y = 0;
            }
            5 => {
                for d in &mut c.displays {
                    d.primary = false;
                }
                c.displays[index].primary = true;
            }
            _ => unreachable!(),
        }
        c.validate()?;
        Ok(c)
    }
}
