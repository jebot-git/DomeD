//! Placement shared by the Wayland desktop, XR renderer and XR hit testing.
use crate::{xr_input::Quad, Pose, Rect, Scene};
use glam::{Mat3, Quat, Vec3};
use serde::{Deserialize, Serialize};

// Reserved ranges remain exactly representable by JSON/JavaScript consumers.
pub const POPUP_ID_BASE: u64 = 1 << 48;
pub const BEAM_ID_BASE: u64 = 1 << 49;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Popup {
    pub id: u64,
    /// Immediate parent panel, including another popup for nested menus.
    pub parent: u64,
    /// Owning application window. Popups inherit its visibility and workspace.
    pub root: u64,
    /// Requested rectangle in parent content coordinates; also identifies the origin.
    pub anchor: Rect,
    #[serde(default)]
    pub context_at: Option<[f32; 2]>,
    #[serde(default)]
    pub viewport: Option<crate::context_menu::Viewport>,
}

impl Popup {
    pub fn height(&self) -> f32 {
        self.viewport.map_or(self.anchor.h, |v| v.height)
    }
    pub fn display_point(&self, point: [f32; 2]) -> [f32; 2] {
        [
            point[0],
            self.viewport
                .map_or(point[1], |v| point[1] - v.offset + v.band()),
        ]
    }
}

/// Keep the client size. If it exceeds the work area, clipping is unavoidable.
pub fn desktop_rect(
    parent: Rect,
    anchor: Option<[f32; 2]>,
    cursor: [f32; 2],
    size: [f32; 2],
    work: Rect,
) -> Rect {
    let [w, h] = size;
    let [x, y] = anchor.map_or([cursor[0], cursor[1] + 8.], |[x, y]| {
        [parent.x + x, parent.y + y]
    });
    Rect {
        x: x.clamp(work.x, (work.x + work.w - w).max(work.x)),
        y: y.clamp(work.y, (work.y + work.h - h).max(work.y)),
        w,
        h,
    }
}

impl Scene {
    pub fn popup(&self, id: u64) -> Option<&Popup> {
        self.popups.iter().find(|p| p.id == id)
    }
    pub fn popup_owner(&self, id: u64) -> u64 {
        self.popup(id).map_or(id, |p| p.root)
    }
    pub fn surface_visible_in(&self, id: u64, realm: crate::Realm) -> bool {
        self.popup(id).map_or_else(
            || self.window(id).is_some_and(|w| {
                !w.minimized && w.workspace == self.active_workspace && w.visible_in(realm)
            }),
            |p| self.popup_visible(p, realm == crate::Realm::Desktop),
        )
    }
    pub fn popup_visible(&self, p: &Popup, desktop: bool) -> bool {
        self.window(p.root).is_some_and(|w| {
            !w.minimized && w.workspace == self.active_workspace && w.visible_in(if desktop { crate::Realm::Desktop } else { crate::Realm::Xr })
        }) && self.window(p.id).is_none_or(|w| !w.minimized)
    }
    pub fn popup_desktop_rects(&self, work: Rect) -> Vec<(u64, Rect)> {
        let mut result: Vec<(u64, Rect)> = Vec::new();
        for _ in 0..self.popups.len() {
            let before = result.len();
            for popup in &self.popups {
                if result.iter().any(|(id, _)| *id == popup.id) || !self.popup_visible(popup, true)
                {
                    continue;
                }
                let parent = if self.popup(popup.parent).is_some() {
                    result
                        .iter()
                        .find(|(id, _)| *id == popup.parent)
                        .map(|(_, r)| *r)
                } else {
                    self.window(popup.parent)
                        .map(|w| if w.maximized { work } else { w.desktop })
                };
                if let Some(parent) = parent {
                    result.push((
                        popup.id,
                        if let Some(click) = popup.context_at {
                            crate::context_menu::rect(
                                parent,
                                click,
                                [popup.anchor.w, popup.height()],
                                work,
                            )
                        } else {
                            if self.window(popup.id).is_some() {
                                desktop_rect(parent, Some([popup.anchor.x, popup.anchor.y]), [parent.x, parent.y], [popup.anchor.w, popup.height()], work)
                            } else {
                                // Use the geometry DomeD configured to the client, relative
                                // to its parent. Moving it again here would desynchronize
                                // submenu navigation from rendering and hit testing.
                                Rect {
                                    x: parent.x + popup.anchor.x,
                                    y: parent.y + popup.anchor.y,
                                    w: popup.anchor.w,
                                    h: popup.height(),
                                }
                            }
                        },
                    ));
                }
            }
            if result.len() == before {
                break;
            }
        }
        result
    }
}

