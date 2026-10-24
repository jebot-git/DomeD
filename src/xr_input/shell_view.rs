//! Dome-attached bars with optical fitting for expanded controls and notifications.
use super::{shell_quads_in_view, DomePatch, Quad};
use crate::{xr_transport::{ShellLayout, NOTIFICATIONS, TASK_BAR, TOP_BAR, TOP_PANEL, BOTTOM_PANEL}, Pose, Scene};
use glam::{Quat, Vec3};

/// Conservative head-relative FOV shared by all eyes, including eye offsets,
/// canted optics and roll. Every point inside this cone at or beyond `near`
/// lies inside each eye's projection frustum.
pub fn shared_viewport(head: Pose, eyes: &[(Pose, [f32; 4])], near: f32) -> Option<[f32; 4]> {
    if eyes.is_empty() || !near.is_finite() || near <= 0. { return None; }
    head.validate().ok()?;
    let mut angles = [-1.56_f32, 1.56, -1.56, 1.56];
    for (eye, fov) in eyes {
        eye.validate().ok()?;
        if !valid(*fov) { return None; }
        angles = [angles[0].max(fov[0]), angles[1].min(fov[1]),
            angles[2].max(fov[2]), angles[3].min(fov[3])];
    }
    let slopes = angles.map(f32::tan);
    let inverse = Quat::from_array(head.orientation_xyzw).conjugate();
    let mut scale = 1_f32;
    for (eye, fov) in eyes {
        let rotation = inverse * Quat::from_array(eye.orientation_xyzw);
        let offset = inverse * (Vec3::from_array(eye.position_m) - Vec3::from_array(head.position_m));
        let [left, right, down, up] = fov.map(f32::tan);
        for normal in [Vec3::new(1., 0., left), Vec3::new(-1., 0., -right),
            Vec3::new(0., 1., down), Vec3::new(0., -1., -up)] {
            let n = rotation * normal;
            let lateral = n.x * if n.x >= 0. { slopes[0] } else { slopes[1] }
                + n.y * if n.y >= 0. { slopes[2] } else { slopes[3] };
            let budget = -n.z - (n.dot(offset) / near).max(0.);
            if budget <= 0. { return None; }
            if lateral < 0. { scale = scale.min(budget / -lateral); }
        }
    }
    let viewport = slopes.map(|s| (s * scale.min(1.) * 0.999).atan());
    valid(viewport).then_some(viewport)
}

fn valid([left, right, down, up]: [f32; 4]) -> bool {
    [left, right, down, up].iter().all(|v| v.is_finite() && v.abs() < 1.57)
        && left < 0. && right > 0. && down < 0. && up > 0.
}

fn inset(fov: [f32; 4]) -> [f32; 4] {
    let margin = 3_f32.to_radians().min((fov[1] - fov[0]).min(fov[3] - fov[2]) * 0.08);
    // Optical visibility extends well beyond a comfortable control area on
    // wide/asymmetric headsets. Edge-aligning chrome against that entire FOV
    // pins it to the lens rim; following head pitch then prevents looking at it.
    // Keep opened controls in the central view, while retaining the smaller
    // binocular bounds on narrow headsets. Input uses this same fitted geometry.
    let horizontal = 40_f32.to_radians();
    let vertical = 30_f32.to_radians();
    [(fov[0] + margin).max(-horizontal), (fov[1] - margin).min(horizontal),
        (fov[2] + margin).max(-vertical), (fov[3] - margin).min(vertical)]
}

/// Exact support bounds of the whole flat/cylindrical patch, not just corners:
/// curved edges can have their most exposed point between the corners.
fn support(q: &Quad, head: Pose, center: Vec3, normal: Vec3) -> [f32; 2] {
    let inverse = Quat::from_array(head.orientation_xyzw).conjugate();
    if let Some(d) = &q.dome {
        let n = (inverse * Quat::from_array(d.frame.orientation_xyzw)).conjugate() * normal;
        let base = (inverse * (Vec3::from_array(d.frame.position_m) - center)).dot(normal);
        let half = d.horizontal / 2.;
        let curve = |a: f32| n.x * a.sin() - n.z * a.cos();
        let mut lo = curve(-half).min(curve(half));
        let mut hi = curve(-half).max(curve(half));
        let extremum = (-n.x).atan2(n.z);
        for turn in -2..=2 {
            let angle = extremum + turn as f32 * std::f32::consts::PI;
            if (-half..=half).contains(&angle) {
                lo = lo.min(curve(angle)); hi = hi.max(curve(angle));
            }
        }
        [base + d.radius * (lo + (n.y * d.bottom).min(n.y * d.top)),
            base + d.radius * (hi + (n.y * d.bottom).max(n.y * d.top))]
    } else {
        let n = (inverse * Quat::from_array(q.pose.orientation_xyzw)).conjugate() * normal;
        let extent = n.x.abs() * q.size[0] / 2. + n.y.abs() * q.size[1] / 2.;
        [-extent, extent]
    }
}

