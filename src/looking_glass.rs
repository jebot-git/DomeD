//! Opt-in Looking Glass-inspired cards. Original client geometry is never rewritten.
use crate::{Pose, Realm, Rect, Scene, WindowId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CardState {
    pub yaw_deg: f32,
    pub parked: bool,
    pub back: bool,
    pub note: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub enabled: bool,
    pub overview: bool,
    pub selected: Option<WindowId>,
    pub page: usize,
    pub cards: BTreeMap<WindowId, CardState>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Card {
    pub id: WindowId,
    pub title: String,
    pub rect: Rect,
    pub yaw_deg: f32,
    pub parked: bool,
    pub back: bool,
    pub note: String,
    pub selected: bool,
    pub minimized: bool,
}
pub const PAGE_SIZE: usize = 6;
pub fn ids(scene: &Scene, realm: Realm) -> Vec<WindowId> {
    scene
        .windows
        .iter()
        .filter(|w| {
            w.workspace == scene.active_workspace
                && w.visible_in(realm)
                && scene.popup(w.id).is_none()
        })
        .map(|w| w.id)
        .collect()
}
pub fn page(scene: &Scene, realm: Realm) -> (Vec<WindowId>, usize, usize) {
    let all = ids(scene, realm);
    let pages = all.len().div_ceil(PAGE_SIZE).max(1);
    let index = scene.looking_glass.page.min(pages - 1);
    (
        all.into_iter()
            .skip(index * PAGE_SIZE)
            .take(PAGE_SIZE)
            .collect(),
        index,
        pages,
    )
}
pub fn cards(scene: &Scene, realm: Realm, width: i32, height: i32) -> Vec<Card> {
    let (ids, _, _) = page(scene, realm);
    let columns = 3;
    let gap = 16.;
    let cell_w = ((width as f32 - 48.) / columns as f32).max(40.);
    let cell_h = ((height as f32 - 220.) / 2.).max(70.);
    ids.into_iter()
        .enumerate()
        .filter_map(|(i, id)| {
            let w = scene.window(id)?;
            let state = scene
                .looking_glass
                .cards
                .get(&id)
                .cloned()
                .unwrap_or_default();
            let yaw = if state.parked {
                if i % 3 == 0 {
                    -72.
                } else {
                    72.
                }
            } else if state.yaw_deg != 0. {
                state.yaw_deg
            } else {
                (i % 3) as f32 * 18. - 18.
            };
            Some(Card {
                id,
                title: w.title.clone(),
                rect: Rect {
                    x: 24. + (i % 3) as f32 * cell_w,
                    y: 110. + (i / 3) as f32 * cell_h,
                    w: cell_w - gap,
                    h: (cell_h - gap - 30.).max(40.),
                },
                yaw_deg: yaw,
                parked: state.parked,
                back: state.back,
                note: state.note,
                selected: scene.looking_glass.selected == Some(id),
                minimized: w.minimized,
            })
        })
        .collect()
}
/// Perspective projection shared by card rendering and hit testing; UV remains canonical.
pub fn corners(rect: Rect, yaw_deg: f32) -> [[f32; 2]; 4] {
    let a = yaw_deg.to_radians();
    let distance = rect.w * 2.5;
    [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]].map(|[u, v]| {
        let x = u * rect.w;
        let z = -x * a.sin();
        let scale = (distance - (rect.w * 0.5 * a.sin()).abs()) / (distance + z);
        [
            rect.x + rect.w / 2. + x * a.cos() * scale,
            rect.y + rect.h / 2. + v * rect.h * scale,
        ]
    })
}
pub fn contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x < rect.x + rect.w && y >= rect.y && y < rect.y + rect.h
}
pub fn hit(card: &Card, x: f32, y: f32) -> bool {
    let q = corners(card.rect, card.yaw_deg);
    let mut sign = 0.;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let cross = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
        if cross.abs() > 0.01 {
            if sign != 0. && cross * sign < 0. {
                return false;
            }
            sign = cross;
        }
    }
    true
}
/// Flat spatial cards with real orientations and ray geometry, not an XR screenshot.
pub fn quads(scene: &Scene, head: Pose) -> Vec<crate::xr_input::Quad> {
    let anchor = if scene.config.anchor_mode == crate::AnchorMode::Head {
        head.upright()
    } else {
        scene.frame_pose().upright()
    };
    let all = page(scene, Realm::Xr).0;
    let count = all.len();
    all.into_iter()
        .enumerate()
        .filter_map(|(index, id)| {
            let w = scene.window(id)?;
            if w.minimized {
                return None;
            }
            let state = scene
                .looking_glass
                .cards
                .get(&id)
                .cloned()
                .unwrap_or_default();
            // Back-side notes live on the compositor-owned overview panel; never mirror
            // an application's front or forward keyboard/mouse input through its back.
            if state.back {
                return None;
            }
            let angle = ((index as f32 - (count.saturating_sub(1)) as f32 / 2.) * 22.).to_radians();
            let yaw = if state.parked {
                if angle < 0. {
                    -72.
                } else {
                    72.
                }
            } else {
                state.yaw_deg
            };
            let turn = glam::Quat::from_rotation_y(-angle + yaw.to_radians());
            let width = 1.05 * w.xr_scale;
            let height = (width * w.desktop.h / w.desktop.w.max(1.)).clamp(0.25, 1.4);
            let position = [
                2.0 * angle.sin(),
                if index % 2 == 0 { 0.16 } else { -0.16 },
                -2.0 * angle.cos(),
            ];
            Some(crate::xr_input::Quad {
                id,
                pose: Pose {
                    position_m: anchor.transform(position),
                    orientation_xyzw: (glam::Quat::from_array(anchor.orientation_xyzw) * turn)
                        .to_array(),
                },
                size: [width, height],
                dome: None,
            })
        })
        .collect()
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
        if self.popup(id).is_some() {
            return Err("Select a top-level window".into());
        }
        self.window(id).ok_or("Unknown window")?;
        if yaw.is_some_and(|v| !v.is_finite() || v.abs() > 80.) {
            return Err("Card yaw must be between -80 and 80 degrees".into());
        }
        if note.as_ref().is_some_and(|n| {
            n.len() > 4096 || n.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
        }) {
            return Err("Notes must be at most 4096 UTF-8 bytes without control characters".into());
        }
        let mut card = self
            .looking_glass
            .cards
            .get(&id)
            .cloned()
            .unwrap_or_default();
        if let Some(v) = yaw {
            card.yaw_deg = v;
        }
        if let Some(v) = parked {
            card.parked = v;
        }
        if let Some(v) = back {
            card.back = v;
        }
        if let Some(v) = note {
            card.note = v;
        }
        let live: Vec<_> = self.windows.iter().map(|w| w.id).collect();
        self.looking_glass.cards.retain(|id, _| live.contains(id));
        self.looking_glass.cards.insert(id, card);
        self.looking_glass.selected = Some(id);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;
    #[test]
    fn opt_in_and_validation_preserve_layout() {
        let mut s = Scene::demo();
        let before = serde_json::to_value(&s.windows).unwrap();
        let id = s.windows[0].id;
        assert!(s.glass_edit(id, Some(60.), None, None, None).is_err());
        s.apply(Action::LookingGlass { enabled: true }).unwrap();
        s.apply(Action::GlassWindow {
            id,
            yaw_deg: Some(60.),
            parked: Some(true),
            back: Some(true),
            note: Some("České poznámky\nBack of window".into()),
        })
        .unwrap();
        let snapshot = serde_json::to_value(&s.looking_glass).unwrap();
        assert!(s.glass_edit(id, Some(f32::NAN), None, None, None).is_err());
        assert!(s
            .glass_edit(id, None, None, None, Some("x".repeat(4097)))
            .is_err());
        assert_eq!(snapshot, serde_json::to_value(&s.looking_glass).unwrap());
        s.apply(Action::LookingGlass { enabled: false }).unwrap();
        assert_eq!(before, serde_json::to_value(&s.windows).unwrap());
    }
    #[test]
    fn bounded_pages_projection_and_spatial_rays() {
        let mut s = Scene::demo();
        s.apply(Action::LookingGlass { enabled: true }).unwrap();
        let c = cards(&s, Realm::Desktop, 1024, 768);
        assert!(c.len() <= PAGE_SIZE);
        for card in &c {
            assert!(hit(
                card,
                card.rect.x + card.rect.w / 2.,
                card.rect.y + card.rect.h / 2.
            ));
            assert!(!hit(card, -20., -20.));
        }
        for q in quads(&s, Pose::default()) {
            q.pose.validate().unwrap();
            assert!(q.size.iter().all(|v| v.is_finite() && *v > 0.));
            let origin = glam::Vec3::from_array(Pose::default().position_m);
            let direction = (q.point([0.3, 0.4]) - origin).normalize();
            let uv = q.hit(origin, direction, false).unwrap();
            assert!((uv[0] - 0.3).abs() < 0.001 && (uv[1] - 0.4).abs() < 0.001);
        }
        s.looking_glass.page = usize::MAX;
        assert!(page(&s, Realm::Desktop).1 < page(&s, Realm::Desktop).2);
    }
}