/// Append children after parents so OpenXR composition order follows popup depth.
pub fn append_quads(scene: &Scene, panels: &mut Vec<Quad>, head: Pose) {
    append_quads_in_fov(scene, panels, head, None);
}

/// Center transient surfaces slightly below their origin, then fit them to the
/// same binocular viewport used for rendering and pointer hit testing.
pub fn append_quads_in_fov(scene: &Scene, panels: &mut Vec<Quad>, head: Pose, viewport: Option<[f32; 4]>) {
    append_quads_with_policy(scene, panels, head, viewport, true);
}

/// An XR helper restarted inside an older desktop must retain that desktop's
/// geometry until both processes advertise the centered placement capability.
pub(crate) fn append_quads_with_policy(scene: &Scene, panels: &mut Vec<Quad>, head: Pose,
    viewport: Option<[f32; 4]>, centered: bool) {
    let frame=head.upright();
    let shell=crate::xr_transport::ShellLayout {width:1024,height:768,top:36,bottom:60,notifications:None,pullout:None};
    let bars=crate::xr_input::bar_quads_from_surfaces(scene,head,&shell,100.,panels);
    let mut lower=f32::NEG_INFINITY;
    let mut upper=f32::INFINITY;
    for bar in &bars {
        let y=if bar.id==crate::xr_transport::TOP_BAR {1.} else {0.};
        for x in [0.,0.5,1.] {
            let p=frame.inverse_rotate((bar.point([x,y])-Vec3::from_array(frame.position_m)).to_array());
            let radius=p[0].hypot(p[2]).max(0.01);
            if bar.id==crate::xr_transport::TOP_BAR {upper=upper.min(p[1]/radius);}
            else {lower=lower.max(p[1]/radius);}
        }
    }

    for _ in 0..scene.popups.len() {
        let before = panels.len();
        for popup in &scene.popups {
            if panels.iter().any(|q| q.id == popup.id) || !scene.popup_visible(popup, false) {
                continue;
            }
            let Some(parent) = panels.iter().find(|q| q.id == popup.parent) else {
                continue;
            };
            let parent_pixels = scene
                .popup(popup.parent)
                .map(|p| [p.anchor.w, p.height()])
                .or_else(|| {
                    scene
                        .window(popup.parent)
                        .map(|w| [w.desktop.w, w.desktop.h])
                });
            let Some([pw, ph]) = parent_pixels else {
                continue;
            };
            let mut size = [
                parent.size[0] * popup.anchor.w / pw,
                parent.size[1] * popup.height() / ph,
            ];
            // Some clients cannot resize their popup buffers (menus, input
            // methods or fixed dialogs). Keep all UVs and hit coordinates while
            // bounding their displayed size to the same half-resolution budget.
            let popup_scale = scene.config.xr_window_resolution
                .popup_scale([popup.anchor.w, popup.height()]);
            size = size.map(|dimension| dimension * popup_scale);
            let q = Quat::from_array(parent.pose.orientation_xyzw);
            let center = parent.point([0.5,0.5]);
            let local=frame.inverse_rotate((center-Vec3::from_array(frame.position_m)).to_array());
            let capacity=(upper-lower)*local[0].hypot(local[2])*0.9;
            if capacity.is_finite() && capacity>0. && size[1]>capacity {
                let scale=capacity/size[1];size=size.map(|v|v*scale);
            }
            let depth = 0.14;
            let click_center = popup.context_at.map(|point| {
                let point = scene
                    .popup(popup.parent)
                    .map_or(point, |p| p.display_point(point));
                parent.point([point[0] / pw, point[1] / ph])
            });
            let origin = click_center.unwrap_or(center);
            let distance = origin.distance(Vec3::from_array(head.position_m));
            // A small angular offset stays consistent across window resolutions
            // and parent distances. Do not lift the entire dialog above its parent.
            let offset = if centered {
                Vec3::new(0., -distance * 5_f32.to_radians().tan(), depth)
            } else if click_center.is_some() {
                Vec3::new(0., size[1] / 2. + crate::context_menu::GAP * parent.size[1] / ph, depth)
            } else {
                Vec3::new(0., parent.size[1] / 2. + size[1] / 2. + 0.04, depth)
            };
            let mut panel = Quad {
                id: popup.id,
                pose: Pose {
                    position_m: (origin + q * offset).to_array(),
                    ..parent.pose
                },
                size,
                dome: None,
            };
            fit_between_bars(&mut panel,frame,lower,upper);
            if let Some(viewport) = viewport.filter(|_| centered) {
                crate::xr_input::fit_popup(&mut panel, head, viewport);
            }
            panels.push(panel);
        }
        if panels.len() == before {
            break;
        }
    }
}

