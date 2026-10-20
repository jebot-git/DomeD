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
/// Keep application menu identities and ordering; reserve File/Edit first and Window
/// before Help. Missing categories use compositor actions, including for binaries
/// which have no menu export. Local command tags avoid remote ID collisions.
fn universal_menus(mut exported: Vec<Node>, shell: Vec<Node>) -> Vec<Node> {
    exported.retain(|n| !n.separator);
    if exported.is_empty() { return shell; }
    let mut take = |name: &str| {
        exported.iter().position(|n| n.text.eq_ignore_ascii_case(name))
            .map(|i| exported.remove(i))
    };
    let file = take("File");
    let edit = take("Edit");
    let window = take("Window");
    let help = take("Help");
    let merge = |native: Option<Node>, category: usize| {
        let local = &shell[category];
        let Some(mut native) = native else { return local.clone(); };
        if !native.enabled {
            for child in &mut native.children { child.enabled = false; }
        }
        native.enabled = true;
        native.submenu = true;
        // Keep native actions and their enabled states; fill missing standard
        // operations without replacing application-specific undo/redo behaviour.
        let mut additions = local.children.clone();
        if category <= 1 {
            additions.retain(|row| !row.separator && !native.children.iter().any(|n|
                n.text.eq_ignore_ascii_case(&row.text)
                || (category == 0 && n.text.eq_ignore_ascii_case("Close"))
                || (matches!(row.command, Some(dome::shortcuts::Command::Undo | dome::shortcuts::Command::Redo))
                    && n.text.to_lowercase().starts_with(&(row.text.to_lowercase() + " ")))));
        }
        if additions.is_empty() { return native; }
        if !native.children.is_empty() {
            native.children.push(Node { separator: true, ..Default::default() });
        }
        native.children.extend(additions);
        native
    };
    let mut menus = vec![merge(file, 0), merge(edit, 1)];
    menus.extend(exported);
    menus.push(merge(window, 3));
    if let Some(help) = help { menus.push(help); }
    menus
}
impl Dome {
    fn universal_menus(&self) -> Vec<Node> {
        let scene = self.shell_scene();
        let shell = ["File", "Edit", "View", "Window", "Help"].into_iter().enumerate()
            .map(|(i, text)| Node {
                text: text.into(), enabled: true, submenu: true,
                command: Some(dome::shortcuts::Command::Separator),
                children: self.shell_menu_items(i + 2).into_iter().enumerate().map(|(id, (text, shortcut, command))| {
                    let chord = if dome::shortcuts::defaults().iter().any(|b| b.command == command) {
                        scene.config.keybindings.iter().find(|b| b.enabled && b.command == command)
                            .map(|b| b.label()).unwrap_or_default()
                    } else { shortcut.into() };
                    let presentation = if let dome::shortcuts::Command::PresentWindow { desktop, xr } = command {
                        Some(scene.focused.and_then(|id| scene.window(id))
                            .is_some_and(|w| w.desktop_visible == desktop && w.xr_visible == xr))
                    } else { None };
                    Node { id: id as i32, text, shortcut: chord, enabled: self.command_enabled(command),
                        separator: command == dome::shortcuts::Command::Separator,
                        checked: presentation == Some(true), checkable: presentation.is_some(), radio: presentation.is_some(),
                        command: Some(command), ..Default::default() }
                }).collect(), ..Default::default()
            }).collect();
        universal_menus(self.appmenu_snapshot().root.children, shell)
    }
    fn activate_menu_node(&mut self, node: &Node) {
        // Close first: commands may change focus or dismiss menus themselves.
        let address = self.focused_menu_address();
        self.open_menu(None);
        if let Some(command) = node.command { self.command(command); }
        else if let Some(address) = address { self.appmenus.activate(address, node.id); }
    }

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
        for node in self.universal_menus() {
            let width = match node.text.as_str() {
                "File" => 50, "Edit" => 52, "View" => 58, "Window" => 80, "Help" => 58,
                _ => (node.text.chars().count() as i32 * 8 + 24).clamp(48, 200),
            };
            headers.push(Header { text: node.text, x, width, enabled: node.enabled });
            x += width;
        }
        headers
    }
    pub fn menu_x(&self, index: usize) -> f64 {
        self.menu_headers().get(index).map_or(0., |h| h.x as f64)
    }
    pub fn panel_menu(&self, index: usize) -> bool {
        index >= 2
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
        if let Some(index) = index.filter(|i| self.panel_menu(*i)) {
            if let Some(node) = self.universal_menus().get(index - 2) {
                if node.command.is_none() {
                    if let Some(address) = self.focused_menu_address() {
                        self.appmenus.open(address.clone(), node.id);
                        if !node.submenu && node.children.is_empty() {
                            self.appmenus.activate(address, node.id);
                            self.menu = None;
                        }
                    }
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
                if self.panel_menu(next) {
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
        let Some(index) = self.menu.filter(|i| self.panel_menu(*i)) else {
            return vec![];
        };
        let menus = self.universal_menus();
        let Some(root) = menus.get(index - 2) else { return vec![]; };
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
        if let Some(menu) = self.menu.filter(|m| !self.panel_menu(*m)) {
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
        if !self.menu.is_some_and(|i| self.panel_menu(i)) {
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
                            self.activate_menu_node(node);
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
        if !self.menu.is_some_and(|i| self.panel_menu(i)) { return false; }
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
                    self.activate_menu_node(node);
                }
            } else if sym == 0xff53 { self.switch_menu(1); }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shell() -> Vec<Node> {
        ["File", "Edit", "View", "Window", "Help"].into_iter().map(|text| Node {
            text: text.into(), enabled: true, submenu: true,
            command: Some(dome::shortcuts::Command::Separator),
            children: vec![Node { id: 7, text: "Shell action".into(), enabled: true,
                command: Some(dome::shortcuts::Command::Close), ..Default::default() }],
            ..Default::default()
        }).collect()
    }
    #[test]
    fn non_exporters_and_empty_desktop_have_standard_menus() {
        let menus = universal_menus(vec![], shell());
        assert_eq!(menus.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
            ["File", "Edit", "View", "Window", "Help"]);
        assert!(menus.iter().all(|n| n.enabled && n.children[0].command.is_some()));
    }
    #[test]
    fn native_window_merges_shell_actions_without_id_collisions_or_duplicate_headers() {
        let native = |id, text: &str| Node { id, text: text.into(), enabled: true, submenu: true,
            children: vec![Node { id: 7, text: "Native action".into(), enabled: true, ..Default::default() }],
            ..Default::default() };
        let menus = universal_menus(vec![native(1, "Tools"), native(2, "Help"),
            native(3, "Window"), native(4, "File")], shell());
        assert_eq!(menus.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(), ["File", "Edit", "Tools", "Window", "Help"]);
        assert_eq!(menus[0].id, 4);
        let rows = &menus[3].children;
        assert_eq!(rows[0].id, rows[2].id);
        assert!(rows[0].command.is_none());
        assert!(rows[1].separator);
        assert_eq!(rows[2].command, Some(dome::shortcuts::Command::Close));
    }
    #[test]
    fn edit_keeps_native_operations_and_fills_only_missing_basics() {
        use dome::shortcuts::Command;
        let mut fallback = shell();
        fallback[1].children = [("Undo", Command::Undo), ("Copy", Command::Copy), ("Cut", Command::Cut)]
            .into_iter().map(|(text, command)| Node { text: text.into(), command: Some(command),
                enabled: true, ..Default::default() }).collect();
        let menus = universal_menus(vec![Node { text: "Edit".into(), enabled: true,
            children: vec![Node { id: 41, text: "Undo Typing".into(), enabled: false, ..Default::default() },
                Node { id: 42, text: "Copy".into(), enabled: true, ..Default::default() }],
            ..Default::default() }], fallback);
        let rows = &menus[1].children;
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].id, 41);
        assert!(!rows[0].enabled);
        assert!(rows[0].command.is_none() && rows[1].command.is_none());
        assert!(rows[2].separator);
        assert_eq!(rows[3].command, Some(Command::Cut));
    }
    #[test]
    fn disabled_native_window_cannot_enable_its_actions_but_shell_controls_remain_available() {
        let menus = universal_menus(vec![Node { text: "Window".into(), children: vec![Node {
            enabled: true, ..Default::default()
        }], ..Default::default() }], shell());
        let window = menus.iter().find(|n| n.text == "Window").unwrap();
        assert!(window.enabled);
        assert!(!window.children[0].enabled);
        assert!(window.children.last().unwrap().enabled);
    }
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
