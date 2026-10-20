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

/// XR cycles in carousel order within the focused lane. Desktop cycles visible clients.
pub fn cycle_windows(scene: &Scene, realm: Realm) -> Vec<u64> {
    if realm == Realm::Xr {
        let lane = scene
            .focused
            .and_then(|id| scene.window(id))
            .map(|w| w.lane)
            .unwrap_or(1);
        scene.workspaces[scene.active_workspace].lanes[lane]
            .windows
            .iter()
            .copied()
            .filter(|id| scene.window(*id).is_some_and(|w| !w.minimized))
            .collect()
    } else {
        scene
            .windows
            .iter()
            .filter(|w| w.workspace == scene.active_workspace && !w.minimized && w.desktop_visible)
            .map(|w| w.id)
            .collect()
    }
}

#[cfg(test)]
mod carousel_tests {
    use super::*;
    use crate::Action;
    #[test]
    fn xr_cycles_focused_lane_order_and_desktop_skips_xr_only_windows() {
        let mut s = Scene::new("test");
        let a = s.add("a", "app");
        let b = s.add("b", "app");
        let c = s.add("c", "app");
        let d = s.add("d", "app");
        s.apply(Action::Move {
            id: c,
            lane: 0,
            workspace: 0,
        })
        .unwrap();
        s.apply(Action::Move {
            id: d,
            lane: 1,
            workspace: 1,
        })
        .unwrap();
        s.apply(Action::Focus { id: a }).unwrap();
        assert_eq!(cycle_windows(&s, Realm::Xr), vec![b, a]);
        s.windows
            .iter_mut()
            .find(|w| w.id == b)
            .unwrap()
            .desktop_visible = false;
        assert_eq!(cycle_windows(&s, Realm::Desktop), vec![a, c]);
        s.apply(Action::Minimize { id: b }).unwrap();
        assert_eq!(cycle_windows(&s, Realm::Xr), vec![a]);
        s.apply(Action::Focus { id: c }).unwrap();
        assert_eq!(cycle_windows(&s, Realm::Xr), vec![c]);
        s.apply(Action::Recenter { id: c }).unwrap();
        assert!(s.panels().iter().find(|p| p.id == c).unwrap().yaw_deg.abs() < 0.001);
    }
}
