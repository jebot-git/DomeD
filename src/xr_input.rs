//! Shared XR surface geometry and physical mouse seat events.
use crate::{Action, Pose, Scene};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
mod shell_view;
mod media_view;
pub use shell_view::{shell_quads_in_fov, shared_viewport};
pub(crate) use shell_view::fit_popup;
/// Open cylinder strip. Vertical coordinates are height/radius, in radians of
/// unwrapped arc length; horizontal radius never shrinks towards a pole.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomePatch {
    pub frame: Pose,
    pub radius: f32,
    pub horizontal: f32,
    pub top: f32,
    pub bottom: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quad {
    pub id: u64,
    pub pose: Pose,
    pub size: [f32; 2],
    pub dome: Option<DomePatch>,
}
impl DomePatch {
    pub fn layer_pose(&self) -> Pose {
        Pose {
            position_m: self
                .frame
                .transform([0., self.radius * (self.top + self.bottom) / 2., 0.]),
            ..self.frame
        }
    }
    pub fn aspect_ratio(&self) -> f32 {
        self.horizontal / (self.top - self.bottom)
    }
    pub fn inward_normal(&self, point: Vec3) -> Vec3 {
        let local = self
            .frame
            .inverse_rotate((point - Vec3::from_array(self.frame.position_m)).to_array());
        Vec3::from_array(self.frame.rotate([-local[0], 0., -local[2]])).normalize_or_zero()
    }
}
/// Lean a cylindrical screen about its lower centre edge, preserving texture
/// dimensions and keeping the upper lane open rather than converging at a pole.
pub fn upper_tilt_frame(
    frame: Pose,
    radius: f32,
    bottom: f32,
    tilt_deg: f32,
    horizontal: f32,
) -> Pose {
    let lean = Quat::from_rotation_x(tilt_deg.to_radians());
    let pivot = Vec3::new(0., radius * bottom, -radius);
    // A tilted cylindrical strip's lower corners dip below its centre.
    // Lift by that dip so the upper lane never intersects the lane below.
    let clearance = radius * (1. - (horizontal / 2.).cos()) * tilt_deg.to_radians().sin();
    let shift = pivot - lean * pivot + Vec3::Y * clearance;
    Pose {
        position_m: frame.transform(shift.to_array()),
        orientation_xyzw: (Quat::from_array(frame.orientation_xyzw) * lean).to_array(),
    }
}
impl Quad {
    pub fn point(&self, uv: [f32; 2]) -> Vec3 {
        if let Some(d) = &self.dome {
            let yaw = (uv[0] - 0.5) * d.horizontal;
            let pitch = d.top + (d.bottom - d.top) * uv[1];
            Vec3::from_array(d.frame.transform([
                d.radius * yaw.sin(),
                d.radius * pitch,
                -d.radius * yaw.cos(),
            ]))
        } else {
            Vec3::from_array(self.pose.transform([
                (uv[0] - 0.5) * self.size[0],
                (0.5 - uv[1]) * self.size[1],
                0.,
            ]))
        }
    }
    pub fn hit(&self, origin: Vec3, direction: Vec3, clamp: bool) -> Option<[f32; 2]> {
        if let Some(patch) = &self.dome {
            let rotation = Quat::from_array(patch.frame.orientation_xyzw).conjugate();
            let o = rotation * (origin - Vec3::from_array(patch.frame.position_m));
            let d = rotation * direction.normalize_or_zero();
            let a = d.x * d.x + d.z * d.z;
            if a < 0.000001 {
                return None;
            } // No top/bottom caps.
            let b = o.x * d.x + o.z * d.z;
            let disc = b * b - a * (o.x * o.x + o.z * o.z - patch.radius * patch.radius);
            if disc < 0. {
                return None;
            }
            for t in [(-b - disc.sqrt()) / a, (-b + disc.sqrt()) / a] {
                if t <= 0. {
                    continue;
                }
                let p = o + d * t;
                // The window is visible from inside the dome, matching the compositor layer.
                if p.x * d.x + p.z * d.z <= 0. {
                    continue;
                }
                let yaw = p.x.atan2(-p.z);
                let pitch = p.y / patch.radius;
                let uv = [
                    yaw / patch.horizontal + 0.5,
                    (patch.top - pitch) / (patch.top - patch.bottom),
                ];
                if clamp {
                    return Some(uv);
                }
                // Compare in cylinder coordinates: converting a few ulps of
                // edge error to UV magnifies it on thin one-degree bars.
                let epsilon = 0.00001;
                if yaw.abs() <= patch.horizontal / 2. + epsilon
                    && pitch >= patch.bottom - epsilon && pitch <= patch.top + epsilon
                {
                    return Some(uv.map(|v| v.clamp(0., 1.)));
                }
            }
            return None;
        }
        let q = Quat::from_array(self.pose.orientation_xyzw);
        let o = q.conjugate() * (origin - Vec3::from_array(self.pose.position_m));
        let d = q.conjugate() * direction;
        if d.z >= -0.0001 {
            return None;
        }
        let t = -o.z / d.z;
        if t <= 0. {
            return None;
        }
        let p = o + d * t;
        let uv = [p.x / self.size[0] + 0.5, 0.5 - p.y / self.size[1]];
        if clamp {
            return Some(uv);
        }
        if uv.iter().any(|v| *v < -0.000001 || *v > 1.000001) {
            return None;
        }
        Some(uv.map(|v| v.clamp(0., 1.)))
    }
}
pub fn quads(scene: &Scene, head: Pose) -> Vec<Quad> {
    quads_in_fov(scene, head, None)
}
/// Include faded background workspaces for rendering, while input uses `quads`.
pub fn render_quads(scene: &Scene, head: Pose) -> Vec<Quad> {
    render_quads_in_fov(scene, head, None)
}
/// Use the renderer's shared binocular viewport for fullscreen media hit testing.
pub fn quads_in_fov(scene: &Scene, head: Pose, viewport: Option<[f32; 4]>) -> Vec<Quad> {
    window_quads(scene, head, false, viewport, true)
}
pub fn render_quads_in_fov(scene: &Scene, head: Pose, viewport: Option<[f32; 4]>) -> Vec<Quad> {
    window_quads(scene, head, true, viewport, true)
}
/// Use the desktop's advertised geometry when an XR helper is restarted across
/// an installation, so native pointer hits remain aligned with rendered popups.
pub fn render_quads_for_control(scene: &Scene, head: Pose, viewport: Option<[f32; 4]>, control: &crate::xr_transport::Control) -> Vec<Quad> {
    window_quads(scene, head, true, viewport, control.centered_popups)
}
fn window_quads(scene: &Scene, head: Pose, backgrounds: bool, viewport: Option<[f32; 4]>, centered: bool) -> Vec<Quad> {
    if scene.locked { return vec![]; }
    if let Some(fullscreen) = media_view::fullscreen_quad(scene, head, viewport) {
        let mut panels = vec![fullscreen];
        crate::spatial_popup::append_quads_with_policy(scene, &mut panels, head, viewport, centered);
        return panels;
    }
    let old = scene.frame_pose();
    let frame = if scene.config.anchor_mode == crate::AnchorMode::Head {
        head
    } else {
        old
    };
    let fq = Quat::from_array(frame.orientation_xyzw);
    let mut panels: Vec<Quad> = scene
        .panels()
        .into_iter()
        .filter(|p| backgrounds || p.workspace == scene.active_workspace)
        .filter_map(|p| {
            let w = scene.window(p.id)?;
            if let Some(slot) = w.pullout_slot.filter(|_| w.workspace == scene.active_workspace) {
                let anchor = scene.pullout_anchor?;
                let radius = scene.config.pullout_distance_m;
                // Preserve a readable angular size and cap individual arcs below 90°.
                let width = (1.3 * w.xr_scale).min(radius * 85f32.to_radians());
                let height = width * w.desktop.h / w.desktop.w;
                let gap = scene.config.window_gap_px
                    * scene
                        .windows
                        .iter()
                        .filter(|w| w.pullout_slot.is_some())
                        .map(|w| (1.3 * w.xr_scale).min(radius * 85f32.to_radians()) / w.desktop.w)
                        .fold(f32::INFINITY, f32::min);
                let angle = (width + gap) / (2. * radius) * if slot == 0 { -1. } else { 1. };
                let dome = DomePatch {
                    frame: Pose {
                        position_m: anchor.transform([0., -0.06, 0.]),
                        orientation_xyzw: (Quat::from_array(anchor.orientation_xyzw)
                            * Quat::from_rotation_y(-angle))
                        .to_array(),
                    },
                    radius,
                    horizontal: width / radius,
                    top: height / (2. * radius),
                    bottom: -height / (2. * radius),
                };
                return Some(Quad {
                    id: p.id,
                    pose: dome.layer_pose(),
                    size: [width, height],
                    dome: Some(dome),
                });
            }
            let offset = w.dome_offset_deg;
            let yaw = (p.yaw_deg + offset[0]).to_radians();
            let scale = w.xr_scale.min(175. / p.width_deg);
            let mid = (p.top_deg + p.bottom_deg) / 2.;
            let half = (p.top_deg - p.bottom_deg) / 2. * scale;
            let pitch = mid + offset[1];
            let height = p.radius_m * (p.top_deg - p.bottom_deg).to_radians() * scale;
            let radius = p.radius_m + if w.xr_scale > 1. && scene.focused != Some(w.id) { 0.004 } else { 0. };
            let dome = DomePatch {
                frame: {
                    let frame = Pose {
                        position_m: frame.position_m,
                        orientation_xyzw: (fq * Quat::from_rotation_y(-yaw)).to_array(),
                    };
                    if w.lane == 0 {
                        upper_tilt_frame(
                            frame,
                            p.radius_m,
                            (pitch - half).to_radians(),
                            scene.config.dome_upper_tilt_deg,
                            (p.width_deg * scale).to_radians(),
                        )
                    } else {
                        frame
                    }
                },
                radius,
                horizontal: (p.width_deg * scale).min(175.).to_radians(),
                top: (pitch + half).to_radians(),
                bottom: (pitch - half).to_radians(),
            };
            let pose = Pose {
                position_m: dome.frame.transform([
                    0.,
                    p.radius_m * pitch.to_radians(),
                    -p.radius_m,
                ]),
                orientation_xyzw: dome.frame.orientation_xyzw,
            };
            Some(Quad {
                id: p.id,
                pose,
                size: [height * w.desktop.w / w.desktop.h, height],
                dome: Some(dome),
            })
        })
        .collect();
    // Use the same conservative pixel pitch as packing so focus consumes four
    // gap pixels even when adjacent lanes or clients have different pixel densities.
    let mut horizontal_pitch = [f32::INFINITY; 3];
    let mut vertical_pitch = f32::INFINITY;
    let mut flat_pitch = [f32::INFINITY; 2];
    let mut pullout_pitch = [f32::INFINITY; 2];
    for q in &panels {
        let w = scene.window(q.id).unwrap();
        if w.workspace != scene.active_workspace { continue; }
        if let Some(d) = &q.dome {
            if w.pullout_slot.is_some() {
                pullout_pitch[0] = pullout_pitch[0].min(d.horizontal / w.desktop.w);
                pullout_pitch[1] = pullout_pitch[1].min((d.top - d.bottom) / w.desktop.h);
                continue;
            }
            horizontal_pitch[w.lane] = horizontal_pitch[w.lane].min(d.horizontal / w.desktop.w);
            vertical_pitch = vertical_pitch.min((d.top - d.bottom) / w.desktop.h);
        } else {
            flat_pitch[0] = flat_pitch[0].min(q.size[0] / w.desktop.w);
            flat_pitch[1] = flat_pitch[1].min(q.size[1] / w.desktop.h);
        }
    }
    for panel in &mut panels {
        if scene.focused == Some(panel.id) {
            let w = scene.window(panel.id).unwrap();
            if let Some(d) = &mut panel.dome {
                let horizontal_pixel = if w.pullout_slot.is_some() {
                    pullout_pitch[0]
                } else {
                    horizontal_pitch[w.lane]
                };
                let vertical_pixel = if w.pullout_slot.is_some() {
                    pullout_pitch[1]
                } else {
                    vertical_pitch
                };
                d.horizontal += 8. * horizontal_pixel;
                d.top += 4. * vertical_pixel;
                d.bottom -= 4. * vertical_pixel;
                d.radius -= 0.001;
                let pitch = (d.top + d.bottom) / 2.;
                panel.pose.position_m = d.frame.transform([0., d.radius * pitch, -d.radius]);
                panel.size = [d.radius * d.horizontal, d.radius * (d.top - d.bottom)];
            } else {
                panel.size[0] += 8. * flat_pitch[0];
                panel.size[1] += 8. * flat_pitch[1];
                let lift = Vec3::from_array(panel.pose.rotate([0., 0., 0.001]));
                panel.pose.position_m = (Vec3::from_array(panel.pose.position_m) + lift).to_array();
            }
        }
    }
    panels.sort_by_key(|q| window_order(scene, q.id));
    crate::spatial_popup::append_quads_with_policy(scene, &mut panels, head, viewport, centered);
    panels
}
/// Back-to-front order shared by layer submission and geometry.
pub fn window_order(scene: &Scene, id: u64) -> u8 {
    if scene.focused == Some(id) { 2 }
    else if scene.window(id).is_some_and(|w| w.xr_scale > 1.) { 0 }
    else { 1 }
}

