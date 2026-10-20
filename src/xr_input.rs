//! Shared native XR interaction policy. No engine or renderer owns gesture semantics.
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
                let width = 0.78 * w.xr_scale;
                let gap = scene.config.window_gap_px
                    * scene
                        .windows
                        .iter()
                        .filter(|w| w.pullout_slot.is_some())
                        .map(|w| 0.78 * w.xr_scale / w.desktop.w)
                        .fold(f32::INFINITY, f32::min);
                let pose = Pose {
                    position_m: anchor.transform([
                        if slot == 0 {
                            -width / 2. - gap / 2.
                        } else {
                            width / 2. + gap / 2.
                        },
                        -0.06,
                        -scene.config.pullout_distance_m,
                    ]),
                    orientation_xyzw: anchor.orientation_xyzw,
                };
                return Some(Quad {
                    id: p.id,
                    pose,
                    size: [width, width * w.desktop.h / w.desktop.w],
                    dome: None,
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
    for q in &panels {
        let w = scene.window(q.id).unwrap();
        if let Some(d) = &q.dome {
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
                let horizontal_pixel = horizontal_pitch[w.lane];
                let vertical_pixel = vertical_pitch;
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
/// latitudes and orbit the dome anchor with the headset's horizontal gaze.
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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Hand {
    pub valid: bool,
    pub position: [f32; 3],
    pub ray: [f32; 3],
    pub pinch: [bool; 3],
    pub grip: bool,
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
#[derive(Clone)]
struct Grab {
    id: u64,
    start: Vec3,
    pose: Pose,
    scale: f32,
    dome_offset: [f32; 2],
    docked: bool,
    switched: bool,
    towards_head: Vec3,
}
struct Scale {
    id: u64,
    distance: f32,
    value: f32,
}
pub struct Interaction {
    armed: [bool; 2],
    buttons: [bool; 3],
    owner: Option<usize>,
    target: Option<u64>,
    grabs: [Option<Grab>; 2],
    scale: Option<Scale>,
    last: f64,
    last_positions: [Option<Vec3>; 2],
}
impl Default for Interaction {
    fn default() -> Self {
        Self {
            armed: [false; 2],
            buttons: [false; 3],
            owner: None,
            target: None,
            grabs: [None, None],
            scale: None,
            last: 0.,
            last_positions: [None, None],
        }
    }
}
fn interaction_pose(q: &Quad, scene: &Scene) -> Pose {
    let mut pose = q.pose;
    if q.dome.is_none() && scene.focused == Some(q.id) {
        let lift = Vec3::from_array(pose.rotate([0., 0., 0.001]));
        pose.position_m = (Vec3::from_array(pose.position_m) - lift).to_array();
    }
    pose
}
impl Interaction {
    pub fn reset(&mut self) -> Packet {
        *self = Self::default();
        Packet {
            events: vec![SeatEvent::Reset],
            actions: vec![],
        }
    }
    pub fn update(
        &mut self,
        time: f64,
        focused: bool,
        hands: [Hand; 2],
        scene: &Scene,
        head: Pose,
    ) -> Packet {
        self.update_surfaces(time, focused, hands, scene, head, &[])
    }
    pub fn update_surfaces(
        &mut self,
        time: f64,
        focused: bool,
        hands: [Hand; 2],
        scene: &Scene,
        head: Pose,
        extra: &[Quad],
    ) -> Packet {
        self.update_pointer(time, focused, hands, scene, head, extra, None)
    }
    /// Gaze owns pointing; either hand supplies buttons and physical fist motion.
    pub fn update_gaze_surfaces(
        &mut self,
        time: f64,
        focused: bool,
        hands: [Hand; 2],
        scene: &Scene,
        head: Pose,
        extra: &[Quad],
        gaze: Hand,
    ) -> Packet {
        if !gaze.valid
            || gaze
                .position
                .iter()
                .chain(gaze.ray.iter())
                .any(|v| !v.is_finite())
        {
            return self.reset();
        }
        self.update_pointer(time, focused, hands, scene, head, extra, Some(gaze))
    }
    fn update_pointer(
        &mut self,
        time: f64,
        focused: bool,
        hands: [Hand; 2],
        scene: &Scene,
        head: Pose,
        extra: &[Quad],
        gaze: Option<Hand>,
    ) -> Packet {
        if !scene.presentation.desktop
            || !time.is_finite()
            || !focused
            || (self.last > 0. && (time - self.last > 0.15 || time < self.last))
        {
            return self.reset();
        }
        self.last = time;
        for (i, h) in hands.iter().enumerate() {
            if !h.valid
                || h.position
                    .iter()
                    .chain(h.ray.iter())
                    .any(|v| !v.is_finite())
            {
                if self.owner == Some(i) || self.grabs[i].is_some() {
                    return self.reset();
                }
                self.armed[i] = false;
                self.last_positions[i] = None;
                continue;
            }
            let p = Vec3::from_array(h.position);
            if (self.grabs[i].is_some() || self.owner == Some(i))
                && self.last_positions[i].is_some_and(|old| old.distance(p) > 0.35)
            {
                return self.reset();
            }
            self.last_positions[i] = Some(p);
            if !h.grip && !h.pinch.iter().any(|p| *p) {
                self.armed[i] = true;
            }
        }
        let mut quads = quads(scene, head);
        quads.extend_from_slice(extra);
        quads.retain(|q| scene.presentation.interactive(q.id));
        if self
            .grabs
            .iter()
            .flatten()
            .any(|g| !quads.iter().any(|q| q.id == g.id))
        {
            return self.reset();
        }
        let hit = |i: usize| -> Option<(u64, [f32; 2])> {
            if !hands[i].valid {
                return None;
            }
            let h = gaze.unwrap_or(hands[i]);
            quads
                .iter()
                .filter_map(|p| {
                    p.hit(Vec3::from_array(h.position), Vec3::from_array(h.ray), false)
                        .map(|uv| (p.id, uv, p.point(uv).distance(Vec3::from_array(h.position))))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(id, uv, _)| (id, uv))
        };
        let mut out = Packet::default();
        for i in 0..2 {
            if !hands[i].grip {
                self.grabs[i] = None;
            } else if self.armed[i] && self.grabs[i].is_none() {
                if let Some((id, _)) = hit(i)
                    .filter(|(id, _)| scene.window(*id).is_some() && scene.popup(*id).is_none())
                {
                    let q = quads.iter().find(|q| q.id == id).unwrap();
                    self.grabs[i] = Some(Grab {
                        id,
                        start: Vec3::from_array(hands[i].position),
                        pose: interaction_pose(q, scene),
                        scale: scene.window(id).unwrap().xr_scale,
                        dome_offset: scene.window(id).unwrap().dome_offset_deg,
                        docked: scene.window(id).unwrap().pullout_slot.is_some(),
                        switched: false,
                        towards_head: (Vec3::from_array(head.position_m)
                            - Vec3::from_array(q.pose.position_m))
                        .normalize_or_zero(),
                    });
                    out.actions.push(Action::Focus { id });
                }
            }
        }
        if self.grabs.iter().any(Option::is_some) {
            for (i, pressed) in self.buttons.iter_mut().enumerate() {
                if *pressed {
                    out.events.push(SeatEvent::Button {
                        button: [0x110, 0x112, 0x111][i],
                        pressed: false,
                    });
                    *pressed = false;
                }
            }
            self.owner = None;
            self.target = None;
            {
                if let [Some(a), Some(b)] = &self.grabs {
                    if a.id == b.id {
                        if a.switched || b.switched {
                            return out;
                        }
                        let delta = (Vec3::from_array(hands[0].position)
                            + Vec3::from_array(hands[1].position)
                            - a.start
                            - b.start)
                            / 2.;
                        let inward =
                            delta.dot((a.towards_head + b.towards_head).normalize_or_zero());
                        let action = if !a.docked
                            && inward > 0.18
                            && scene
                                .windows
                                .iter()
                                .filter(|w| w.pullout_slot.is_some())
                                .count()
                                < 2
                        {
                            Some(Action::PullOut { id: a.id, head })
                        } else if a.docked && inward < -0.18 {
                            Some(Action::ReturnToDome { id: a.id })
                        } else {
                            None
                        };
                        if let Some(action) = action {
                            out.actions.push(action);
                            for g in self.grabs.iter_mut().flatten() {
                                g.switched = true;
                            }
                            self.scale = None;
                            return out;
                        }
                    }
                }
            }
            if let [Some(a), Some(b)] = &self.grabs {
                if a.id == b.id {
                    let left = Vec3::from_array(hands[0].position);
                    let right = Vec3::from_array(hands[1].position);
                    let d = left.distance(right);
                    if d < 0.04 {
                        return self.reset();
                    }
                    let s = self.scale.get_or_insert_with(|| Scale {
                        id: a.id,
                        distance: d,
                        value: scene.window(a.id).unwrap().xr_scale,
                    });
                    let value = (s.value * d / s.distance).clamp(0.25, 4.);
                    out.actions.push(Action::SpatialTransform {
                        id: s.id,
                        pose: None,
                        scale: value,
                    });
                    return out;
                }
            }
            if self.scale.take().is_some() {
                for i in 0..2 {
                    if let Some(g) = &mut self.grabs[i] {
                        g.start = Vec3::from_array(hands[i].position);
                        if let Some(q) = quads.iter().find(|q| q.id == g.id) {
                            g.pose = interaction_pose(q, scene);
                            g.scale = scene.window(g.id).unwrap().xr_scale;
                        }
                    }
                }
            }
            for i in 0..2 {
                if let Some(g) = &mut self.grabs[i] {
                    let delta = Vec3::from_array(hands[i].position) - g.start;
                    {
                        if g.switched {
                            continue;
                        }
                        let inward = delta.dot(g.towards_head);
                        if !g.docked && inward > 0.18 {
                            // Reserve capacity immediately within this packet as well as in Scene.
                            let pending = out
                                .actions
                                .iter()
                                .filter(|a| matches!(a, Action::PullOut { .. }))
                                .count();
                            if scene
                                .windows
                                .iter()
                                .filter(|w| w.pullout_slot.is_some())
                                .count()
                                + pending
                                < 2
                            {
                                out.actions.push(Action::PullOut { id: g.id, head });
                                g.switched = true;
                            }
                        } else if g.docked && inward < -0.18 {
                            out.actions.push(Action::ReturnToDome { id: g.id });
                            g.switched = true;
                        } else if !g.docked {
                            let q = Quat::from_array(g.pose.orientation_xyzw);
                            let local = q.conjugate() * delta;
                            let r = scene.config.radius_m;
                            out.actions.push(Action::DomeOffset {
                                id: g.id,
                                yaw_deg: g.dome_offset[0] + (local.x / r).to_degrees(),
                                pitch_deg: g.dome_offset[1] + (local.y / r).to_degrees(),
                            });
                        }
                    }
                }
            }
            return out;
        }
        self.scale = None;
        if let Some(gaze) = gaze {
            let origin = Vec3::from_array(gaze.position);
            let ray = Vec3::from_array(gaze.ray);
            let target = if let Some(id) = self.target {
                let Some(q) = quads.iter().find(|q| q.id == id) else {
                    return self.reset();
                };
                q.hit(origin, ray, true).map(|uv| (id, uv))
            } else {
                quads
                    .iter()
                    .filter_map(|q| {
                        q.hit(origin, ray, false)
                            .map(|uv| (q.id, uv, q.point(uv).distance_squared(origin)))
                    })
                    .min_by(|a, b| a.2.total_cmp(&b.2))
                    .map(|(id, uv, _)| (id, uv))
            };
            out.events.push(
                target
                    .map(|(id, uv)| SeatEvent::Motion { id, uv })
                    .unwrap_or(SeatEvent::Leave),
            );
            for b in 0..3 {
                let down = (0..2).any(|i| {
                    self.armed[i] && hands[i].valid && !hands[i].grip && hands[i].pinch[b]
                });
                let pressed =
                    down && (target.is_some() || self.buttons[b] || !scene.popups.is_empty());
                if pressed != self.buttons[b] {
                    if pressed {
                        if let Some((id, _)) = target {
                            self.target = Some(id);
                            let owner = scene.popup_owner(id);
                            if scene.window(owner).is_some() {
                                out.actions.push(Action::Focus { id: owner });
                            }
                        }
                    }
                    out.events.push(SeatEvent::Button {
                        button: [0x110, 0x112, 0x111][b],
                        pressed,
                    });
                    self.buttons[b] = pressed;
                }
            }
            if !self.buttons.iter().any(|v| *v) {
                self.target = None;
            }
            return out;
        }
        let owner = self.owner.or_else(|| {
            [1usize, 0]
                .into_iter()
                .find(|i| {
                    self.armed[*i]
                        && hands[*i].pinch.iter().any(|v| *v)
                        && (hit(*i).is_some() || !scene.popups.is_empty())
                })
                .or_else(|| {
                    [1usize, 0]
                        .into_iter()
                        .find(|i| self.armed[*i] && (hit(*i).is_some() || !scene.popups.is_empty()))
                })
        });
        if let Some(i) = owner {
            if !self.armed[i] || !hands[i].valid {
                return self.reset();
            }
            let target = if let Some(id) = self.target {
                let Some(q) = quads.iter().find(|q| q.id == id) else {
                    return self.reset();
                };
                q.hit(
                    Vec3::from_array(hands[i].position),
                    Vec3::from_array(hands[i].ray),
                    true,
                )
                .map(|uv| (id, uv))
            } else {
                hit(i)
            };
            if let Some((id, uv)) = target {
                out.events.push(SeatEvent::Motion { id, uv });
            } else {
                out.events.push(SeatEvent::Leave);
            }
            for b in 0..3 {
                let pressed = hands[i].pinch[b]
                    && (target.is_some() || self.buttons[b] || !scene.popups.is_empty());
                if pressed != self.buttons[b] {
                    if pressed {
                        if let Some((id, _)) = target {
                            self.target = Some(id);
                            if scene.window(scene.popup_owner(id)).is_some() {
                                out.actions.push(Action::Focus {
                                    id: if scene.window(id).is_some() {
                                        id
                                    } else {
                                        scene.popup_owner(id)
                                    },
                                });
                            }
                        }
                    }
                    out.events.push(SeatEvent::Button {
                        button: [0x110, 0x112, 0x111][b],
                        pressed,
                    });
                    self.buttons[b] = pressed;
                }
            }
            self.owner = if self.buttons.iter().any(|v| *v) {
                Some(i)
            } else {
                self.target = None;
                None
            };
        }
        out
    }
}
/// OpenXR joint order: palm, wrist, thumb 2–5, index 6–10, middle 11–15,
/// ring 16–20, little 21–25. Thresholds are normalised to palm span like WiVRn-Pro.
#[derive(Default)]
pub struct Detector {
    pinch: [bool; 3],
    grip: bool,
    candidate: u8,
    since: f64,
    release: Option<f64>,
    last: f64,
    armed: bool,
    neutral: Option<f64>,
}
impl Detector {
    fn gestures(&mut self, time: f64, ratios: [f32; 3], extension: [f32; 4]) {
        if !time.is_finite() || time < self.last || time - self.last > 0.15 {
            *self = Self::default();
        }
        self.last = time;
        if !self.armed {
            if extension.iter().filter(|v| **v > 0.82).count() >= 3
                && ratios.iter().all(|v| *v > 0.42)
            {
                let since = *self.neutral.get_or_insert(time);
                if time - since >= 0.1 {
                    self.armed = true;
                }
            } else {
                self.neutral = None;
            }
            return;
        }
        if self.grip {
            if extension.iter().filter(|v| **v > 0.82).count() >= 2 {
                let since = *self.release.get_or_insert(time);
                if time - since >= 0.1 {
                    self.grip = false;
                    self.candidate = 0;
                    self.release = None;
                }
            } else {
                self.release = None;
            }
            return;
        }
        if let Some(i) = self.pinch.iter().position(|v| *v) {
            if ratios[i] > 0.42 {
                self.pinch = [false; 3];
                self.candidate = 0;
            }
            return;
        }
        let pins: Vec<_> = ratios
            .iter()
            .enumerate()
            .filter(|(_, v)| **v < 0.28)
            .map(|(i, _)| i)
            .collect();
        let candidate =
            if extension[0] < 0.65 && extension.iter().filter(|v| **v < 0.65).count() >= 3 {
                4
            } else if pins.len() == 1 {
                pins[0] as u8 + 1
            } else {
                0
            };
        if candidate != self.candidate {
            self.candidate = candidate;
            self.since = time;
        }
        if candidate > 0 && time - self.since >= if candidate == 4 { 0.08 } else { 0.045 } {
            if candidate == 4 {
                self.grip = true;
            } else {
                self.pinch[candidate as usize - 1] = true;
            }
        }
    }
    pub fn sample(&mut self, time: f64, joints: Option<[[f32; 3]; 26]>, tracked: bool) -> Hand {
        let Some(j) = joints.filter(|j| tracked && j.iter().flatten().all(|v| v.is_finite()))
        else {
            *self = Self::default();
            return Hand::default();
        };
        let p = |i| Vec3::from_array(j[i]);
        let span = p(7).distance(p(22));
        if !(0.03..=0.13).contains(&span) {
            *self = Self::default();
            return Hand::default();
        }
        let ratios = [10, 15, 20].map(|tip| p(5).distance(p(tip)) / span);
        let extension = [7usize, 12, 17, 22].map(|base| {
            let length = (base..base + 3)
                .map(|i| p(i).distance(p(i + 1)))
                .sum::<f32>();
            if length > 0.015 {
                p(base).distance(p(base + 3)) / length
            } else {
                1.
            }
        });
        self.gestures(time, ratios, extension);
        let forward = (p(7) - p(1)).normalize_or_zero();
        Hand {
            valid: forward.length() > 0.9,
            position: p(0).to_array(),
            ray: forward.to_array(),
            pinch: self.pinch,
            grip: self.grip,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Scene, Pose, Hand) {
        let mut s = Scene::new("test");
        let id = s.add("Client", "app");
        let _ = id;
        let q = quads(&s, Pose::default()).remove(0);
        let origin = Vec3::from_array(q.pose.position_m)
            + Quat::from_array(q.pose.orientation_xyzw) * Vec3::Z;
        let ray = (Vec3::from_array(q.pose.position_m) - origin).normalize();
        (
            s,
            Pose::default(),
            Hand {
                valid: true,
                position: origin.to_array(),
                ray: ray.to_array(),
                ..Hand::default()
            },
        )
    }
    fn apply(s: &mut Scene, p: &Packet) {
        for a in &p.actions {
            s.apply(a.clone()).unwrap();
        }
    }
    #[test]
    fn gaze_points_without_hands_and_ignores_hand_ray_for_all_mouse_buttons() {
        for b in 0..3 {
            let (s, head, gaze) = setup();
            let mut input = Interaction::default();
            let hover =
                input.update_gaze_surfaces(1., true, [Hand::default(); 2], &s, head, &[], gaze);
            assert!(hover
                .events
                .iter()
                .any(|e| matches!(e, SeatEvent::Motion { .. })));
            let mut hand = gaze;
            hand.ray = [0., 1., 0.];
            input.update_gaze_surfaces(1.01, true, [hand; 2], &s, head, &[], gaze);
            hand.pinch[b] = true;
            let press = input.update_gaze_surfaces(1.02, true, [hand; 2], &s, head, &[], gaze);
            assert_eq!(
                press
                    .events
                    .iter()
                    .filter(|e| matches!(e, SeatEvent::Button { pressed: true, .. }))
                    .count(),
                1
            );
            assert!(press.events.iter().any(|e| matches!(e, SeatEvent::Button { button, pressed: true } if *button == [0x110, 0x112, 0x111][b])));
            // Either hand may release without releasing the other's held button.
            let still = input.update_gaze_surfaces(
                1.03,
                true,
                [Hand::default(), hand],
                &s,
                head,
                &[],
                gaze,
            );
            assert!(!still
                .events
                .iter()
                .any(|e| matches!(e, SeatEvent::Button { .. })));
            let release =
                input.update_gaze_surfaces(1.04, true, [Hand::default(); 2], &s, head, &[], gaze);
            assert!(release
                .events
                .iter()
                .any(|e| matches!(e, SeatEvent::Button { pressed: false, .. })));
        }
    }
    #[test]
    fn gaze_drag_keeps_surface_until_release_and_requires_neutral_after_reset() {
        let (s, head, mut gaze) = setup();
        let mut hand = gaze;
        let mut input = Interaction::default();
        input.update_gaze_surfaces(1., true, [hand; 2], &s, head, &[], gaze);
        hand.pinch[0] = true;
        let first = input.update_gaze_surfaces(1.01, true, [hand; 2], &s, head, &[], gaze);
        let id = first
            .events
            .iter()
            .find_map(|e| {
                if let SeatEvent::Motion { id, .. } = e {
                    Some(*id)
                } else {
                    None
                }
            })
            .unwrap();
        gaze.ray = [0.8, 0., -1.];
        let drag = input.update_gaze_surfaces(1.02, true, [hand; 2], &s, head, &[], gaze);
        assert!(drag
            .events
            .iter()
            .any(|e| matches!(e, SeatEvent::Motion { id: target, .. } if *target == id)));
        let lost = input.update_gaze_surfaces(1.03, false, [hand; 2], &s, head, &[], gaze);
        assert!(matches!(lost.events[0], SeatEvent::Reset));
        let resumed = input.update_gaze_surfaces(1.04, true, [hand; 2], &s, head, &[], gaze);
        assert!(!resumed
            .events
            .iter()
            .any(|e| matches!(e, SeatEvent::Button { pressed: true, .. })));
    }
    #[test]
    fn three_pinches_map_to_linux_buttons_and_release() {
        for b in 0..3 {
            let (s, h, mut hand) = setup();
            let mut i = Interaction::default();
            i.update(1., true, [hand, Hand::default()], &s, h);
            hand.pinch[b] = true;
            let p = i.update(1.01, true, [hand, Hand::default()], &s, h);
            assert!(p.events.iter().any(|e|matches!(e,SeatEvent::Button{button,pressed:true}if *button==[0x110,0x112,0x111][b])));
            hand.pinch[b] = false;
            assert!(i
                .update(1.02, true, [hand, Hand::default()], &s, h)
                .events
                .iter()
                .any(|e| matches!(e, SeatEvent::Button { pressed: false, .. })));
        }
    }
    #[test]
    fn held_gesture_needs_neutral_after_loss() {
        let (s, h, mut hand) = setup();
        let mut i = Interaction::default();
        hand.pinch[0] = true;
        assert!(i
            .update(1., true, [hand, Hand::default()], &s, h)
            .events
            .is_empty());
        hand.pinch[0] = false;
        i.update(1.01, true, [hand, Hand::default()], &s, h);
        hand.pinch[0] = true;
        i.update(1.02, true, [hand, Hand::default()], &s, h);
        assert!(matches!(
            i.update(1.03, false, [hand, Hand::default()], &s, h).events[0],
            SeatEvent::Reset
        ));
        assert!(i
            .update(1.04, true, [hand, Hand::default()], &s, h)
            .events
            .is_empty());
    }
    #[test]
    fn gaze_fist_targets_gaze_but_moves_with_the_wrist() {
        let (mut scene, head, gaze) = setup();
        let mut hand = gaze;
        hand.ray = [0., 1., 0.];
        let mut input = Interaction::default();
        input.update_gaze_surfaces(1., true, [hand, Hand::default()], &scene, head, &[], gaze);
        hand.grip = true;
        hand.pinch = [true; 3];
        let packet = input.update_gaze_surfaces(
            1.01,
            true,
            [hand, Hand::default()],
            &scene,
            head,
            &[],
            gaze,
        );
        apply(&mut scene, &packet);
        let before = scene.windows[0].dome_offset_deg;
        hand.position[0] += 0.1;
        let packet = input.update_gaze_surfaces(
            1.02,
            true,
            [hand, Hand::default()],
            &scene,
            head,
            &[],
            gaze,
        );
        assert!(!packet
            .events
            .iter()
            .any(|e| matches!(e, SeatEvent::Button { pressed: true, .. })));
        apply(&mut scene, &packet);
        assert!(scene.windows[0].dome_offset_deg[0] > before[0]);
        let lost = input.update_gaze_surfaces(
            1.03,
            true,
            [hand, Hand::default()],
            &scene,
            head,
            &[],
            Hand::default(),
        );
        assert!(matches!(lost.events[0], SeatEvent::Reset));
    }
    #[test]
    fn grip_moves_window_and_suppresses_fist_pinches() {
        let (mut s, h, mut hand) = setup();
        let mut i = Interaction::default();
        i.update(1., true, [hand, Hand::default()], &s, h);
        hand.grip = true;
        hand.pinch = [true; 3];
        let p = i.update(1.01, true, [hand, Hand::default()], &s, h);
        apply(&mut s, &p);
        let old = s.windows[0].dome_offset_deg;
        hand.position[0] += 0.1;
        let p = i.update(1.02, true, [hand, Hand::default()], &s, h);
        assert!(!p
            .events
            .iter()
            .any(|e| matches!(e, SeatEvent::Button { pressed: true, .. })));
        apply(&mut s, &p);
        assert!(s.windows[0].dome_offset_deg[0] > old[0]);
        assert!(s.windows[0].xr_pose.is_none());
    }
    #[test]
    fn both_grips_scale_and_releasing_one_does_not_jump() {
        let (mut s, h, hand) = setup();
        let mut hands = [hand; 2];
        hands[0].position[0] -= 0.1;
        hands[1].position[0] += 0.1;
        let mut i = Interaction::default();
        i.update(1., true, hands, &s, h);
        hands.iter_mut().for_each(|h| h.grip = true);
        let p = i.update(1.01, true, hands, &s, h);
        apply(&mut s, &p);
        hands[0].position[0] -= 0.05;
        hands[1].position[0] += 0.05;
        let p = i.update(1.02, true, hands, &s, h);
        apply(&mut s, &p);
        assert!((s.windows[0].xr_scale - 1.5).abs() < 0.01);
        let before = s.windows[0].xr_pose;
        hands[1].grip = false;
        let p = i.update(1.03, true, hands, &s, h);
        apply(&mut s, &p);
        assert_eq!(s.windows[0].xr_pose, before);
        assert!((s.windows[0].xr_scale - 1.5).abs() < 0.01);
    }
    #[test]
    fn pointer_drag_remains_captured_outside_window() {
        let (s, h, mut hand) = setup();
        let mut i = Interaction::default();
        i.update(1., true, [hand, Hand::default()], &s, h);
        hand.pinch[0] = true;
        i.update(1.01, true, [hand, Hand::default()], &s, h);
        let q = quads(&s, h).remove(0);
        hand.ray = (q.point([1.2, 0.5]) - Vec3::from_array(hand.position))
            .normalize()
            .to_array();
        let p = i.update(1.02, true, [hand, Hand::default()], &s, h);
        assert!(p
            .events
            .iter()
            .any(|e| matches!(e,SeatEvent::Motion{id,uv}if *id==s.windows[0].id&&uv[0]>1.)));
        assert!(!p
            .events
            .iter()
            .any(|e| matches!(e, SeatEvent::Button { pressed: false, .. })));
    }
    #[test]
    fn gesture_debounce_hysteresis_and_fist_priority() {
        let mut d = Detector::default();
        d.gestures(0.8, [1.; 3], [1.; 4]);
        d.gestures(0.91, [1.; 3], [1.; 4]);
        d.gestures(1., [0.2, 1., 1.], [1.; 4]);
        d.gestures(1.04, [0.2, 1., 1.], [1.; 4]);
        assert_eq!(d.pinch, [false; 3]);
        d.gestures(1.05, [0.2, 1., 1.], [1.; 4]);
        assert_eq!(d.pinch, [true, false, false]);
        d.gestures(1.06, [0.35, 1., 1.], [1.; 4]);
        assert!(d.pinch[0]);
        d.gestures(1.07, [1.; 3], [1.; 4]);
        assert_eq!(d.pinch, [false; 3]);
        d.gestures(1.08, [0.1; 3], [0.3; 4]);
        d.gestures(1.17, [0.1; 3], [0.3; 4]);
        assert!(d.grip);
        assert_eq!(d.pinch, [false; 3]);
        d.gestures(1.18, [1.; 3], [1.; 4]);
        d.gestures(1.23, [1.; 3], [1.; 4]);
        assert!(d.grip);
        d.gestures(1.29, [1.; 3], [1.; 4]);
        assert!(!d.grip);
    }
    #[test]
    fn held_pinch_on_startup_and_after_tracking_loss_needs_open_hand() {
        let mut d = Detector::default();
        d.gestures(1., [0.1, 1., 1.], [1.; 4]);
        d.gestures(1.1, [0.1, 1., 1.], [1.; 4]);
        assert_eq!(d.pinch, [false; 3]);
        d.gestures(1.11, [1.; 3], [1.; 4]);
        d.gestures(1.22, [1.; 3], [1.; 4]);
        d.gestures(1.23, [0.1, 1., 1.], [1.; 4]);
        d.gestures(1.28, [0.1, 1., 1.], [1.; 4]);
        assert!(d.pinch[0]);
        assert!(!d.sample(1.29, None, false).valid);
        d.gestures(1.3, [0.1, 1., 1.], [1.; 4]);
        d.gestures(1.4, [0.1, 1., 1.], [1.; 4]);
        assert_eq!(d.pinch, [false; 3]);
    }
    #[test]
    fn ambiguous_pinches_and_extended_index_are_not_grip() {
        let mut d = Detector::default();
        d.gestures(0.8, [1.; 3], [1.; 4]);
        d.gestures(0.91, [1.; 3], [1.; 4]);
        d.gestures(1., [0.1, 0.1, 1.], [1.; 4]);
        d.gestures(1.1, [0.1, 0.1, 1.], [1.; 4]);
        assert_eq!(d.pinch, [false; 3]);
        d.gestures(1.11, [1.; 3], [1., 0.3, 0.3, 0.3]);
        d.gestures(1.21, [1.; 3], [1., 0.3, 0.3, 0.3]);
        assert!(!d.grip);
    }
    #[test]
    fn timeout_and_missing_grab_target_cancel() {
        let (mut s, h, mut hand) = setup();
        let mut i = Interaction::default();
        i.update(1., true, [hand, Hand::default()], &s, h);
        hand.grip = true;
        i.update(1.01, true, [hand, Hand::default()], &s, h);
        assert!(matches!(
            i.update(1.3, true, [hand, Hand::default()], &s, h).events[0],
            SeatEvent::Reset
        ));
        hand.grip = false;
        i.update(1.31, true, [hand, Hand::default()], &s, h);
        hand.grip = true;
        i.update(1.32, true, [hand, Hand::default()], &s, h);
        s.windows.clear();
        assert!(matches!(
            i.update(1.33, true, [hand, Hand::default()], &s, h).events[0],
            SeatEvent::Reset
        ));
    }
}

/// Draw inside the content extent: borders never add to the packing gap or hit bounds.
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
