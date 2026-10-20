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
        let Some(menu) = self.menu.filter(|i| self.exported_menu(*i)) else {
            return false;
        };
        let size = self.shell_geometry().size;
        let panels = self.appmenu_panels(size.w, size.h);
        let Some(panel) = panels.last() else {
            self.open_menu(None);
            return true;
        };
        match sym {
            0xff1b => self.open_menu(None),
            0xff52 | 0xff54 => {
                let len = panel.rows.len() as i32;
                if len > 0 {
                    let delta = if sym == 0xff52 { -1 } else { 1 };
                    let mut i = self.menu_hover;
                    for _ in 0..len {
                        i = (i + delta).rem_euclid(len);
                        if panel.rows[i as usize].enabled && !panel.rows[i as usize].separator {
                            self.menu_hover = i;
                            break;
                        }
                    }
                }
            }
            0xff0d | 0xff53 => {
                if let Some(node) = panel
                    .rows
                    .get(self.menu_hover.max(0) as usize)
                    .filter(|n| n.enabled && !n.separator)
                {
                    if node.submenu {
                        self.appmenu_path.push(node.id);
                        self.menu_hover = 0;
                        if let Some(a) = self.focused_menu_address() {
                            self.appmenus.open(a, node.id);
                        }
                    } else if sym == 0xff0d {
                        if let Some(a) = self.focused_menu_address() {
                            self.appmenus.activate(a, node.id);
                        }
                        self.open_menu(None);
                    } else {
                        self.open_menu(Some((menu + 1) % self.menu_headers().len()));
                    }
                }
            }
            0xff51 => {
                if self.appmenu_path.pop().is_none() {
                    self.open_menu(Some(menu - 1));
                }
                self.menu_hover = 0;
            }
            _ => (),
        }
        true
    }
}
