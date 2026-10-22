//! One clock popup, input policy and date link for both desktop and XR.
use crate::Dome;
impl Dome {
    pub fn clock_bar_width(&self) -> i32 {
        self.clock.bar_width(self.appearance.font_size)
    }
    pub fn clock_reserve(&self) -> i32 {
        self.clock_bar_width() + 44
    }
    pub fn clock_json(&self, width: i32, height: i32) -> serde_json::Value {
        let mut value = self
            .clock
            .json(width, height, dome::clock::mail_program().is_some());
        value["width"] = self.clock_bar_width().into();
        value
    }

    pub fn toggle_clock(&mut self) {
        self.clock.toggle();
        self.close_audio();
        self.history_panel = None;
        self.menu = None;
        self.launcher = false;
        self.desktop_manager = false;
        self.task_popup = None;
        self.default_apps_open = false;
        self.appearance_open = false;
        self.xr_settings_open = false;
        self.binding_capture = None;
        self.tray.close();
        self.dismiss_wayland_popups();
    }
    fn clock_launch(&mut self) {
        let result = match dome::clock::mail_program() {
            Some(path) => {
                dome::clock::launch_date(&path, self.clock.selected, &self.launch_socket())
            }
            None => Err("Install Penguin Mail to open this date in its calendar.".into()),
        };
        match result {
            Ok(()) => self.clock.open = false,
            Err(e) => self.clock.error = e,
        }
    }
    fn clock_option(&mut self, row: usize) {
        self.clock.settings.toggle(row);
        self.clock.error = self.clock.settings.save().err().unwrap_or_default();
    }
    pub fn clock_key(&mut self, key: u32) -> bool {
        if !self.clock.open {
            return false;
        }
        match key {
            0xff1b => self.clock.open = false,
            0xff55 => self.clock.move_month(-1),
            0xff56 => self.clock.move_month(1),
            0xff50 => self.clock.today(),
            0xff09 | 0xfe20 => {
                self.clock.settings_open = true;
                self.clock.settings_row = (self.clock.settings_row + 1) % 4;
            }
            0xff52 | 0xff54 if self.clock.settings_open => {
                self.clock.settings_row =
                    (self.clock.settings_row + if key == 0xff52 { 3 } else { 1 }) % 4;
            }
            0xff51 | 0xff53 if self.clock.settings_open => {
                self.clock_option(self.clock.settings_row)
            }
            0xff51 => self.clock.move_day(-1),
            0xff53 => self.clock.move_day(1),
            0xff52 => self.clock.move_day(-7),
            0xff54 => self.clock.move_day(7),
            0xff0d | 0xff8d | 0x20 => {
                if self.clock.settings_open {
                    self.clock_option(self.clock.settings_row)
                } else {
                    self.clock_launch()
                }
            }
            _ => (),
        }
        true
    }
    pub fn clock_click(&mut self, x: f64, y: f64, width: i32, height: i32) -> bool {
        if !self.clock.open {
            return false;
        }
        let [left, top, w, h] =
            dome::clock::Calendar::rect(width, height, self.clock.settings_open);
        if x < left || x >= left + w || y < top || y >= top + h {
            self.clock.open = false;
            return true;
        }
        let (x, y) = (x - left, y - top);
        if y < 42. {
            if x < 48. {
                self.clock.move_month(-1);
            } else if x >= w - 48. {
                self.clock.move_month(1);
            } else {
                self.clock.today();
            }
            return true;
        }
        if (72. ..282.).contains(&y) {
            let col = ((x - 14.) / ((w - 28.) / 7.)).floor() as i32;
            let row = ((y - 72.) / 35.).floor() as usize;
            if x >= 14. && x < w - 14. && (0..7).contains(&col) && row < 6 {
                if let Some(date) = self.clock.days().get(row * 7 + col as usize).copied() {
                    self.clock.selected = date;
                    self.clock_launch();
                }
            }
        } else if (290. ..324.).contains(&y) {
            self.clock_launch();
        } else if (330. ..362.).contains(&y) {
            self.clock.settings_open = !self.clock.settings_open;
            self.clock.settings_row = 0;
        } else if self.clock.settings_open && (370. ..498.).contains(&y) {
            let row = ((y - 370.) / 32.) as usize;
            if row < 4 {
                self.clock.settings_row = row;
                self.clock_option(row);
            }
        }
        true
    }
}
