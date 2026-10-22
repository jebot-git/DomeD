//! Shared selection keys for compositor-owned menus, in desktop and XR.
pub fn selection(current: usize, count: usize, key: u32) -> Option<usize> {
    let last = count.saturating_sub(1);
    let current = current.min(last);
    Some(match key {
        0xff52 | 0xfe20 => current.saturating_sub(1),
        0xff54 | 0xff09 => (current + 1).min(last),
        0xff50 => 0,
        0xff57 => last,
        0xff55 => current.saturating_sub(5),
        0xff56 => (current + 5).min(last),
        _ => return None,
    })
}

pub fn cycle<T: PartialEq + Clone>(values: &[T], current: &T, direction: i32) -> Option<T> {
    if values.is_empty() { return None; }
    let index = values.iter().position(|v| v == current).unwrap_or(0) as i32;
    Some(values[(index + direction).rem_euclid(values.len() as i32) as usize].clone())
}

impl crate::Dome {
    pub fn move_menu_selection(&mut self, key: u32) {
        let Some(menu) = self.menu else { return; };
        let enabled: Vec<_> = self.menu_items(menu).iter().enumerate()
            .filter(|(_, (_, _, command))| self.command_enabled(*command))
            .map(|(index, _)| index).collect();
        if enabled.is_empty() { self.menu_hover = -1; return; }
        let index = enabled.iter().position(|i| *i as i32 == self.menu_hover);
        let next = match key {
            0xff54 => index.map_or(0, |i| (i + 1) % enabled.len()),
            0xff52 => index.map_or(enabled.len() - 1, |i| (i + enabled.len() - 1) % enabled.len()),
            _ => selection(index.unwrap_or(0), enabled.len(), key).unwrap_or(0),
        };
        self.menu_hover = enabled[next] as i32;
    }
    pub fn shell_panel_key(&mut self, key: u32) -> bool {
        if self.clock_key(key) { return true; }
        if self.appearance_open && self.keyboard_draft.is_none() {
            if let Some(row) = selection(self.shell_selection, 9, key) {
                self.shell_selection = row;
            } else {
                match key {
                    0xff1b => self.appearance_open = false,
                    0xff51 | 0xff53 if self.shell_selection < 6 => {
                        self.appearance_activate(self.shell_selection, if key == 0xff51 { -1 } else { 1 });
                    }
                    0xff51 | 0xff53 if self.shell_selection >= 7 => {
                        self.shell_selection = if key == 0xff51 { 7 } else { 8 };
                    }
                    0xff0d | 0xff8d | 0x20 => self.appearance_activate(self.shell_selection, 1),
                    _ => (),
                }
            }
            return true;
        }
        if self.session_confirm.is_some() {
            match key {
                0xff51 | 0xff52 | 0xff50 => self.shell_selection = 0,
                0xff53 | 0xff54 | 0xff57 => self.shell_selection = 1,
                0xff09 | 0xfe20 => self.shell_selection = 1 - self.shell_selection.min(1),
                0xff1b => self.session_confirm = None,
                0xff0d | 0xff8d | 0x20 => {
                    if self.shell_selection == 1 { self.confirm_session(); }
                    else { self.session_confirm = None; }
                }
                _ => (),
            }
            return true;
        }
        if self.shell_modal_open() { return false; }
        if self.tray.key(key) { return true; }
        if self.task_context { return false; }
        if let Some(index) = self.task_popup {
            let windows = self.task_list().get(index).map(|t| t.windows.clone()).unwrap_or_default();
            if let Some(row) = selection(self.shell_selection, windows.len().min(4), key) {
                self.shell_selection = row;
            } else {
                match key {
                    0xff1b => self.task_popup = None,
                    0xff0d | 0xff8d | 0x20 => {
                        if let Some(id) = windows.get(self.shell_selection).copied() {
                            self.action(dome::Action::Focus { id });
                        }
                        self.task_popup = None;
                    }
                    _ => (),
                }
            }
            return true;
        }
        if self.desktop_manager {
            let count = self.scene.lock().unwrap().workspaces.len();
            if let Some(row) = selection(self.shell_selection, count + 1, key) {
                self.shell_selection = row;
            } else {
                match key {
                    0xff1b => self.desktop_manager = false,
                    0xff0d | 0xff8d | 0x20 => {
                        if self.shell_selection < count {
                            self.action(dome::Action::SwitchWorkspace { workspace: self.shell_selection });
                        } else {
                            self.action(dome::Action::AddWorkspace { name: format!("Desktop {}", count + 1) });
                        }
                        self.desktop_manager = false;
                    }
                    _ => (),
                }
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_recovers_stale_rows_and_bounds_empty_menus() {
        assert_eq!(selection(50, 9, 0xff52), Some(7));
        assert_eq!(selection(0, 0, 0xff54), Some(0));
        assert_eq!(selection(0, 9, 0xff57), Some(8));
        assert_eq!(selection(8, 9, 0xff54), Some(8));
        assert_eq!(selection(8, 9, 0xff50), Some(0));
        assert_eq!(selection(0, 9, 0xff0d), None);
    }
    #[test]
    fn appearance_values_cycle_in_both_directions() {
        assert_eq!(cycle(&[208, 248, 255], &208, -1), Some(255));
        assert_eq!(cycle(&[208, 248, 255], &255, 1), Some(208));
        assert_eq!(cycle::<i32>(&[], &0, 1), None);
    }
}
