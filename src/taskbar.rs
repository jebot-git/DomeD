//! Application grouping and activation policy shared by the native shell and tests.
use crate::{desktop_entries::App, Scene};
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct Task {
    pub key: String,
    pub name: String,
    pub icon: String,
    pub app: Option<usize>,
    pub windows: Vec<u64>,
    pub active: bool,
    pub minimized: bool,
    pub pinned: bool,
}
pub fn tasks(scene: &Scene, apps: &[App], pins: &[String]) -> Vec<Task> {
    let mut result: Vec<Task> = pins
        .iter()
        .filter_map(|key| {
            apps.iter().find(|a| a.desktop_id == *key).map(|a| Task {
                key: key.clone(),
                name: a.name.clone(),
                icon: a.icon.clone(),
                app: Some(a.id),
                windows: vec![],
                active: false,
                minimized: false,
                pinned: true,
            })
        })
        .collect();
    for w in scene
        .windows
        .iter()
        .filter(|w| w.workspace == scene.active_workspace)
    {
        let app = crate::desktop_entries::resolve(apps, &w.app_id);
        let key = app
            .map(|a| a.desktop_id.clone())
            .unwrap_or(w.app_id.clone());
        let index = if let Some(i) = result.iter().position(|t| t.key == key) {
            i
        } else {
            result.push(Task {
                key,
                name: crate::desktop_entries::display_name(app, &w.title).to_owned(),
                icon: app.map(|a| a.icon.clone()).unwrap_or_default(),
                app: app.map(|a| a.id),
                windows: vec![],
                active: false,
                minimized: true,
                pinned: false,
            });
            result.len() - 1
        };
        let task = &mut result[index];
        task.windows.push(w.id);
        task.active |= scene.focused == Some(w.id);
        task.minimized &= w.minimized;
    }
    result
}
#[derive(Debug, PartialEq)]
pub enum Activation {
    Launch(usize),
    Focus(u64),
    Minimize(u64),
    Group,
}
pub fn activate(task: &Task) -> Activation {
    match task.windows.as_slice() {
        [] => Activation::Launch(task.app.unwrap()),
        [id] if task.active => Activation::Minimize(*id),
        [id] => Activation::Focus(*id),
        _ => Activation::Group,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_minimized_windows_and_toggles_active_single() {
        let mut s = Scene::new("test");
        let a = s.add("one", "Editor");
        let b = s.add("two", "Editor");
        assert_eq!(tasks(&s, &[], &[])[0].windows, vec![a, b]);
        assert_eq!(activate(&tasks(&s, &[], &[])[0]), Activation::Group);
        s.apply(crate::Action::Close { id: b }).unwrap();
        assert_eq!(activate(&tasks(&s, &[], &[])[0]), Activation::Minimize(a));
        s.apply(crate::Action::Minimize { id: a }).unwrap();
        let task = tasks(&s, &[], &[]).remove(0);
        assert!(task.minimized);
        assert_eq!(activate(&task), Activation::Focus(a));
    }
}

/// XR task popups need a grace period while a ray crosses their edge or loses
/// its target. This policy has no seat/focus actions; callers only hide chrome.
#[derive(Default)]
pub struct PopupLeaveDelay {
    popup: Option<(usize, bool)>,
    left_at: Option<std::time::Duration>,
}
impl PopupLeaveDelay {
    pub const PAUSE: std::time::Duration = std::time::Duration::from_millis(400);
    pub fn update(&mut self, popup: Option<(usize, bool)>, inside: bool, now: std::time::Duration) -> bool {
        if self.popup != popup { self.popup = popup; self.left_at = None; }
        if popup.is_none() || inside { self.left_at = None; return false; }
        let left = *self.left_at.get_or_insert(now);
        now.saturating_sub(left) >= Self::PAUSE
    }
}
