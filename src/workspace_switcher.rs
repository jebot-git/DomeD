//! Desktop-switcher data and Unicode rename editing shared by desktop and XR.
use crate::{Rect, Scene};
use serde::Serialize;

pub const MAX_DESKTOPS: usize = 8;
pub const MAX_NAME: usize = 32;
pub fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err("Desktop name must contain 1–32 printable characters".into());
    }
    Ok(name)
}

#[derive(Debug, Serialize)]
pub struct Desktop {
    pub name: String,
    pub windows: usize,
}
pub fn desktops(scene: &Scene) -> Vec<Desktop> {
    let mut rows: Vec<_> = scene
        .workspaces
        .iter()
        .map(|w| Desktop {
            name: w.name.clone(),
            windows: 0,
        })
        .collect();
    for window in &scene.windows {
        if scene.popup(window.id).is_none() {
            if let Some(row) = rows.get_mut(window.workspace) {
                row.windows += 1;
            }
        }
    }
    rows
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Layout {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub row_y: i32,
    pub row_height: i32,
    pub count: usize,
    pub can_add: bool,
}
#[derive(Debug, PartialEq)]
pub enum Hit {
    Outside,
    None,
    Switch(usize),
    Rename(usize),
    Input,
    Save,
    Cancel,
    Add,
}
impl Layout {
    pub fn new(width: i32, height: i32, count: usize) -> Self {
        let can_add = count < MAX_DESKTOPS;
        let rows = count + usize::from(can_add);
        let w = (width - 20).clamp(1, 560);
        let row_height = ((height - 166) / rows.max(1) as i32).clamp(16, 36);
        let h = rows as i32 * row_height + 60;
        let x = (width - w - 10).max(0);
        let y = (height - 70 - h).max(36);
        Self {
            x,
            y,
            w,
            h,
            row_y: y + 32,
            row_height,
            count,
            can_add,
        }
    }
    pub fn hit(self, x: f64, y: f64, editing: Option<usize>) -> Hit {
        if x < self.x as f64
            || x >= (self.x + self.w) as f64
            || y < self.y as f64
            || y >= (self.y + self.h) as f64
        {
            return Hit::Outside;
        }
        if y < self.row_y as f64 {
            return Hit::None;
        }
        let row = ((y - self.row_y as f64) / self.row_height as f64) as usize;
        if let Some(editing) = editing {
            if row != editing {
                return Hit::None;
            }
            return if x >= (self.x + self.w - 88) as f64 {
                Hit::Cancel
            } else if x >= (self.x + self.w - 172) as f64 {
                Hit::Save
            } else {
                Hit::Input
            };
        }
        if row < self.count {
            if x >= (self.x + self.w - 88) as f64 {
                Hit::Rename(row)
            } else {
                Hit::Switch(row)
            }
        } else if row == self.count && self.can_add {
            Hit::Add
        } else {
            Hit::None
        }
    }
    pub fn rect(self) -> Rect {
        Rect {
            x: self.x as f32,
            y: self.y as f32,
            w: self.w as f32,
            h: self.h as f32,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Rename {
    pub workspace: usize,
    pub original: String,
    pub text: String,
    /// UTF-8 byte indices, always on character boundaries.
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub error: String,
}
impl Rename {
    pub fn new(workspace: usize, name: String) -> Self {
        Self {
            workspace,
            cursor: name.len(),
            original: name.clone(),
            text: name,
            anchor: Some(0),
            error: String::new(),
        }
    }
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }
    fn selection(&self) -> std::ops::Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }
    fn erase_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            self.anchor = None;
            return false;
        }
        self.cursor = range.start;
        self.text.replace_range(range, "");
        self.anchor = None;
        true
    }
    pub fn key(&mut self, sym: u32, character: Option<char>, ctrl: bool, shift: bool) {
        self.error.clear();
        if ctrl && character.is_some_and(|c| c == 'a' || c == 'A') {
            self.select_all();
            return;
        }
        match sym {
            0xff51 | 0xff53 | 0xff50 | 0xff57 => {
                if shift {
                    if self.anchor.is_none() {
                        self.anchor = Some(self.cursor);
                    }
                } else if self.anchor.is_some() && matches!(sym, 0xff51 | 0xff53) {
                    let range = self.selection();
                    self.cursor = if sym == 0xff51 {
                        range.start
                    } else {
                        range.end
                    };
                    self.anchor = None;
                    return;
                } else {
                    self.anchor = None;
                }
                self.cursor = match sym {
                    0xff50 => 0,
                    0xff57 => self.text.len(),
                    0xff51 => self.text[..self.cursor]
                        .char_indices()
                        .last()
                        .map_or(0, |(i, _)| i),
                    _ => self.text[self.cursor..]
                        .chars()
                        .next()
                        .map_or(self.cursor, |c| self.cursor + c.len_utf8()),
                };
            }
            0xff08 | 0xffff => {
                if !self.erase_selection() {
                    if sym == 0xff08 {
                        let start = self.text[..self.cursor]
                            .char_indices()
                            .last()
                            .map_or(0, |(i, _)| i);
                        self.text.replace_range(start..self.cursor, "");
                        self.cursor = start;
                    } else if let Some(c) = self.text[self.cursor..].chars().next() {
                        self.text
                            .replace_range(self.cursor..self.cursor + c.len_utf8(), "");
                    }
                }
            }
            _ if !ctrl => {
                if let Some(c) = character.filter(|c| !c.is_control()) {
                    let selected = self.text[self.selection()].chars().count();
                    if self.text.chars().count() - selected >= MAX_NAME {
                        self.error = "Desktop names can contain at most 32 characters".into();
                        return;
                    }
                    self.erase_selection();
                    self.text.insert(self.cursor, c);
                    self.cursor += c.len_utf8();
                }
            }
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;
    #[test]
    fn counts_follow_windows_including_minimized_and_other_realms() {
        let mut scene = Scene::new("test");
        let a = scene.add("one", "test");
        let b = scene.add("two", "test");
        scene.apply(Action::Minimize { id: a }).unwrap();
        scene
            .windows
            .iter_mut()
            .find(|w| w.id == b)
            .unwrap()
            .desktop_visible = false;
        assert_eq!(desktops(&scene)[0].windows, 2);
        scene
            .apply(Action::Move {
                id: b,
                lane: 0,
                workspace: 1,
            })
            .unwrap();
        assert_eq!(
            desktops(&scene)
                .iter()
                .map(|r| r.windows)
                .collect::<Vec<_>>(),
            vec![1, 1, 0]
        );
        scene.apply(Action::Close { id: a }).unwrap();
        assert_eq!(desktops(&scene)[0].windows, 0);
    }
    #[test]
    fn unicode_editor_selection_insertion_and_character_limits() {
        let mut rename = Rename::new(1, "日本語".into());
        rename.key(0, Some('é'), false, false);
        assert_eq!(rename.text, "é");
        rename.key(0, Some('界'), false, false);
        rename.key(0xff51, None, false, false);
        rename.key(0xffff, None, false, false);
        assert_eq!(rename.text, "é");
        rename.key(0xff08, None, false, false);
        assert!(rename.text.is_empty());
        for _ in 0..32 {
            rename.key(0, Some('日'), false, false);
        }
        rename.key(0, Some('a'), false, false);
        assert_eq!(rename.text.chars().count(), 32);
        assert!(!rename.error.is_empty());
        rename.key(0, Some('a'), true, false);
        rename.key(0, Some('Č'), false, false);
        assert_eq!(rename.text, "Č");
        rename.key(0xff50, None, false, false);
        rename.key(0xff53, None, false, true);
        rename.key(0, Some('✓'), false, false);
        assert_eq!(rename.text, "✓");
    }
    #[test]
    fn names_and_layout_are_bounded_and_click_targets_match_rows() {
        assert_eq!(validate_name("  Práce 日本語  ").unwrap(), "Práce 日本語");
        for bad in [" ", "a\nb", "\0", &"日".repeat(33)] {
            assert!(validate_name(bad).is_err());
        }
        for count in [1, 3, 8] {
            let l = Layout::new(1024, 768, count);
            assert!(l.y >= 36 && l.y + l.h <= 698 && l.x + l.w <= 1024);
            assert_eq!(
                l.hit((l.x + 30) as f64, (l.row_y + 5) as f64, None),
                Hit::Switch(0)
            );
            assert_eq!(
                l.hit((l.x + l.w - 40) as f64, (l.row_y + 5) as f64, None),
                Hit::Rename(0)
            );
            assert_eq!(
                l.hit((l.x + l.w - 120) as f64, (l.row_y + 5) as f64, Some(0)),
                Hit::Save
            );
        }
        let small = Layout::new(640, 420, 8);
        assert!(small.y >= 36 && small.y + small.h <= 350);
        assert!(!small.can_add);
    }
    #[test]
    fn rename_preserves_desktop_identity_and_session_names() {
        let mut scene = Scene::new("test");
        let id = scene.add("window", "test");
        let focused = scene.focused;
        let name = "  Práce 日本語  ";
        scene
            .apply(Action::RenameWorkspace {
                workspace: 1,
                name: name.into(),
            })
            .unwrap();
        assert_eq!(scene.active_workspace, 0);
        assert_eq!(scene.focused, focused);
        assert_eq!(scene.window(id).unwrap().workspace, 0);
        assert_eq!(scene.workspaces[1].name, "Práce 日本語");
        for name in [" ".to_string(), "a\nb".into(), "日".repeat(33)] {
            assert!(scene
                .apply(Action::RenameWorkspace { workspace: 1, name })
                .is_err());
            assert_eq!(scene.workspaces[1].name, "Práce 日本語");
        }
        let saved = crate::session_state::Layout::capture(&scene, &Default::default());
        let saved: crate::session_state::Layout =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let mut reopened = Scene::new("restored");
        saved.prepare(&mut reopened);
        assert_eq!(reopened.workspaces[1].name, "Práce 日本語");
    }
}
