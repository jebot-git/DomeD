//! Client repaint hints include both desktop and XR presentation.
use crate::{Scene, WindowId};

pub fn suspended(scene: &Scene, id: WindowId, desktop_available: bool, xr_available: bool) -> bool {
    let Some(window) = scene.window(id) else { return true; };
    if scene.locked || window.minimized || window.workspace != scene.active_workspace {
        return true;
    }
    let desktop = desktop_available && window.desktop_visible &&
        scene.popup(id).is_none_or(|popup| scene.popup_visible(popup, true));
    let xr = xr_available && scene.presentation.desktop && window.xr_visible &&
        scene.popup(id).is_none_or(|popup| scene.popup_visible(popup, false));
    !(desktop || xr)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> (Scene, WindowId) {
        let mut scene = Scene::new("wayland");
        let id = scene.add("Browser", "org.domed.Browser");
        (scene, id)
    }
    #[test]
    fn a_desktop_window_stays_visible_without_an_xr_session() {
        let (mut scene, id) = scene();
        scene.windows[0].xr_visible = false;
        assert!(!suspended(&scene, id, true, false));
    }
    #[test]
    fn an_xr_only_window_needs_an_active_xr_output() {
        let (mut scene, id) = scene();
        scene.windows[0].desktop_visible = false;
        assert!(suspended(&scene, id, true, false));
        assert!(!suspended(&scene, id, false, true));
    }
    #[test]
    fn neither_realm_presented_suspends_a_window() {
        let (mut scene, id) = scene();
        scene.windows[0].desktop_visible = false;
        scene.windows[0].xr_visible = false;
        assert!(suspended(&scene, id, true, true));
    }
    #[test]
    fn minimization_suspends_both_realms() {
        let (mut scene, id) = scene();
        scene.windows[0].minimized = true;
        assert!(suspended(&scene, id, true, true));
    }
    #[test]
    fn another_workspace_is_not_presented() {
        let (mut scene, id) = scene();
        scene.active_workspace = 1;
        assert!(suspended(&scene, id, true, true));
    }
    #[test]
    fn locking_suspends_application_repainting() {
        let (mut scene, id) = scene();
        scene.locked = true;
        assert!(suspended(&scene, id, true, true));
    }
    #[test]
    fn hiding_xr_desktop_content_preserves_a_visible_monitor_window() {
        let (mut scene, id) = scene();
        scene.presentation.desktop = false;
        assert!(!suspended(&scene, id, true, true));
        scene.windows[0].desktop_visible = false;
        assert!(suspended(&scene, id, true, true));
    }
    #[test]
    fn an_unavailable_monitor_does_not_keep_an_unseen_window_visible() {
        let (scene, id) = scene();
        assert!(suspended(&scene, id, false, false));
        assert!(!suspended(&scene, id, false, true));
    }
}