/// Appearance-contrasting reticle using the desktop cursor palette.
pub fn cursor_pixels(light: bool) -> Vec<u8> {
    let (fill, edge) = crate::visual_feedback::cursor_colors(light);
    let mut pixels = vec![0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let dx = (x as f32 - 15.5).abs();
            let dy = (y as f32 - 15.5).abs();
            let radius = dx.hypot(dy);
            let ring = (9.5..=12.).contains(&radius);
            let cross = (dx <= 1. && dy <= 6.) || (dy <= 1. && dx <= 6.);
            let outline = (8.0..=13.5).contains(&radius)
                || (dx <= 2.5 && dy <= 7.5) || (dy <= 2.5 && dx <= 7.5);
            let color = if cross || ring { fill }
                else if outline { edge } else { [0; 4] };
            pixels[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4].copy_from_slice(&color);
        }
    }
    pixels
}

/// Dome bars sit outside the lane
/// latitudes and orbit the dome anchor with the headset's horizontal yaw.
/// Head pitch stays independent so looking up/down can reach either bar.
fn bar_quads(scene: &Scene, head: Pose, shell: &crate::xr_transport::ShellLayout, horizontal_fov_deg: f32) -> Vec<Quad> {
    let surfaces=quads(scene,head);
    bar_quads_from_surfaces(scene,head,shell,horizontal_fov_deg,&surfaces)
}
/// Shared physical bar geometry without recursively generating popup panels.
pub(crate) fn bar_quads_from_surfaces(scene: &Scene, head: Pose,
    shell: &crate::xr_transport::ShellLayout, horizontal_fov_deg: f32, surfaces: &[Quad]) -> Vec<Quad> {
    if shell.width == 0 || shell.height == 0 {
        return vec![];
    }
    let c = &scene.config;
    // Expanded configuration is a view pane, never a stretched upper bar.
    if shell.top == shell.height {
        let frame = if c.anchor_mode == crate::AnchorMode::Room {
            scene.frame_pose()
        } else {
            head
        };
        let depth = c.radius_m;
        let aspect = shell.width as f32 / shell.height as f32;
        let height = (2. * depth * (c.vertical_fov_deg.min(80.) * 0.35).to_radians().tan())
            .min(2. * depth * 40f32.to_radians().tan() / aspect);
        return vec![Quad {
            id: crate::xr_transport::TOP_BAR,
            pose: Pose {
                position_m: frame.transform([0., 0., -depth]),
                orientation_xyzw: frame.orientation_xyzw,
            },
            size: [height * aspect, height],
            dome: None,
        }];
    }
    let spans = scene.packed_lane_spans(scene.active_workspace);
    let forward = Vec3::from_array(head.rotate([0., 0., -1.]));
    let forward = Vec3::new(forward.x, 0., forward.z).normalize_or_zero();
    let horizontal = if horizontal_fov_deg.is_finite() {
        horizontal_fov_deg.clamp(30., 160.).to_radians()
    } else { 100f32.to_radians() };
    [(crate::xr_transport::TOP_BAR, 0, c.upper_bar_height_deg, shell.top, 36.),
     (crate::xr_transport::TASK_BAR, 2, c.taskbar_height_deg, shell.bottom, 60.)]
        .into_iter()
        .map(|(id, lane, height, pixels, nominal)| {
            // Follow the visible adjacent lane, including crowded/scaled panels,
            // focused outsets and the upper lane's exact lean and cylinder.
            let reference = surfaces.iter().filter(|q| scene.window(q.id)
                .is_some_and(|w| w.lane == lane && w.pullout_slot.is_none()))
                .filter(|q| q.dome.is_some())
                .max_by(|a, b| {
                    let score = |q: &Quad| {
                        let p = q.point([0.5, 0.5]) - Vec3::from_array(head.position_m);
                        Vec3::new(p.x, 0., p.z).normalize_or_zero().dot(forward)
                    };
                    score(a).total_cmp(&score(b))
                });
            let mut patch = reference.and_then(|q| q.dome.clone()).unwrap_or_else(|| {
                let frame = if c.anchor_mode == crate::AnchorMode::Head { head } else { scene.frame_pose() };
                DomePatch {
                    frame: if lane == 0 {
                        upper_tilt_frame(frame, c.radius_m, spans[0][0].to_radians(), c.dome_upper_tilt_deg, horizontal)
                    } else { frame },
                    radius: c.radius_m, horizontal,
                    bottom: spans[lane][0].to_radians(), top: spans[lane][1].to_radians(),
                }
            });
            let lane_pixel = (patch.top - patch.bottom)
                / reference.and_then(|q| scene.window(q.id)).map_or(460., |w| w.desktop.h);
            let bar_height = height.to_radians();
            // At most eight logical pixels at either surface's pixel density.
            let gap = 8. * lane_pixel.min(bar_height / nominal);
            let texture_height = bar_height * pixels as f32 / nominal;
            if lane == 0 {
                patch.top += gap + bar_height;
                patch.bottom = patch.top - texture_height;
            } else {
                patch.bottom -= gap + bar_height;
                patch.top = patch.bottom + texture_height;
            }
            // Orbit the entire patch about world up, including its lean pivot.
            // Turning inside the tilted cylinder frame introduces roll and moves
            // its edges sideways. Remove the adjacent panel's heading (and head
            // pitch/roll) before applying the headset's horizontal heading.
            let lean = if lane == 0 {
                Quat::from_rotation_x(c.dome_upper_tilt_deg.to_radians())
            } else { Quat::IDENTITY };
            let orientation = Quat::from_array(patch.frame.orientation_xyzw);
            let base = orientation * lean.conjugate();
            let yaw = forward.x.atan2(-forward.z);
            let orbit = Quat::from_rotation_y(-yaw) * base.conjugate();
            let anchor = Vec3::from_array(if c.anchor_mode == crate::AnchorMode::Head {
                head.position_m
            } else { scene.frame_pose().position_m });
            patch.frame.position_m = (anchor + orbit
                * (Vec3::from_array(patch.frame.position_m) - anchor)).to_array();
            patch.frame.orientation_xyzw = (orbit * orientation).to_array();
            patch.horizontal = horizontal;
            Quad {
                id, pose: patch.layer_pose(),
                size: [patch.radius * horizontal, patch.radius * texture_height],
                dome: Some(patch),
            }
        }).collect()
}