/// Keep every corner within the bar edges, including tilted/nested dialogs.
/// Move first; uniformly shrink an oversized surface while retaining its UVs.
fn fit_between_bars(panel: &mut Quad, frame: Pose, lower: f32, upper: f32) {
    if !lower.is_finite() || !upper.is_finite() || lower>=upper {return;}
    for _ in 0..32 {
        let mut bottom=f32::NEG_INFINITY;
        let mut top=f32::INFINITY;
        for uv in [[0.,0.],[1.,0.],[0.,1.],[1.,1.]] {
            let p=frame.inverse_rotate((panel.point(uv)-Vec3::from_array(frame.position_m)).to_array());
            let radius=p[0].hypot(p[2]).max(0.01);
            bottom=bottom.max(lower*radius-p[1]);
            top=top.min(upper*radius-p[1]);
        }
        if bottom<=top {
            let shift=0f32.clamp(bottom,top);
            panel.pose.position_m=(Vec3::from_array(panel.pose.position_m)+Vec3::from_array(frame.rotate([0.,shift,0.]))).to_array();
            return;
        }
        panel.size=panel.size.map(|v|v*0.8);
    }
}

/// Decorative beams never enter the list used for input targeting.
pub fn beams(scene: &Scene, panels: &[Quad], head: Pose) -> Vec<Quad> {
    scene
        .popups
        .iter()
        .filter_map(|popup| {
            let panel = panels.iter().find(|q| q.id == popup.id)?;
            let parent = panels.iter().find(|q| q.id == popup.parent)?;
            let pixels = scene
                .popup(popup.parent)
                .map(|p| [p.anchor.w, p.height()])
                .or_else(|| {
                    scene
                        .window(popup.parent)
                        .map(|w| [w.desktop.w, w.desktop.h])
                })?;
            let origin = popup.context_at.unwrap_or([popup.anchor.x, popup.anchor.y]);
            let origin = scene
                .popup(popup.parent)
                .map_or(origin, |p| p.display_point(origin));
            let uv = [
                (origin[0] / pixels[0]).clamp(0., 1.),
                (origin[1] / pixels[1]).clamp(0., 1.),
            ];
            let start = parent.point(uv);
            let end = panel.point([0.5, 1.]);
            let delta = end - start;
            if delta.length() < 0.001 {
                return None;
            }
            let along = delta.normalize();
            let towards_head = Vec3::from_array(head.position_m) - (start + end) / 2.;
            let normal = (towards_head - along * towards_head.dot(along)).normalize_or_zero();
            if normal.length_squared() < 0.5 {
                return None;
            }
            let right = along.cross(normal).normalize();
            Some(Quad {
                id: popup.id ^ BEAM_ID_BASE,
                pose: Pose {
                    position_m: ((start + end) / 2. + normal * 0.002).to_array(),
                    orientation_xyzw: Quat::from_mat3(&Mat3::from_cols(right, along, normal))
                        .to_array(),
                },
                size: [0.004, delta.length()],
                dome: None,
            })
        })
        .collect()
}
