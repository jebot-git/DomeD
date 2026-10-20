use crate::Dome;
use smithay::input::keyboard::{Layout, XkbConfig};
impl Dome {
    pub fn keyboard_settings_open(&mut self) {
        self.keyboard_draft = Some(self.keyboard_config.clone());
        self.keyboard_field = 0;
        self.keyboard_error.clear();
        self.default_apps_open = false;
        self.appearance_open = true;
    }
    pub fn keyboard_settings_key(&mut self, sym: u32, character: Option<char>, ctrl: bool) -> bool {
        let Some(draft) = self
            .keyboard_draft
            .as_mut()
            .filter(|_| self.appearance_open)
        else {
            return false;
        };
        match sym {
            0xff1b => {
                self.keyboard_draft = None;
                self.appearance_open = false;
            }
            0xff09 => self.keyboard_field = (self.keyboard_field + 1) % 7,
            0xfe20 | 0xff52 => self.keyboard_field = self.keyboard_field.saturating_sub(1),
            0xff54 => self.keyboard_field = (self.keyboard_field + 1).min(6),
            0xff50 => self.keyboard_field = 0,
            0xff57 => self.keyboard_field = 6,
            0xff51 | 0xff53 | 0xff0d | 0xff8d | 0x20 if self.keyboard_field >= 3 => {
                match self.keyboard_field {
                    3 => {
                        let steps = if sym == 0xff51 { 2 } else { 1 };
                        for _ in 0..steps { draft.switch_shortcut = draft.switch_shortcut.next(); }
                    }
                    4 => draft.ime = !draft.ime,
                    5 | 6 => {
                        if sym == 0xff51 || sym == 0xff53 {
                            self.keyboard_field = if sym == 0xff51 { 5 } else { 6 };
                        } else if self.keyboard_field == 5 {
                            self.keyboard_apply = true;
                        } else {
                            self.keyboard_draft = None;
                            self.appearance_open = false;
                        }
                    }
                    _ => (),
                }
            }
            0xff0d | 0xff8d => self.keyboard_apply = true,
            _ if self.keyboard_field >= 3 => (),
            _ => {
                let field = match self.keyboard_field {
                    0 => &mut draft.layout,
                    1 => &mut draft.variant,
                    _ => &mut draft.options,
                };
                if ctrl && character.is_some_and(|c| c == 'a' || c == 'A') {
                    field.clear();
                } else if sym == 0xff08 {
                    field.pop();
                } else if let Some(c) =
                    character.filter(|c| c.is_ascii_alphanumeric() || "_,:-".contains(*c))
                {
                    if field.len() < 256 {
                        field.push(c);
                    }
                }
            }
        }
        true
    }
    pub fn keyboard_settings_click(&mut self, x: f64, y: f64, width: i32, height: i32) {
        let left = (width as f64 - 600.) / 2.;
        let top = (height as f64 - 400.) / 2.;
        if x < left || x >= left + 600. || y < top || y >= top + 400. {
            return;
        }
        let row = ((y - top - 60.) / 48.).floor() as i32;
        if y >= top + 60. && y < top + 204. && x >= left + 180. {
            self.keyboard_field = row as usize;
        } else if row == 3 && x >= left + 180. {
            self.keyboard_field = 3;
            if let Some(d) = self.keyboard_draft.as_mut() {
                d.switch_shortcut = d.switch_shortcut.next();
            }
        } else if y >= top + 252. && y < top + 288. {
            self.keyboard_field = 4;
            if let Some(d) = self.keyboard_draft.as_mut() {
                d.ime = !d.ime;
            }
        } else if y >= top + 340. {
            if x < left + 300. {
                self.keyboard_apply = true;
            } else {
                self.keyboard_draft = None;
                self.appearance_open = false;
            }
        }
    }
    /// Called outside Smithay's input filter: re-entering its keyboard lock there deadlocks.
    pub fn update_keyboard(&mut self) {
        let keyboard = self.seat.get_keyboard().unwrap();
        if self.keyboard_apply {
            self.keyboard_apply = false;
            if let Some(mut draft) = self.keyboard_draft.clone() {
                draft.active = draft
                    .active
                    .min(draft.layout.split(',').count().saturating_sub(1));
                let previous_modifiers = keyboard.modifier_state();
                let result = draft.validate().and_then(|_| {
                    keyboard
                        .set_xkb_config(
                            self,
                            XkbConfig {
                                layout: &draft.layout,
                                variant: &draft.variant,
                                options: Some(draft.options.clone()),
                                ..Default::default()
                            },
                        )
                        .map_err(|e| format!("Invalid XKB configuration: {e}"))
                });
                match result {
                    Ok(()) => {
                        let mut modifiers = keyboard.modifier_state();
                        modifiers.num_lock = previous_modifiers.num_lock;
                        modifiers.caps_lock = previous_modifiers.caps_lock;
                        keyboard.set_modifier_state(modifiers);
                        #[cfg(feature = "standalone")]
                        self.update_keyboard_leds(keyboard.led_state());
                        if let Some(focus) = keyboard.current_focus() {
                            use smithay::input::keyboard::KeyboardTarget;
                            let seat = self.seat.clone();
                            focus.modifiers(
                                &seat,
                                self,
                                keyboard.modifier_state(),
                                smithay::utils::SERIAL_COUNTER.next_serial(),
                            );
                        }
                        keyboard.with_xkb_state(self, |mut xkb| {
                            xkb.set_layout(Layout(draft.active as u32))
                        });
                        self.keyboard_config = draft;
                        self.keyboard_error = self.keyboard_config.save().err().unwrap_or_default();
                        if self.keyboard_error.is_empty() {
                            self.keyboard_draft = None;
                            self.appearance_open = false;
                        }
                    }
                    Err(e) => self.keyboard_error = e,
                }
            }
        }
        let change = std::mem::take(&mut self.keyboard_switch);
        if change != 0 {
            keyboard.with_xkb_state(self, |mut xkb| {
                for _ in 0..change.abs() {
                    if change > 0 {
                        xkb.cycle_next_layout()
                    } else {
                        xkb.cycle_prev_layout()
                    }
                }
            });
        }
        let active = keyboard.with_xkb_state(self, |xkb| {
            xkb.xkb().lock().unwrap().active_layout().0 as usize
        });
        if self.keyboard_config.active != active {
            self.keyboard_config.active = active;
            if let Err(e) = self.keyboard_config.save() {
                self.keyboard_error = e;
            }
        }
    }
}

/// Preserve the kernel LED state when the desktop preference is “leave unchanged”.
pub fn existing_numlock() -> Option<bool> {
    std::fs::read_dir("/sys/class/leds")
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with("::numlock"))
        .find_map(|e| {
            std::fs::read_to_string(e.path().join("brightness"))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .map(|n| n != 0)
        })
}
#[cfg(feature = "standalone")]
impl Dome {
    pub fn update_keyboard_leds(&mut self, state: smithay::input::keyboard::LedState) {
        use smithay::reexports::input::Led;
        let mut leds = Led::empty();
        if state.num == Some(true) {
            leds |= Led::NUMLOCK;
        }
        if state.caps == Some(true) {
            leds |= Led::CAPSLOCK;
        }
        if state.scroll == Some(true) {
            leds |= Led::SCROLLLOCK;
        }
        for device in &mut self.keyboard_devices {
            device.led_update(leds);
        }
    }
}