/// Rebase a mouse cursor from its surface point after the head or panel moves.
pub fn mouse_angles_at(head: Pose, point: Vec3) -> [f32; 2] {
    let d = Vec3::from_array(
        head.inverse_rotate((point - Vec3::from_array(head.position_m)).to_array()),
    )
    .normalize_or_zero();
    [d.x.atan2(-d.z), d.y.clamp(-1., 1.).asin()]
}
/// Cursor geometry follows the same curved or flat surface used for input.
pub fn surface_reticle(surface: &Quad, uv: [f32; 2], id: u64) -> Quad {
    let point = surface.point(uv);
    let normal = surface
        .dome
        .as_ref()
        .map(|d| d.inward_normal(point))
        .unwrap_or_else(|| Quat::from_array(surface.pose.orientation_xyzw) * Vec3::Z);
    Quad {
        id,
        pose: Pose {
            position_m: (point + normal * 0.002).to_array(),
            orientation_xyzw: Quat::from_rotation_arc(Vec3::Z, normal).to_array(),
        },
        size: [0.04, 0.04],
        dome: None,
    }
}
/// Invisible full cylinder for the mouse between windows, including behind the user.
/// It participates in cursor placement only, never application hit testing.
pub const MOUSE_DOME: u64 = u64::MAX - 4;
pub fn mouse_dome(scene: &Scene, head: Pose) -> Quad {
    let frame = if scene.config.anchor_mode == crate::AnchorMode::Head {
        head
    } else {
        scene.frame_pose()
    };
    let patch = DomePatch {
        frame, radius: scene.config.radius_m,
        horizontal: std::f32::consts::TAU, top: 1., bottom: -1.,
    };
    Quad { id: MOUSE_DOME, pose: patch.layer_pose(),
        size: [patch.radius * patch.horizontal, patch.radius * 2.], dome: Some(patch) }
}

