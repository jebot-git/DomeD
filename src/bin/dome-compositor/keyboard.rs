use crate::Dome;
use smithay::input::keyboard::{Layout, XkbConfig};
impl Dome {
    pub fn keyboard_settings_open(&mut self) {
        self.keyboard_draft = Some(self.keyboard_config.clone());
        self.keyboard_field = 0;
        self.keyboard_error.clear();
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
            0xff09 => self.keyboard_field = (self.keyboard_field + 1) % 3,
            0xff0d => {
                self.keyboard_apply = true;
            }
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
            if let Some(d) = self.keyboard_draft.as_mut() {
                d.switch_shortcut = d.switch_shortcut.next();
            }
        } else if y >= top + 252. && y < top + 288. {
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
