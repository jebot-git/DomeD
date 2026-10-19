//! Focus restoration for the shared seat when physical input moves between projections.
use crate::{Realm, Scene};
pub fn restore_focus(scene: &Scene, remembered: Option<Option<u64>>, realm: Realm) -> Option<u64> {
    let valid = |id| {
        scene.window(id).is_some_and(|w| {
            !w.minimized
                && w.workspace == scene.active_workspace
                && (realm == Realm::Xr || w.desktop_visible)
        })
    };
    match remembered {
        Some(None) => None,
        Some(Some(id)) if valid(id) => Some(id),
        _ => scene.focused.filter(|id| valid(*id)).or_else(|| {
            scene
                .windows
                .iter()
                .rev()
                .find(|w| valid(w.id))
                .map(|w| w.id)
        }),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_never_focuses_xr_only_client_and_each_realm_restores_its_window() {
        let mut s = Scene::new("test");
        let desktop = s.add("Desktop", "app");
        let xr = s.add("XR", "app");
        s.windows[1].desktop_visible = false;
        assert_eq!(restore_focus(&s, None, Realm::Desktop), Some(desktop));
        assert_eq!(
            restore_focus(&s, Some(Some(xr)), Realm::Desktop),
            Some(desktop)
        );
        assert_eq!(restore_focus(&s, Some(Some(xr)), Realm::Xr), Some(xr));
        assert_eq!(restore_focus(&s, Some(None), Realm::Desktop), None);
        s.windows[0].minimized = true;
        assert_eq!(restore_focus(&s, Some(Some(desktop)), Realm::Desktop), None);
    }
}