/// Render the compositor's authoritative mouse target even when the renderer's
/// newer head pose would miss that surface. UVs also preserve captured drags.
pub fn mouse_cursor_reticle(
    surfaces: &[Quad], cursor: Option<&crate::xr_transport::Cursor>, head: Pose, angles: [f32; 2],
    scene: &Scene,
) -> Quad {
    let background = mouse_dome(scene, head);
    cursor.and_then(|c| surfaces.iter().chain(std::iter::once(&background))
        .find(|q| q.id == c.id).map(|q| surface_reticle(q, c.uv, u64::MAX)))
        .unwrap_or_else(|| {
            let origin = Vec3::from_array(head.position_m);
            background.hit(origin, mouse_ray(head, angles), true)
                .map(|uv| surface_reticle(&background, uv, u64::MAX))
                .unwrap_or_else(|| mouse_reticle(head, angles))
        })
}

/// One angular pointer spans all depths and empty space without target snapping.
pub fn mouse_ray(head: Pose, angles: [f32; 2]) -> Vec3 {
    let [yaw, pitch] = angles;
    Vec3::from_array(head.rotate([
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        -yaw.cos() * pitch.cos(),
    ]))
}

/// Shell layers render above applications, so they also win overlapping hits.
/// Expanded modal panes suspend application capture until they close.
pub fn mouse_target(
    surfaces: &[Quad], origin: Vec3, ray: Vec3, captured: Option<u64>, modal: bool,
) -> Option<(u64, [f32; 2], f32)> {
    surfaces.iter()
        .filter(|q| if modal {
            matches!(q.id, crate::xr_transport::TOP_BAR | crate::xr_transport::TOP_PANEL)
        } else {
            captured.is_none_or(|id| id == q.id)
        })
        .filter_map(|q| q.hit(origin, ray, captured.is_some() && !modal)
            .map(|uv| (q.id, uv, q.point(uv).distance(origin))))
        .min_by(|a, b| {
            crate::xr_transport::is_shell(b.0).cmp(&crate::xr_transport::is_shell(a.0))
                .then_with(|| a.2.total_cmp(&b.2))
        })
}

