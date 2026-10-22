//! Launcher selection, scrolling and compositor-owned cursor-key repeat.
use crate::state::Dome;
use smithay::backend::input::Keycode;
use std::time::{Duration, Instant};

pub const ROWS: usize = 8;
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_INTERVAL: Duration = Duration::from_millis(40);

#[derive(Default)]
pub struct Navigation {
    pub offset: usize,
    pub navigated: bool,
    pub error: String,
    pub repeat: Option<(Keycode, isize, Instant)>,
}
impl Navigation {
    fn reveal(&mut self, index: usize, total: usize) {
        self.offset = self.offset.min(total.saturating_sub(ROWS));
        if index < self.offset {
            self.offset = index;
        } else if index >= self.offset + ROWS {
            self.offset = index + 1 - ROWS;
        }
    }
    fn move_by(&mut self, index: &mut usize, total: usize, step: isize) {
        *index = if total == 0 {
            0
        } else {
            (*index as isize + step).rem_euclid(total as isize) as usize
        };
        self.navigated = true;
        self.error.clear();
        self.reveal(*index, total);
    }
}

impl Dome {
    pub fn open_launcher(&mut self) {
        self.clock.open = false;
        self.apps = dome::service::apps();
        self.launcher = true;
        self.launch_query.clear();
        self.launch_index = 0;
        self.launch_navigation = Navigation::default();
    }
    pub fn launcher_key(&mut self, code: Keycode, sym: u32, character: Option<char>, text: bool) {
        self.launch_navigation.repeat = None;
        match sym {
            0xff1b => self.launcher = false,
            0xff08 => {
                self.launch_query.pop();
                self.launch_index = 0;
                self.launch_navigation = Navigation::default();
            }
            0xff52 | 0xff51 | 0xff54 | 0xff53 => {
                let step = if matches!(sym, 0xff52 | 0xff51) {
                    -1
                } else {
                    1
                };
                let total = self.matching_apps().len();
                self.launch_navigation
                    .move_by(&mut self.launch_index, total, step);
                self.launch_navigation.repeat = Some((code, step, Instant::now() + REPEAT_DELAY));
            }
            0xff50 | 0xff57 => {
                let total = self.matching_apps().len();
                self.launch_index = if sym == 0xff50 {
                    0
                } else {
                    total.saturating_sub(1)
                };
                self.launch_navigation.navigated = true;
                self.launch_navigation.error.clear();
                self.launch_navigation.reveal(self.launch_index, total);
            }
            0xff0d | 0xff8d => self.launch_selected(),
            _ if text => {
                if let Some(c) = character.filter(|c| !c.is_control()) {
                    if self.launch_query.len() + c.len_utf8() <= 256 {
                        self.launch_query.push(c);
                        self.launch_index = 0;
                        self.launch_navigation = Navigation::default();
                    }
                }
            }
            _ => (),
        }
    }
    pub fn repeat_launcher_key(&mut self) {
        let Some((code, step, next)) = self.launch_navigation.repeat else {
            return;
        };
        if !self.launcher || !self.native_session_active || !self.shell_keys.contains(&code) {
            self.launch_navigation.repeat = None;
            return;
        }
        let now = Instant::now();
        if now >= next {
            let total = self.matching_apps().len();
            self.launch_navigation
                .move_by(&mut self.launch_index, total, step);
            // Do not burst through queued repeats after a slow frame or session switch.
            self.launch_navigation.repeat = Some((code, step, now + REPEAT_INTERVAL));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_navigation_reaches_every_row_and_wraps_in_both_directions() {
        let mut navigation = Navigation::default();
        let mut index = 0;
        for expected in (1..25).chain([0]) {
            navigation.move_by(&mut index, 25, 1);
            assert_eq!(index, expected);
            assert!(index >= navigation.offset && index < navigation.offset + ROWS);
        }
        navigation.move_by(&mut index, 25, -1);
        assert_eq!((index, navigation.offset), (24, 17));
        navigation.move_by(&mut index, 0, 1);
        assert_eq!((index, navigation.offset), (0, 0));
        navigation.move_by(&mut index, 1, -1);
        assert_eq!((index, navigation.offset), (0, 0));
    }
}