fn fit(q: &mut Quad, head: Pose, fov: [f32; 4], near: f32) {
    let [left, right, down, up] = fov.map(f32::tan);
    let center = q.point([0.5, 0.5]);
    let local = Vec3::from_array(head.inverse_rotate((center - Vec3::from_array(head.position_m)).to_array()));
    let z = support(q, head, center, Vec3::Z)[1].max(0.);
    let depth = (-local.z).max(near + z);
    let xmin = support(q, head, center, Vec3::new(1., 0., left))[0];
    let xmax = support(q, head, center, Vec3::new(1., 0., right))[1];
    let ymin = support(q, head, center, Vec3::new(0., 1., down))[0];
    let ymax = support(q, head, center, Vec3::new(0., 1., up))[1];
    let scale = (depth * (right - left) / (xmax - xmin).max(f32::EPSILON))
        .min(depth * (up - down) / (ymax - ymin).max(f32::EPSILON))
        .clamp(0., 1.) * 0.999;
    let x = local.x.clamp(left * depth - scale * xmin, right * depth - scale * xmax);
    let bottom = down * depth - scale * ymin;
    let top = up * depth - scale * ymax;
    let y = match q.id { TOP_BAR => top, TASK_BAR => bottom, _ => local.y.clamp(bottom, top) };
    let target = Vec3::from_array(head.transform([x, y, -depth]));
    q.size = q.size.map(|s| s * scale);
    if let Some(d) = &mut q.dome {
        d.frame.position_m = (target + scale * (Vec3::from_array(d.frame.position_m) - center)).to_array();
        d.radius *= scale;
        q.pose = d.layer_pose();
    } else { q.pose.position_m = target.to_array(); }
}

/// Rendering and hit testing share dome bars and frozen, pulled-out menu panels.
pub fn shell_quads_in_fov(scene: &Scene, head: Pose, shell: &ShellLayout, viewport: [f32; 4]) -> Vec<Quad> {
    if !valid(viewport) { return vec![]; }
    let safe = inset(viewport);
    let near = scene.config.bar_distance_m.min(scene.config.radius_m).max(0.1);
    let mut resting = shell.clone();
    if shell.pullout.is_some() {
        resting.top = shell.top.min(36);
        resting.bottom = shell.bottom.min(60);
    }
    let mut quads = shell_quads_in_view(scene, head, &resting, (safe[1] - safe[0]).to_degrees());
    quads.retain(|q| q.size.iter().all(|s| s.is_finite() && *s > 0.));
    if let Some(pulled) = &shell.pullout {
        if !scene.locked && pulled.anchor.validate().is_ok() && valid(pulled.viewport) {
            for id in [TOP_PANEL, BOTTOM_PANEL] {
                if let Some([_, _, w, h]) = shell.surface_rect(id).filter(|r| r[2] > 0 && r[3] > 0) {
                    let radius = scene.config.pullout_distance_m;
                    let width = 1.3_f32.min(radius * 85_f32.to_radians());
                    let height = width * h as f32 / w as f32;
                    let patch = DomePatch {
                        frame: Pose { position_m: pulled.anchor.transform([0., -0.06, 0.]), ..pulled.anchor },
                        radius, horizontal: width / radius,
                        top: height / (2. * radius), bottom: -height / (2. * radius),
                    };
                    let mut panel = Quad { id, pose: patch.layer_pose(),
                        size: [width, height], dome: Some(patch) };
                    // Fit once against the frozen opening pose/optics, exactly
                    // as a pulled window stays put after head movement.
                    fit(&mut panel, pulled.anchor, inset(pulled.viewport), radius.min(near));
                    quads.push(panel);
                }
            }
        }
    }
    let mut bottom_angle = safe[2] + (8_f32.to_radians()).min((safe[3] - safe[2]) * 0.15);
    if let Some(bar) = quads.iter().find(|q| q.id == TASK_BAR) {
        // These are the exact depth extrema of the view-aligned cylinder.
        for u in [0., 0.5, 1.] {
            let p = Vec3::from_array(head.inverse_rotate((bar.point([u, 0.]) - Vec3::from_array(head.position_m)).to_array()));
            bottom_angle = bottom_angle.max(p.y.atan2(-p.z) + 3_f32.to_radians());
        }
    }
    if let Some(q) = quads.iter_mut().find(|q| q.id == NOTIFICATIONS) {
        let depth = scene.config.bar_distance_m;
        let bottom = bottom_angle.min(safe[3] - (safe[3] - safe[2]) * 0.1).tan() * depth;
        let width = depth * (safe[1].tan() - safe[0].tan());
        let height = depth * safe[3].tan() - bottom;
        let scale = (width / q.size[0]).min(height / q.size[1]).min(1.) * 0.999;
        q.size = q.size.map(|s| s * scale);
        q.pose = Pose { position_m: head.transform([
            depth * (safe[0].tan() + safe[1].tan()) / 2., bottom + q.size[1] / 2., -depth,
        ]), ..head };
        fit(q, head, safe, near);
    }
    quads
}
