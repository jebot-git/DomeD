use crate::Dome;
use dome::{geometry_settings, Action, Config};

// Painting and hit testing must share the same section-aware scroll offset.
fn settings_first(selected: usize, visible: usize, count: usize, bindings: usize, binding_count: usize) -> usize {
    let section = if (bindings..bindings + binding_count).contains(&selected) { bindings } else { 0 };
    selected.saturating_sub(visible.saturating_sub(1)).max(section)
        .min(count.saturating_sub(visible))
}

fn display_settings_rows(displays: &std::collections::BTreeMap<String, Vec<smithay::output::Mode>>,
    space: &smithay::desktop::Space<smithay::desktop::Window>, c: &Config) -> Vec<(String,String)> {
    let mut rows = Vec::new();
    for name in displays.keys() {
        let profile = c.displays.iter().find(|d| d.name == *name);
        let output = space.outputs().find(|o| o.name() == *name);
        let mode = output.and_then(|o| o.current_mode()).or_else(|| {
            let profile = profile?;
            displays[name].iter().find(|mode| mode.size.w == profile.width
                && mode.size.h == profile.height && (profile.refresh == 0 || profile.refresh == mode.refresh)).copied()
        });
        let pos = output
            .and_then(|o| space.output_geometry(o))
            .map(|g| g.loc)
            .unwrap_or_else(|| (profile.and_then(|d| d.x).unwrap_or(0), profile.map_or(0, |d| d.y)).into());
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

fn settings_next(selected: usize, delta: isize, count: usize) -> usize {
    if count == 0 { 0 } else { (selected.min(count - 1) as isize + delta).rem_euclid(count as isize) as usize }
}

fn cycle_display_offset(current: i32, resolution: i32, direction: f32) -> i32 {
    let step = if direction > 0. { 1 } else { -1 };
    let next = (current.signum() + step + 1).rem_euclid(3) - 1;
    next * resolution
}

#[cfg(test)]
mod tests {
    use super::{cycle_display_offset, settings_first, settings_next, display_settings_rows};

    #[test]
    fn disabled_monitors_keep_saved_modes_and_positions_in_settings() {
        let mut config = dome::Config::default();
        config.displays.push(dome::DisplayConfig { name: "DisplayPort-1".into(), enabled: false,
            width: 1920, height: 1080, refresh: 60000, x: Some(-1920), y: -1080, primary: true });
        let modes = std::collections::BTreeMap::from([("DisplayPort-1".into(), vec![
            smithay::output::Mode { size: (1920,1080).into(), refresh: 60000 }])]);
        let rows = display_settings_rows(&modes, &smithay::desktop::Space::default(), &config);
        assert_eq!(rows.len(),6);
        assert_eq!(rows[0].1,"No");
        assert_eq!(rows[1].1,"1920×1080 60.00Hz");
        assert_eq!(rows[2].1,"-1920 px");
        assert_eq!(rows[3].1,"-1080 px");
        assert_eq!(rows[4].1,"No");
        assert_eq!(rows[5].1,"Yes");
    }

    #[test]
    fn settings_navigation_wraps_and_keeps_the_destination_visible() {
        for count in [1, 16, 100] {
            assert_eq!(settings_next(0, -1, count), count - 1);
            assert_eq!(settings_next(count - 1, 1, count), 0);
            for delta in [-8, -3, 3, 8] {
                let selected = settings_next(count - 1, delta, count);
                let first = settings_first(selected, 12, count, 30, 50);
                assert!(first <= selected && selected < first + 12);
            }
        }
        assert_eq!(settings_next(0, -1, 0), 0);
        assert_eq!(settings_next(150, 1, 100), 0); // monitor removed while Settings is open
        assert_eq!(settings_next(150, -1, 100), 98);
    }

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
        let h = 700f64.min((height - 100).max(1) as f64);
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
        let selected = self.xr_settings_row.min(rows.len().saturating_sub(1));
        let first = settings_first(selected, visible, rows.len(),
            self.keybindings_settings_start(&c), c.keybindings.len());
        serde_json::json!({"x":x,"y":y,"w":w,"h":h,"row_height":row_height,"rows":rows,"first":first,"visible":visible,"selected":selected,"error":self.geometry_error,"wrap":true})
    }
    pub fn adjust_xr_setting(&mut self, row: usize, direction: f32) {
        // These settings links use the same compositor-owned dialogs in both realms.
        if row < 2 {
            self.xr_settings_open = false;
            self.binding_capture = None;
            self.command(if row == 0 { dome::shortcuts::Command::Appearance } else { dome::shortcuts::Command::KeyboardSettings });
            return;
        }
        let c = self.scene.lock().unwrap().config.clone();
        let display_count = self.display_modes.len() * 6;
        let geometry_start = 3 + display_count;
        let binding_start = self.keybindings_settings_start(&c);
        if (binding_start..binding_start + c.keybindings.len()).contains(&row) {
            self.binding_capture = Some(row - binding_start);
            self.geometry_error = "Press a key chord; Backspace disables, Escape cancels".into();
            return;
        }
        let adjusted = if row == 2 {
            let mut config = c.clone();
            config.gpu_device = dome::gpu_settings::cycle(c.gpu_device.as_deref(), &self.gpu_choices, direction as i32);
            config.validate().map(|_| config)
        } else if row < geometry_start {
            self.adjust_display_setting(&c, row - 3, direction)
        } else {
            geometry_settings::adjust(&c, row - geometry_start, direction)
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
        self.xr_settings_row = self.xr_settings_row.min(last);
        match key {
            0xff1b => {
                self.xr_settings_open = false;
                self.binding_capture = None;
            }
            0xff52 => self.xr_settings_row = settings_next(self.xr_settings_row, -1, last + 1),
            0xff54 => self.xr_settings_row = settings_next(self.xr_settings_row, 1, last + 1),
            0xff51 => self.adjust_xr_setting(self.xr_settings_row, -1.),
            0xff53 | 0xff0d => self.adjust_xr_setting(self.xr_settings_row, 1.),
            0xff55 => self.xr_settings_row = settings_next(self.xr_settings_row, -8, last + 1),
            0xff56 => self.xr_settings_row = settings_next(self.xr_settings_row, 8, last + 1),
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
            self.xr_settings_row = settings_next(self.xr_settings_row, 3, last + 1);
        } else if delta < 0. {
            self.xr_settings_row = settings_next(self.xr_settings_row, -3, last + 1);
        }
        true
    }
    pub fn keybindings_settings_start(&self, c: &Config) -> usize {
        3 + self.display_modes.len() * 6 + geometry_settings::rows(c).len()
    }
    fn settings_rows(&self, c: &Config) -> Vec<(String, String)> {
        let mut rows = vec![("Appearance…".to_owned(), "Open".to_owned()),
            ("Keyboard Layouts…".to_owned(), self.keyboard_config.label())];
        rows.push(("Compositor GPU · next session".into(),
            dome::gpu_settings::label(c.gpu_device.as_deref(), self.active_gpu_device.as_deref(), &self.gpu_choices)));
        rows.extend(display_settings_rows(&self.display_modes, &self.space, c));
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