pub fn move_mouse(angles: &mut [f32; 2], dx: f64, dy: f64) {
    if !dx.is_finite() || !dy.is_finite() {
        return;
    }
    angles[0] = (angles[0] + dx as f32 * 0.0025)
        .clamp(-std::f32::consts::PI, std::f32::consts::PI);
    angles[1] = (angles[1] - dy as f32 * 0.0025).clamp(-1.5, 1.5);
}

/// Confine the mouse to the perspective viewport, including its corners.
/// A small angular inset keeps the reticle clear of the visible edge.
pub fn confine_mouse(angles: &mut [f32; 2], viewport: [f32; 4]) {
    let [left, right, down, up] = viewport;
    let inset = 0.025;
    let left = (left + inset).min(0.);
    let right = (right - inset).max(0.);
    let down = (down + inset).min(0.);
    let up = (up - inset).max(0.);
    angles[0] = angles[0].clamp(left, right);
    // Vertical FOV is measured against -Z, whereas pitch is an elevation.
    let cos_yaw = angles[0].cos();
    angles[1] = angles[1].clamp(
        (down.tan() * cos_yaw).atan(),
        (up.tan() * cos_yaw).atan(),
    );
}

/// Recover visibility when a newer render pose or a captured surface moves
/// outside the viewport before the compositor can update its target.
pub fn confine_mouse_reticle(reticle: Quad, head: Pose, viewport: [f32; 4]) -> Quad {
    let original = mouse_angles_at(head, Vec3::from_array(reticle.pose.position_m));
    let mut angles = original;
    confine_mouse(&mut angles, viewport);
    if (angles[0] - original[0]).abs() > 0.0001
        || (angles[1] - original[1]).abs() > 0.0001
    {
        mouse_reticle(head, angles)
    } else {
        reticle
    }
}

