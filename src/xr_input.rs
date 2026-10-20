//! Shared XR surface geometry and physical mouse seat events.
use crate::{Action, Pose, Scene};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
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
                // Ray/cylinder reconstruction can put an exact edge a few ulps outside.
                if uv.iter().all(|v| (-0.000001..=1.000001).contains(v)) {
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
        .filter(|p| p.workspace == scene.active_workspace)
        .filter_map(|p| {
            let w = scene.window(p.id)?;
            if let Some(slot) = w.pullout_slot {
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
                radius: p.radius_m,
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
    crate::spatial_popup::append_quads(scene, &mut panels, head);
    panels
}
/// Dome bars sit outside the lane
/// latitudes and orbit the dome anchor with the headset's horizontal yaw.
/// Head pitch stays independent so looking up/down can reach either bar.
fn bar_quads(scene: &Scene, head: Pose, shell: &crate::xr_transport::ShellLayout) -> Vec<Quad> {
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
    {
        let anchor = if c.anchor_mode == crate::AnchorMode::Head {
            head
        } else {
            scene.frame_pose()
        };
        let forward = Vec3::from_array(head.rotate([0., 0., -1.]));
        let yaw = if forward.x.hypot(forward.z) > 0.0001 {
            forward.x.atan2(-forward.z)
        } else {
            let right = Vec3::from_array(head.rotate([1., 0., 0.]));
            right.z.atan2(right.x)
        };
        let rotation = Quat::from_rotation_y(-yaw);
        let frame = Pose {
            position_m: anchor.position_m,
            orientation_xyzw: rotation.to_array(),
        };
        let inner = c.dome_vertical_span_deg / 2.;
        return [
            (
                crate::xr_transport::TOP_BAR,
                c.upper_bar_x_deg,
                c.upper_bar_max_width_deg,
                c.upper_bar_height_deg,
                shell.top,
                36.,
                1.,
            ),
            (
                crate::xr_transport::TASK_BAR,
                c.taskbar_x_deg,
                c.taskbar_max_width_deg,
                c.taskbar_height_deg,
                shell.bottom,
                60.,
                -1.,
            ),
        ]
        .into_iter()
        .map(|(id, x, width, height, pixels, nominal, sign)| {
            let bar_height = c.radius_m * height.to_radians();
            let h = bar_height * pixels as f32 / nominal;
            let edge = c.radius_m * inner.to_radians();
            let bar_frame = if sign > 0. {
                let bottom = scene
                    .panels()
                    .iter()
                    .filter(|p| p.workspace == scene.active_workspace && p.lane == 0)
                    .map(|p| p.bottom_deg)
                    .reduce(f32::min)
                    .unwrap_or(c.lane_spans(scene.workspaces[scene.active_workspace].shares)[0][0]);
                upper_tilt_frame(
                    frame,
                    c.radius_m,
                    bottom.to_radians(),
                    c.dome_upper_tilt_deg,
                    width.min(160.).to_radians(),
                )
            } else {
                frame
            };
            Quad {
                id,
                pose: Pose {
                    position_m: bar_frame.transform([
                        c.radius_m * x.to_radians().tan(),
                        sign * (edge + bar_height - h / 2.),
                        -c.radius_m,
                    ]),
                    orientation_xyzw: bar_frame.orientation_xyzw,
                },
                size: [
                    2. * c.radius_m * (width.min(160.) / 2.).to_radians().tan(),
                    h,
                ],
                dome: None,
            }
        })
        .collect();
    }
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
        size: [0.02, 0.02],
        dome: None,
    }
}
/// Render the compositor's authoritative mouse target even when the renderer's
/// newer head pose would miss that surface. UVs also preserve captured drags.
pub fn mouse_cursor_reticle(
    surfaces: &[Quad], cursor: Option<&crate::xr_transport::Cursor>, head: Pose, angles: [f32; 2],
) -> Quad {
    cursor.and_then(|c| surfaces.iter().find(|q| q.id == c.id).map(|q| surface_reticle(q, c.uv, u64::MAX)))
        .unwrap_or_else(|| mouse_reticle(head, angles))
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

pub fn move_mouse(angles: &mut [f32; 2], dx: f64, dy: f64) {
    if !dx.is_finite() || !dy.is_finite() {
        return;
    }
    angles[0] = (angles[0] + dx as f32 * 0.0025 + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    angles[1] = (angles[1] - dy as f32 * 0.0025).clamp(-1.5, 1.5);
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
        size: [0.006, 0.006],
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
    let border = (requested.min(2) as u32).min(width / 2).min(height / 2);
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
    let mut quads = bar_quads(scene, head, shell);
    if let Some([x, y, w, h]) = shell
        .notifications
        .filter(|_| shell.width > 0 && shell.height > 0)
    {
        let depth = scene.config.bar_distance_m;
        let scale = (2.
            * depth
            * (scene.config.upper_bar_max_width_deg.min(80.) / 2.)
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
    quads
}
