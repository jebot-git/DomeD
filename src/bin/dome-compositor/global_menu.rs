//! One shared layout for menu painting, mouse, keyboard and XR hit testing.
use crate::{appmenu::Node, Dome};
use serde::Serialize;
#[derive(Serialize)]
pub struct Header {
    pub text: String,
    pub x: i32,
    pub width: i32,
    pub enabled: bool,
}
#[derive(Serialize)]
pub struct Panel {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub rows: Vec<Node>,
    pub selected: i32,
}
fn row_selection(rows: &[Node], current: i32, key: u32) -> Option<i32> {
    let enabled: Vec<_> = rows.iter().enumerate().filter(|(_, row)| row.enabled && !row.separator).map(|(i, _)| i as i32).collect();
    if enabled.is_empty() { return None; }
    let current = enabled.iter().position(|i| *i == current);
    let last = enabled.len() - 1;
    let index = match key {
        0xff50 => 0,
        0xff57 => last,
        0xff52 | 0xfe20 => current.map_or(last, |i| (i + last) % enabled.len()),
        0xff54 | 0xff09 => current.map_or(0, |i| (i + 1) % enabled.len()),
        0xff55 => current.unwrap_or(0).saturating_sub(5),
        0xff56 => (current.unwrap_or(0) + 5).min(last),
        _ => return None,
    };
    Some(enabled[index])
}
impl Dome {
    pub fn menu_headers(&self) -> Vec<Header> {
        let scene = self.shell_scene();
        let focused = scene.focused.and_then(|id| scene.window(id));
        let entry = focused.and_then(|w| self.apps.iter().find(|a| a.matches(&w.app_id)));
        let name = entry
            .map(|a| a.name.clone())
            .or_else(|| focused.map(|w| w.app_id.clone()))
            .unwrap_or("Desktop".into());
        let mut headers = vec![
            Header {
                text: "◉".into(),
                x: 0,
                width: 46,
                enabled: true,
            },
            Header {
                text: name,
                x: 46,
                width: 184,
                enabled: true,
            },
        ];
        let mut x = 230;
        if focused.is_some() {
            for node in self.appmenu_snapshot().root.children {
                if node.separator {
                    continue;
                }
                let width = (node.text.chars().count() as i32 * 8 + 24).clamp(48, 200);
                headers.push(Header {
                    text: node.text,
                    x,
                    width,
                    enabled: node.enabled,
                });
                x += width;
            }
        } else {
            for (text, width) in [
                ("File", 50),
                ("Edit", 52),
                ("View", 58),
                ("Window", 80),
                ("Help", 58),
            ] {
                headers.push(Header {
                    text: text.into(),
                    x,
                    width,
                    enabled: true,
                });
                x += width;
            }
        }
        headers
    }
    pub fn menu_x(&self, index: usize) -> f64 {
        self.menu_headers().get(index).map_or(0., |h| h.x as f64)
    }
    pub fn exported_menu(&self, index: usize) -> bool {
        index >= 2 && self.shell_scene().focused.is_some()
    }
    pub fn open_menu(&mut self, index: Option<usize>) {
        // A focus action can precede the next render tick. Consume it before
        // opening, so that tick cannot immediately dismiss the new menu.
        self.update_appmenu();
        if index.is_some() && self.spatial_popups.iter().any(|(_,p)| matches!(p, smithay::desktop::PopupKind::Xdg(_))) {
            self.dismiss_wayland_popups();
        }
        self.menu = index;
        self.menu_hover = -1;
        self.appmenu_path.clear();
        if let Some(index) = index.filter(|i| self.exported_menu(*i)) {
            let snapshot = self.appmenu_snapshot();
            if let (Some(address), Some(node)) = (
                snapshot.address,
                snapshot
                    .root
                    .children
                    .iter()
                    .filter(|n| !n.separator)
                    .nth(index - 2),
            ) {
                self.appmenus.open(address, node.id);
                if !node.submenu && node.children.is_empty() {
                    self.appmenus
                        .activate(self.focused_menu_address().unwrap(), node.id);
                    self.menu = None;
                }
            }
        }
    }
    pub fn switch_menu(&mut self, direction: i32) {
        let headers = self.menu_headers();
        let Some(current) = self.menu else { return; };
        for step in 1..=headers.len() {
            let next = (current as i32 + direction * step as i32).rem_euclid(headers.len() as i32) as usize;
            if headers[next].enabled {
                self.open_menu(Some(next));
                if self.exported_menu(next) {
                    let size = self.shell_geometry().size;
                    self.menu_hover = self.appmenu_panels(size.w, size.h).first()
                        .and_then(|p| row_selection(&p.rows, -1, 0xff50)).unwrap_or(-1);
                } else {
                    self.move_menu_selection(0xff50);
                }
                return;
            }
        }
    }
    pub fn appmenu_panels(&self, width: i32, height: i32) -> Vec<Panel> {
        let Some(index) = self.menu.filter(|i| self.exported_menu(*i)) else {
            return vec![];
        };
        let snapshot = self.appmenu_snapshot();
        let Some(root) = snapshot
            .root
            .children
            .iter()
            .filter(|n| !n.separator)
            .nth(index - 2)
        else {
            return vec![];
        };
        let pw = 380.min(width.max(1));
        let mut x = (self.menu_x(index) as i32).clamp(0, (width - pw).max(0));
        let mut y = 36;
        let mut node = root;
        let mut panels = Vec::new();
        for level in 0..=self.appmenu_path.len() {
            let next = self.appmenu_path.get(level);
            let selected = next
                .and_then(|id| node.children.iter().position(|n| n.id == *id))
                .map(|i| i as i32)
                .unwrap_or(self.menu_hover);
            let rows = node.children.clone();
            let ph = rows.len() as i32 * 28 + 8;
            y = y.min((height - ph).max(36));
            panels.push(Panel {
                x,
                y,
                width: pw,
                rows,
                selected,
            });
            let Some(id) = next else { break };
            let Some((i, child)) = node
                .children
                .iter()
                .enumerate()
                .find(|(_, n)| n.id == *id && n.enabled && n.submenu)
            else {
                break;
            };
            y += 4 + i as i32 * 28;
            x = if x + pw * 2 <= width {
                x + pw
            } else {
                (x - pw).max(0)
            };
            node = child;
        }
        panels
    }
    pub fn appmenu_motion(&mut self, x: f64, y: f64, width: i32, height: i32) {
        // XR refreshes a stationary mouse ray every frame. Only actual movement
        // may take selection back from the keyboard.
        if self.menu_pointer.is_some_and(|p| (x-p[0]).hypot(y-p[1]) < 0.5) { return; }
        self.menu_pointer = Some([x,y]);
        if y < 36. && self.menu.is_some() {
            if let Some(next) = self
                .menu_index(x)
                .filter(|i| self.menu_headers()[*i].enabled)
            {
                if self.menu != Some(next) {
                    self.open_menu(Some(next));
                }
            }
            return;
        }
        if let Some(menu) = self.menu.filter(|m| !self.exported_menu(*m)) {
            let x0 = self.menu_x(menu);
            self.menu_hover = if x >= x0 && x < x0 + 380. && y >= 40. { ((y-40.) / 28.) as i32 } else { -1 };
            return;
        }
        let panels = self.appmenu_panels(width, height);
        for (level, panel) in panels.iter().enumerate().rev() {
            if x >= panel.x as f64
                && x < (panel.x + panel.width) as f64
                && y >= (panel.y + 4) as f64
                && y < (panel.y + 4 + panel.rows.len() as i32 * 28) as f64
            {
                let row = ((y - panel.y as f64 - 4.) / 28.) as usize;
                self.menu_hover = row as i32;
                if let Some(node) = panel.rows.get(row) {
                    if node.enabled && node.submenu {
                        if self.appmenu_path.get(level) != Some(&node.id) {
                            self.appmenu_path.truncate(level);
                            self.appmenu_path.push(node.id);
                            if let Some(address) = self.focused_menu_address() {
                                self.appmenus.open(address, node.id);
                            }
                        }
                        self.menu_hover = -1;
                    } else {
                        self.appmenu_path.truncate(level);
                    }
                }
                break;
            }
        }
    }
    pub fn appmenu_click(&mut self, x: f64, y: f64, width: i32, height: i32) -> bool {
        if !self.menu.is_some_and(|i| self.exported_menu(i)) {
            return false;
        }
        for (level, panel) in self.appmenu_panels(width, height).iter().enumerate().rev() {
            if x >= panel.x as f64
                && x < (panel.x + panel.width) as f64
                && y >= panel.y as f64
                && y < (panel.y + 8 + panel.rows.len() as i32 * 28) as f64
            {
                if y < (panel.y + 4) as f64 {
                    return true;
                }
                if let Some(node) = panel.rows.get(((y - panel.y as f64 - 4.) / 28.) as usize) {
                    if node.enabled && !node.separator {
                        if node.submenu {
                            self.appmenu_path.truncate(level);
                            self.appmenu_path.push(node.id);
                            self.menu_hover = -1;
                            if let Some(a) = self.focused_menu_address() {
                                self.appmenus.open(a, node.id);
                            }
                        } else {
                            if let Some(a) = self.focused_menu_address() {
                                self.appmenus.activate(a, node.id);
                            }
                            self.open_menu(None);
                        }
                    }
                }
                return true;
            }
        }
        self.open_menu(None);
        true
    }
    pub fn appmenu_key(&mut self, sym: u32) -> bool {
        if !self.menu.is_some_and(|i| self.exported_menu(i)) { return false; }
        let size = self.shell_geometry().size;
        let panels = self.appmenu_panels(size.w, size.h);
        if sym == 0xff1b { self.open_menu(None); return true; }
        if sym == 0xff51 {
            if self.appmenu_path.pop().is_some() {
                self.menu_hover = panels.iter().rev().nth(1).map_or(-1, |p| p.selected);
            } else { self.switch_menu(-1); }
            return true;
        }
        let Some(panel) = panels.last() else {
            if sym == 0xff53 { self.switch_menu(1); }
            return true;
        };
        if matches!(sym, 0xff52 | 0xff54 | 0xff50 | 0xff57 | 0xff55 | 0xff56 | 0xff09 | 0xfe20) {
            self.menu_hover = row_selection(&panel.rows, self.menu_hover, sym).unwrap_or(-1);
            return true;
        }
        if matches!(sym, 0xff0d | 0xff8d | 0x20 | 0xff53) {
            let selected = usize::try_from(self.menu_hover).ok()
                .filter(|i| panel.rows.get(*i).is_some_and(|n| n.enabled && !n.separator))
                .or_else(|| row_selection(&panel.rows, -1, 0xff50).map(|i| i as usize));
            if let Some(node) = selected.and_then(|i| panel.rows.get(i)) {
                if node.submenu {
                    self.appmenu_path.push(node.id);
                    self.menu_hover = row_selection(&node.children, -1, 0xff50).unwrap_or(-1);
                    if let Some(address) = self.focused_menu_address() { self.appmenus.open(address, node.id); }
                } else if sym == 0xff53 {
                    self.switch_menu(1);
                } else {
                    if let Some(address) = self.focused_menu_address() { self.appmenus.activate(address, node.id); }
                    self.open_menu(None);
                }
            } else if sym == 0xff53 { self.switch_menu(1); }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrows_wrap_skip_disabled_and_separators_and_recover_stale_rows() {
        let rows = vec![Node { enabled: true, ..Default::default() }, Node::default(),
            Node { enabled: true, separator: true, ..Default::default() }, Node { enabled: true, submenu: true, ..Default::default() }];
        assert_eq!(row_selection(&rows, -1, 0xff52), Some(3));
        assert_eq!(row_selection(&rows, -1, 0xff54), Some(0));
        assert_eq!(row_selection(&rows, 0, 0xff54), Some(3));
        assert_eq!(row_selection(&rows, 3, 0xff54), Some(0));
        assert_eq!(row_selection(&rows, 0, 0xff52), Some(3));
        assert_eq!(row_selection(&rows, 99, 0xff57), Some(3));
        assert_eq!(row_selection(&[], -1, 0xff54), None);
        assert_eq!(row_selection(&[Node::default()], -1, 0xff54), None);
    }
}