/// The mouse is a view cursor: its apparent size and depth never depend on
/// whether the ray is over a bar, a distant lane, or the space between them.
pub fn mouse_reticle(head: Pose, angles: [f32; 2]) -> Quad {
    let ray = mouse_ray(head, angles);
    Quad {
        id: u64::MAX,
        pose: Pose {
            position_m: (Vec3::from_array(head.position_m) + ray * 0.4).to_array(),
            orientation_xyzw: Quat::from_rotation_arc(Vec3::Z, -ray).to_array(),
        },
        size: [0.012, 0.012],
        dome: None,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeatEvent {
    Scroll { horizontal: f64, vertical: f64 },
    Motion { id: u64, uv: [f32; 2] },
    Button { button: u32, pressed: bool },
    Leave,
    Reset,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Packet {
    pub events: Vec<SeatEvent>,
    pub actions: Vec<Action>,
}

pub fn paint_border(rgba: &mut [u8], width: u32, height: u32, requested: u8) {
    if rgba.len() != width as usize * height as usize * 4 {
        return;
    }
    paint_border_scaled(rgba, width, height, requested, 1.);
}
pub fn paint_border_scaled(rgba: &mut [u8], width: u32, height: u32, requested: u8, scale: f32) {
    if rgba.len() != width as usize * height as usize * 4 { return; }
    let border = ((requested.min(2) as f32 * scale).round() as u32).min(width / 2).min(height / 2);
    for y in 0..height {
        let ranges = if y < border || y >= height - border {
            [0..width, 0..0]
        } else {
            [0..border, width - border..width]
        };
        for range in ranges {
            for x in range {
                let at = (y as usize * width as usize + x as usize) * 4;
                rgba[at..at + 4].copy_from_slice(&[146, 154, 165, 255]);
            }
        }
    }
}

/// Notifications occupy a separate, readable view pane instead of stretching a bar.
pub fn shell_quads(
    scene: &Scene,
    head: Pose,
    shell: &crate::xr_transport::ShellLayout,
) -> Vec<Quad> {
    shell_quads_in_view(scene, head, shell, 100.)
}

/// Preferred lane geometry before optical FOV fitting. Active XR rendering and
/// input use `shell_quads_in_fov` with the headset's shared viewport.
pub fn shell_quads_in_view(
    scene: &Scene, head: Pose, shell: &crate::xr_transport::ShellLayout, horizontal_fov_deg: f32,
) -> Vec<Quad> {
    if scene.locked { return vec![]; }
    let mut quads = bar_quads(scene, head, shell, horizontal_fov_deg);
    if let Some([x, y, w, h]) = shell
        .notifications
        .filter(|_| shell.width > 0 && shell.height > 0)
    {
        let depth = scene.config.bar_distance_m;
        let scale = (2.
            * depth
            * (horizontal_fov_deg.clamp(30., 80.) / 2.)
                .to_radians()
                .tan()
            / shell.width as f32)
            .min(
                2. * depth
                    * (scene.config.vertical_fov_deg.min(80.) * 0.4)
                        .to_radians()
                        .tan()
                    / shell.height as f32,
            );
        quads.push(Quad {
            id: crate::xr_transport::NOTIFICATIONS,
            pose: Pose {
                position_m: head.transform([
                    (x as f32 + w as f32 / 2. - shell.width as f32 / 2.) * scale,
                    (shell.height as f32 / 2. - y as f32 - h as f32 / 2.) * scale,
                    -depth,
                ]),
                orientation_xyzw: head.orientation_xyzw,
            },
            size: [w as f32 * scale, h as f32 * scale],
            dome: None,
        });
    }
    if scene.xr_fullscreen().is_some() {
        // Explicitly opened menus/panels remain available; idle chrome and
        // notifications must not cover fullscreen media or intercept its mouse.
        quads.retain(|q| (q.id == crate::xr_transport::TOP_BAR && shell.top > 36)
            || (q.id == crate::xr_transport::TASK_BAR && shell.bottom > 60));
    }
    quads
}
