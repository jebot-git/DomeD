use crate::Dome;
use dome::{geometry_settings, Action, Config};

// Painting and hit testing must share the same section-aware scroll offset.
fn settings_first(selected: usize, visible: usize, count: usize, bindings: usize, binding_count: usize) -> usize {
    let section = if (bindings..bindings + binding_count).contains(&selected) { bindings } else { 0 };
    selected.saturating_sub(visible.saturating_sub(1)).max(section)
        .min(count.saturating_sub(visible))
}

fn cycle_display_offset(current: i32, resolution: i32, direction: f32) -> i32 {
    let step = if direction > 0. { 1 } else { -1 };
    let next = (current.signum() + step + 1).rem_euclid(3) - 1;
    next * resolution
}

#[cfg(test)]
mod tests {
    use super::{cycle_display_offset, settings_first};

    #[test]
    fn keybinding_help_starts_at_the_binding_section_and_scrolls_with_selection() {
        assert_eq!(settings_first(0, 12, 90, 17, 50), 0);
        assert_eq!(settings_first(16, 12, 90, 17, 50), 5);
        assert_eq!(settings_first(17, 12, 90, 17, 50), 17);
        assert_eq!(settings_first(25, 12, 90, 17, 50), 17);
        assert_eq!(settings_first(30, 12, 90, 17, 50), 19);
        assert_eq!(settings_first(89, 12, 90, 17, 50), 78);
    }

    #[test]
    fn display_offsets_cycle_both_directions_at_each_axis_resolution() {
        for resolution in [1920, 1080, 2560, 1440] {
            for (direction, expected) in [
                (1., [resolution, -resolution, 0]),
                (-1., [-resolution, resolution, 0]),
            ] {
                let mut offset = 0;
                for next in expected {
                    offset = cycle_display_offset(offset, resolution, direction);
                    assert_eq!(offset, next);
                }
            }
        }
    }

