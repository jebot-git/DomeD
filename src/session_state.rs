//! Versioned login-session data, independent of live Wayland object identities.
use crate::{Pose, Realm, Scene, Window, Workspace};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Restart {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub realm: Realm,
    pub windows: Vec<u64>,
    pub desktop_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub active_workspace: usize,
    pub focused: Option<u64>,
    pub pullout_anchor: Option<Pose>,
    pub presentation: crate::xr_presentation::Presentation,
}
impl Layout {
    pub fn capture(scene: &Scene, excluded: &HashSet<u64>) -> Self {
        let windows: Vec<_> = scene
            .windows
            .iter()
            .filter(|w| !excluded.contains(&w.id))
            .map(|w|scene.native_saved_window(w))
            .collect();
        let ids: HashSet<_> = windows.iter().map(|w| w.id).collect();
        let mut workspaces = scene.workspaces.clone();
        for workspace in &mut workspaces {
            for lane in &mut workspace.lanes {
                lane.windows.retain(|id| ids.contains(id));
            }
        }
        let (presentation,active_workspace,focused)=scene.native_saved_context();
        Self {
            workspaces,
            windows,
            active_workspace,
            focused: focused.filter(|id| ids.contains(id)),
            pullout_anchor: scene.pullout_anchor,
            presentation,
        }
    }
    pub fn prepare(&self, scene: &mut Scene) {
        scene.workspaces = self.workspaces.clone();
        for workspace in &mut scene.workspaces {
            for lane in &mut workspace.lanes {
                lane.windows.clear();
            }
        }
        scene.active_workspace = self.active_workspace;
        scene.pullout_anchor = self.pullout_anchor;
        scene.presentation = self.presentation;
        scene.revision += 1;
    }
    pub fn place(&self, scene: &mut Scene, saved: &Window, id: u64, restored: &HashMap<u64, u64>) {
        let head = scene.view_pose;
        let Some(window) = scene.windows.iter_mut().find(|w| w.id == id) else {
            return;
        };
        window.workspace = saved.workspace;
        window.lane = saved.lane;
        window.desktop = saved.desktop;
        window.restore = saved.restore;
        window.minimized = saved.minimized;
        window.maximized = saved.maximized;
        window.xr_fullscreen_anchor = (window.maximized && window.xr_media()).then_some(head);
        window.desktop_visible = saved.desktop_visible;
        window.xr_visible = saved.xr_visible;
        window.xr_pose = saved.xr_pose;
        window.xr_scale = saved.xr_scale;
        window.dome_offset_deg = saved.dome_offset_deg;
        window.pullout_slot = saved.pullout_slot;
        window.pullout_restore_scale = saved.pullout_restore_scale;
        if saved.pullout_slot.is_some() && scene.pullout_anchor.is_none() {
            scene.pullout_anchor = self.pullout_anchor;
        }
        // Rebuild membership using new identities, preserving saved lane order.
        for (workspace_index, workspace) in scene.workspaces.iter_mut().enumerate() {
            for (lane_index, lane) in workspace.lanes.iter_mut().enumerate() {
                lane.rotation_deg = self.workspaces[workspace_index].lanes[lane_index].rotation_deg;
                lane.windows.retain(|current| *current != id);
                let mut ordered: Vec<_> = self.workspaces[workspace_index].lanes[lane_index]
                    .windows
                    .iter()
                    .filter_map(|old| restored.get(old).copied())
                    .collect();
                for current in lane.windows.drain(..) {
                    if !ordered.contains(&current) {
                        ordered.push(current);
                    }
                }
                if saved.workspace == workspace_index
                    && saved.lane == lane_index
                    && !ordered.contains(&id)
                {
                    ordered.push(id);
                }
                lane.windows = ordered;
            }
        }
        let ranks: HashMap<_, _> = self
            .windows
            .iter()
            .enumerate()
            .filter_map(|(rank, w)| restored.get(&w.id).map(|id| (*id, rank)))
            .collect();
        scene
            .windows
            .sort_by_key(|w| ranks.get(&w.id).copied().unwrap_or(usize::MAX));
        scene.revision += 1;
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSession {
    pub version: u32,
    pub layout: Layout,
    pub restarts: Vec<Restart>,
}
impl SavedSession {
    pub fn validate(&self) -> Result<(), String> {
        let l = &self.layout;
        if self.version != 1
            || l.workspaces.is_empty()
            || l.workspaces.len() > 32
            || l.windows.len() > 256
            || self.restarts.len() > 256
            || l.active_workspace >= l.workspaces.len()
        {
            return Err("Invalid session version or size".into());
        }
        if let Some(pose) = l.pullout_anchor {
            pose.validate()?;
        }
        let mut ids = HashSet::new();
        for w in &l.windows {
            if w.id > u64::MAX - 256
                || !ids.insert(w.id)
                || w.workspace >= l.workspaces.len()
                || w.lane >= 3
                || ![
                    w.desktop.x,
                    w.desktop.y,
                    w.desktop.w,
                    w.desktop.h,
                    w.xr_scale,
                    w.dome_offset_deg[0],
                    w.dome_offset_deg[1],
                ]
                .iter()
                .all(|v| v.is_finite())
                || w.desktop.w <= 0.
                || w.desktop.h <= 0.
                || !(0.25..=4.).contains(&w.xr_scale)
                || w.pullout_slot.is_some_and(|slot| slot >= 2)
                || w.pullout_restore_scale
                    .is_some_and(|scale| !scale.is_finite() || !(0.25..=4.).contains(&scale))
            {
                return Err("Invalid saved window".into());
            }
            if let Some(r) = w.restore {
                if ![r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) || r.w <= 0. || r.h <= 0. {
                    return Err("Invalid restore rectangle".into());
                }
            }
            if let Some(pose) = w.xr_pose {
                pose.validate()?;
            }
        }
        let by_id: HashMap<_, _> = l.windows.iter().map(|w| (w.id, w)).collect();
        let mut members = HashSet::new();
        for (workspace_index, workspace) in l.workspaces.iter().enumerate() {
            if !workspace
                .shares
                .iter()
                .all(|share| share.is_finite() && *share > 0.)
                || (workspace.shares.iter().sum::<f32>() - 1.).abs() > 0.01
                || workspace.lanes.iter().any(|lane| {
                    !lane.rotation_deg.is_finite()
                        || lane.windows.iter().any(|id| !ids.contains(id))
                })
            {
                return Err("Invalid saved workspace".into());
            }
            for (lane_index, lane) in workspace.lanes.iter().enumerate() {
                for id in &lane.windows {
                    if !members.insert(*id)
                        || by_id[id].workspace != workspace_index
                        || by_id[id].lane != lane_index
                    {
                        return Err("Inconsistent saved lane membership".into());
                    }
                }
            }
        }
        if members != ids || l.focused.is_some_and(|id| !ids.contains(&id)) {
            return Err("Incomplete saved window membership".into());
        }
        for restart in &self.restarts {
            if restart.argv.len() > 256
                || restart
                    .argv
                    .iter()
                    .any(|arg| arg.len() > 65536 || arg.contains('\0'))
                || (restart.argv.is_empty() && restart.desktop_id.is_none())
                || restart
                    .env
                    .keys()
                    .any(|key| !matches!(key.as_str(), "QIMGV_SBS_PYTHON" | "ARC_SBS_MODEL"))
                || restart
                    .env
                    .values()
                    .any(|value| value.len() > 65536 || value.contains('\0'))
                || restart.windows.iter().any(|id| !ids.contains(id))
            {
                return Err("Invalid application restart data".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_window_ids_restore_desktop_and_xr_placement_and_lane_order() {
        let mut old = Scene::new("wayland");
        let a = old.add("Document", "editor");
        let b = old.add("XR", "viewer");
        old.windows[0].minimized = true;
        old.windows[1].origin = Realm::Xr;
        old.windows[1].desktop_visible = false;
        old.windows[1].xr_pose = Some(Pose {
            position_m: [0.4, 1.2, -1.8],
            ..Pose::default()
        });
        old.windows[1].xr_scale = 1.4;
        old.windows[1].dome_offset_deg = [12., -8.];
        old.windows[1].pullout_slot = Some(1);
        old.workspaces[0].lanes[1].rotation_deg = 25.;
        let saved = SavedSession {
            version: 1,
            layout: Layout::capture(&old, &HashSet::new()),
            restarts: vec![],
        };
        saved.validate().unwrap();
        let saved: SavedSession =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let mut new = Scene::new("wayland");
        let unrelated = new.add("Unrelated", "other");
        new.apply(crate::Action::Close { id: unrelated }).unwrap();
        saved.layout.prepare(&mut new);
        let nb = new.add("XR", "viewer");
        let na = new.add("Document", "editor");
        let mut mapping = HashMap::from([(b, nb)]);
        saved
            .layout
            .place(&mut new, &saved.layout.windows[1], nb, &mapping);
        mapping.insert(a, na);
        saved
            .layout
            .place(&mut new, &saved.layout.windows[0], na, &mapping);
        assert_eq!(
            new.window(nb).unwrap().xr_pose,
            old.window(b).unwrap().xr_pose
        );
        assert_eq!(new.window(nb).unwrap().xr_scale, 1.4);
        assert_eq!(new.window(nb).unwrap().dome_offset_deg, [12., -8.]);
        assert_eq!(new.window(nb).unwrap().pullout_slot, Some(1));
        assert!(!new.window(nb).unwrap().desktop_visible);
        assert!(new.window(na).unwrap().minimized);
        assert_eq!(new.workspaces[0].lanes[1].rotation_deg, 25.);
        for lane in 0..3 {
            let expected: Vec<_> = old.workspaces[0].lanes[lane]
                .windows
                .iter()
                .map(|id| mapping[id])
                .collect();
            assert_eq!(new.workspaces[0].lanes[lane].windows, expected);
        }
        let mut corrupt = saved;
        corrupt.layout.windows[0].lane = 3;
        assert!(corrupt.validate().is_err());
    }
}
