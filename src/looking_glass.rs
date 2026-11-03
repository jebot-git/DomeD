//! Looking Glass v2: presentation-only tilt at the original desktop/dome placement.
use crate::{Rect, Scene, WindowId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const SETTLE_DELAY_MS: u64 = 350;
pub const PREVIEW_TOWARD_DEG: f32 = 12.;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CardState {
    pub yaw_deg: f32,
    pub display_yaw_deg: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub enabled: bool,
    pub cards: BTreeMap<WindowId, CardState>,
    pub focus_candidate: Option<WindowId>,
    pub settle_at_ms: Option<u64>,
    #[serde(skip)]
    pub last_tick_ms: u64,
}
pub fn angle(scene: &Scene, id: WindowId) -> f32 {
    if !scene.looking_glass.enabled {
        return 0.;
    }
    scene
        .looking_glass
        .cards
        .get(&id)
        .map_or(0., |c| c.display_yaw_deg)
}
pub fn opacity(scene: &Scene, id: WindowId) -> f32 {
    1. - 0.35 * (angle(scene, id).abs() / 90.).min(1.)
}
pub fn corners(rect: Rect, yaw: f32) -> [[f32; 2]; 4] {
    let a = yaw.clamp(-89.9, 89.9).to_radians();
    [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]].map(|[x, y]| {
        let scale = 1. / (1. - x * a.sin() / 2.5);
        [
            rect.x + rect.w * (0.5 + x * a.cos() * scale),
            rect.y + rect.h * (0.5 + y * scale),
        ]
    })
}
/// Inverse of the exact GPU projection; transparent parts cannot take input.
pub fn inverse(rect: Rect, yaw: f32, point: [f32; 2]) -> Option<[f32; 2]> {
    if rect.w <= 0. || rect.h <= 0. {
        return None;
    }
    let a = yaw.clamp(-89.9, 89.9).to_radians();
    let dx = (point[0] - rect.x) / rect.w - 0.5;
    let dy = (point[1] - rect.y) / rect.h - 0.5;
    let denom = a.cos() + dx * a.sin() / 2.5;
    if denom.abs() < 1e-7 {
        return None;
    }
    let x = dx / denom;
    let y = dy * (1. - x * a.sin() / 2.5);
    let uv = [x + 0.5, y + 0.5];
    uv.iter().all(|v| *v >= 0. && *v <= 1.).then_some(uv)
}
pub fn bounds(rect: Rect, yaw: f32) -> Rect {
    let q = corners(rect, yaw);
    let min = [
        q.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
        q.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
    ];
    let max = [
        q.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
        q.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max),
    ];
    Rect {
        x: min[0].floor(),
        y: min[1].floor(),
        w: (max[0].ceil() - min[0].floor()).max(1.),
        h: (max[1].ceil() - min[1].floor()).max(1.),
    }
}
/// Keep the normal dome/custom pose and physical extent, changing orientation only.
pub fn tilt_quad(scene: &Scene, q: &mut crate::xr_input::Quad) {
    let tilt = angle(scene, q.id);
    if tilt.abs() < 0.01 {
        return;
    }
    let center = q.point([0.5, 0.5]);
    q.pose.position_m = center.to_array();
    q.pose.orientation_xyzw = (glam::Quat::from_array(q.pose.orientation_xyzw)
        * glam::Quat::from_rotation_y(tilt.to_radians()))
    .to_array();
    q.dome = None;
}
impl Scene {
    pub fn glass_edit(
        &mut self,
        id: WindowId,
        yaw: Option<f32>,
        parked: Option<bool>,
        back: Option<bool>,
        note: Option<String>,
    ) -> Result<(), String> {
        if self.locked || !self.looking_glass.enabled {
            return Err("Looking Glass is not active".into());
        }
        self.window(id).ok_or("Unknown window")?;
        if self.popup(id).is_some() {
            return Err("Tilt the owning window".into());
        }
        if back == Some(true) || note.is_some() {
            return Err("Window backs and notes are not part of Looking Glass v2".into());
        }
        let value = yaw.unwrap_or_else(|| if parked == Some(true) { 90. } else { 0. });
        if !value.is_finite() || value.abs() > 90. {
            return Err("Tilt must be between -90 and 90 degrees".into());
        }
        self.looking_glass
            .cards
            .retain(|id, _| self.windows.iter().any(|w| w.id == *id));
        self.looking_glass.cards.insert(
            id,
            CardState {
                yaw_deg: value,
                display_yaw_deg: value,
            },
        );
        self.looking_glass.focus_candidate = None;
        self.looking_glass.settle_at_ms = None;
        Ok(())
    }
    pub fn glass_cycle(&mut self, id: WindowId, now: u64) {
        if !self.looking_glass.enabled {
            return;
        }
        self.looking_glass.focus_candidate = Some(id);
        self.looking_glass.settle_at_ms = Some(now + SETTLE_DELAY_MS);
        self.looking_glass.last_tick_ms = now;
        self.looking_glass.cards.entry(id).or_default();
        self.revision += 1;
    }
    pub fn glass_tick(&mut self, now: u64) {
        if !self.looking_glass.enabled {
            return;
        }
        if self.looking_glass.settle_at_ms.is_some_and(|t| now >= t) {
            if let Some(c) = self
                .looking_glass
                .focus_candidate
                .and_then(|id| self.looking_glass.cards.get_mut(&id))
            {
                c.yaw_deg = 0.;
            }
            self.looking_glass.settle_at_ms = None;
        }
        let step = (now.saturating_sub(self.looking_glass.last_tick_ms).min(50) as f32) * 0.72;
        self.looking_glass.last_tick_ms = now;
        let mut changed = false;
        for (id, card) in &mut self.looking_glass.cards {
            let target = if self.looking_glass.focus_candidate == Some(*id)
                && self.looking_glass.settle_at_ms.is_some()
            {
                card.yaw_deg.signum() * (card.yaw_deg.abs() - PREVIEW_TOWARD_DEG).max(0.)
            } else {
                card.yaw_deg
            };
            let delta = (target - card.display_yaw_deg).clamp(-step, step);
            if delta.abs() > 0.001 {
                card.display_yaw_deg += delta;
                changed = true;
            }
        }
        if changed {
            self.revision += 1;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Pose};
    #[test]
    fn tilt_is_reversible_and_keeps_client_geometry() {
        let mut s = Scene::demo();
        let id = s.windows[0].id;
        let before = serde_json::to_value(&s.windows).unwrap();
        assert!(s.glass_edit(id, Some(90.), None, None, None).is_err());
        s.apply(Action::LookingGlass { enabled: true }).unwrap();
        s.glass_edit(id, Some(90.), None, None, None).unwrap();
        assert_eq!(angle(&s, id), 90.);
        assert!(opacity(&s, id) < 1.);
        assert!(s.glass_edit(id, Some(91.), None, None, None).is_err());
        assert!(s.glass_edit(id, Some(f32::NAN), None, None, None).is_err());
        s.apply(Action::LookingGlass { enabled: false }).unwrap();
        assert_eq!(angle(&s, id), 0.);
        assert_eq!(before, serde_json::to_value(&s.windows).unwrap());
    }
    #[test]
    fn legacy_toggle_obeys_session_lock_and_clears_pending_focus() {
        let mut s = Scene::demo();
        s.locked = true;
        assert!(s.apply(Action::GlassOverview { open: true }).is_err());
        assert!(!s.looking_glass.enabled);
        s.locked = false;
        s.apply(Action::GlassOverview { open: true }).unwrap();
        s.glass_cycle(s.windows[0].id, 100);
        s.apply(Action::GlassOverview { open: false }).unwrap();
        assert!(s.looking_glass.settle_at_ms.is_none());
        assert!(s.looking_glass.focus_candidate.is_none());
    }
    #[test]
    fn projection_and_inverse_match_at_all_tilts() {
        let r = Rect {
            x: 100.,
            y: 80.,
            w: 1279.,
            h: 719.,
        };
        for yaw in [-90_f32, -80., -30., 0., 30., 80., 90.] {
            let a = yaw.clamp(-89.9_f32, 89.9_f32).to_radians();
            for uv in [[0.2, 0.3], [0.5, 0.5], [0.8, 0.7]] {
                let x = uv[0] - 0.5;
                let y = uv[1] - 0.5;
                let k = 1. / (1. - x * a.sin() / 2.5);
                let p = [
                    r.x + r.w * (0.5 + x * a.cos() * k),
                    r.y + r.h * (0.5 + y * k),
                ];
                let restored = inverse(r, yaw, p).unwrap();
                assert!((restored[0] - uv[0]).abs() < 0.001 && (restored[1] - uv[1]).abs() < 0.001);
            }
        }
    }
    #[test]
    fn focus_preview_debounces_without_flattening_intermediate_windows() {
        let mut s = Scene::demo();
        let a = s.windows[0].id;
        let b = s.windows[1].id;
        s.apply(Action::LookingGlass { enabled: true }).unwrap();
        s.glass_edit(a, Some(60.), None, None, None).unwrap();
        s.glass_edit(b, Some(-75.), None, None, None).unwrap();
        s.glass_cycle(a, 100);
        s.glass_tick(130);
        assert_eq!(angle(&s, a), 48.);
        s.glass_cycle(b, 200);
        s.glass_tick(250);
        assert_eq!(angle(&s, a), 60.);
        assert_eq!(angle(&s, b), -63.);
        s.glass_tick(549);
        assert_eq!(s.looking_glass.cards[&b].yaw_deg, -75.);
        for t in (550..=750).step_by(20) {
            s.glass_tick(t);
        }
        assert_eq!(angle(&s, b), 0.);
        assert_eq!(angle(&s, a), 60.);
    }
    #[test]
    fn xr_tilt_keeps_natural_center_extent_and_ray_uv() {
        let mut s = Scene::demo();
        let old = crate::xr_input::quads(&s, Pose::default());
        let q = old[0].clone();
        let id = q.id;
        s.apply(Action::LookingGlass { enabled: true }).unwrap();
        s.glass_edit(id, Some(65.), None, None, None).unwrap();
        let mut tilted = q.clone();
        tilt_quad(&s, &mut tilted);
        assert_eq!(q.size, tilted.size);
        assert!((q.point([0.5, 0.5]) - tilted.point([0.5, 0.5])).length() < 1e-5);
        let origin = glam::Vec3::from_array(Pose::default().position_m);
        let direction = (tilted.point([0.3, 0.4]) - origin).normalize();
        let uv = tilted.hit(origin, direction, false).unwrap();
        assert!((uv[0] - 0.3).abs() < 0.001 && (uv[1] - 0.4).abs() < 0.001);
    }
}