    #[test]
    fn legacy_and_automatic_positions_enter_the_resolution_cycle() {
        assert_eq!(cycle_display_offset(100, 1920, -1.), 0);
        assert_eq!(cycle_display_offset(-100, 1080, 1.), 0);
        assert_eq!(cycle_display_offset(3840, 1920, 1.), -1920);
        assert_eq!(cycle_display_offset(-2160, 1080, -1.), 1080);
    }
}

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
        let first = settings_first(self.xr_settings_row, visible, rows.len(),
            self.keybindings_settings_start(&c), c.keybindings.len());
        serde_json::json!({"x":x,"y":y,"w":w,"h":h,"row_height":row_height,"rows":rows,"first":first,"visible":visible,"selected":self.xr_settings_row,"error":self.geometry_error})
    }
    pub fn adjust_xr_setting(&mut self, row: usize, direction: f32) {
        // These settings links use the same compositor-owned dialogs in both realms.
        if row < 2 {
            self.xr_settings_open = false;
            self.binding_capture = None;
            self.command(if row == 0 { dome::shortcuts::Command::Appearance } else { dome::shortcuts::Command::KeyboardSettings });
            return;
        }
        let row = row - 2;
        let c = self.scene.lock().unwrap().config.clone();
        let geometry_count = geometry_settings::rows(&c).len();
        if row >= geometry_count && row < geometry_count + c.keybindings.len() {
            self.binding_capture = Some(row - geometry_count);
            self.geometry_error = "Press a key chord; Backspace disables, Escape cancels".into();
            return;
        }
        let adjusted = if row < geometry_count {
            geometry_settings::adjust(&c, row, direction)
        } else {
            self.adjust_display_setting(&c, row - geometry_count - c.keybindings.len(), direction)
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
        let c = self.scene.lock().unwrap().config.clone();
        let count = self.settings_rows(&c).len();
        let visible = ((h - 150.) / row).floor().max(1.) as usize;
        let first = settings_first(self.xr_settings_row, visible, count,
            self.keybindings_settings_start(&c), c.keybindings.len());
        if y >= top + 40.
            && y < top + 40. + visible as f64 * row
            && x >= left + 12.
            && x < left + w - 12.
        {
            self.xr_settings_row = (first + ((y - top - 40.) / row) as usize).min(count - 1);
            if self.xr_settings_row < 2 || x >= left + w - 270. {
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
            0xff1b => {
                self.xr_settings_open = false;
                self.binding_capture = None;
            }
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
    pub fn keybindings_settings_start(&self, c: &Config) -> usize {
        2 + geometry_settings::rows(c).len()
    }
    fn settings_rows(&self, c: &Config) -> Vec<(String, String)> {
        let mut rows = vec![("Appearance…".to_owned(), "Open".to_owned()),
            ("Keyboard Layouts…".to_owned(), self.keyboard_config.label())];
        rows.extend(geometry_settings::rows(c)
            .into_iter()
            .map(|(a, b)| (a.to_string(), b)));
        for (index, binding) in c.keybindings.iter().enumerate() {
            rows.push((
                format!(
                    "Keybinding · {}",
                    dome::shortcuts::action_label(binding.command)
                ),
                if self.binding_capture == Some(index) {
                    "Press chord…".into()
                } else {
                    binding.label()
                },
            ));
        }
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
        let profile = &c.displays[index];
        let mode = output.and_then(|o| o.current_mode()).or_else(|| {
            modes
                .iter()
                .find(|m| {
                    m.size.w == profile.width
                        && m.size.h == profile.height
                        && (profile.refresh == 0 || m.refresh == profile.refresh)
                })
                .or_else(|| modes.first())
                .copied()
        });
        let pos = output
            .and_then(|o| self.space.output_geometry(o))
            .map(|g| g.loc)
            .unwrap_or_else(|| (profile.x.unwrap_or(0), profile.y).into());
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
                if d.x.is_some() {
                    d.x = Some(pos.x.signum() * m.size.w);
                    d.y = pos.y.signum() * m.size.h;
                }
            }
            2 => {
                let mode = mode.ok_or("No display modes")?;
                c.displays[index].x = Some(cycle_display_offset(pos.x, mode.size.w, direction));
            }
            3 => {
                let mode = mode.ok_or("No display modes")?;
                c.displays[index].x = Some(pos.x);
                c.displays[index].y = cycle_display_offset(pos.y, mode.size.h, direction);
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

impl Dome {
    pub fn resolve_shortcut(
        &self,
        key: u32,
        mods: dome::shortcuts::Mods,
    ) -> Option<dome::shortcuts::Command> {
        let scene = self.scene.lock().unwrap();
        dome::shortcuts::resolve_with(&scene.config.keybindings, key, mods).filter(|command| {
            !matches!(command, dome::shortcuts::Command::CycleLane(_))
                || dome::input_routing::lane_navigation_available(&scene, self.input_realm)
        })
    }
    pub fn capture_binding(&mut self, key: u32, mods: dome::shortcuts::Mods) -> bool {
        let Some(index) = self.binding_capture.filter(|_| self.xr_settings_open) else {
            self.binding_capture = None;
            return false;
        };
        if key == 0xff1b {
            self.binding_capture = None;
            self.geometry_error.clear();
            return true;
        }
        if (0xffe1..=0xffee).contains(&key) {
            return true;
        }
        if key == 0x20
            && self
                .keyboard_config
                .switch_shortcut
                .matches(mods.logo, mods.ctrl, mods.alt, mods.shift)
        {
            self.geometry_error =
                "That chord switches keyboard layouts; choose another chord".into();
            return true;
        }
        let mut config = self.scene.lock().unwrap().config.clone();
        let Some(binding) = config.keybindings.get_mut(index) else {
            self.binding_capture = None;
            return true;
        };
        binding.enabled = key != 0xff08;
        if binding.enabled {
            binding.key = if key == 0xfe20 {
                0xff09
            } else {
                char::from_u32(key)
                    .map(|c| c.to_ascii_lowercase() as u32)
                    .unwrap_or(key)
            };
            binding.mods = mods;
        }
        match config.validate() {
            Ok(()) => {
                self.geometry_error.clear();
                self.action(Action::Configure { config });
                self.persist_geometry();
                self.binding_capture = None;
            }
            Err(error) => self.geometry_error = error,
        }
        true
    }
    pub fn toggle_desktop_xr(&mut self) {
        let visible = self.xr_bridge.lock().unwrap().visible();
        if !visible {
            self.action(Action::XrPresentation {
                top: Some(true),
                bottom: Some(true),
                desktop: Some(true),
            });
            self.start_xr();
        } else {
            self.command(dome::shortcuts::Command::ToggleXrDesktop);
        }
    }
}
