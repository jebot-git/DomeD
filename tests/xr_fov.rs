use dome::{xr_input, xr_presentation::Frame, xr_transport::{ShellLayout, NOTIFICATIONS, TOP_BAR, TASK_BAR}, AnchorMode, Pose, Scene};
use glam::{Quat, Vec3};

fn local(head: Pose, point: Vec3) -> Vec3 {
    Vec3::from_array(head.inverse_rotate((point - Vec3::from_array(head.position_m)).to_array()))
}

fn eyes(head: Pose, fov: [f32; 4]) -> Vec<(Pose, [f32; 4])> {
    [-1., 1.].map(|side| (Pose {
        position_m: head.transform([side * 0.036, 0., 0.01]),
        orientation_xyzw: (Quat::from_array(head.orientation_xyzw)
            * Quat::from_rotation_y(side * 0.12) * Quat::from_rotation_z(side * 0.04)).to_array(),
    }, fov)).to_vec()
}

#[test]
fn complete_shell_stays_in_both_eyes_after_head_motion_and_during_visibility_transitions() {
    for anchor in [AnchorMode::Room, AnchorMode::Head] {
        for pitch in [-1.3, 0., 1.3] {
            let head = Pose { position_m: [2.1, 1.8, -3.2],
                orientation_xyzw: (Quat::from_rotation_y(2.4) * Quat::from_rotation_x(pitch)
                    * Quat::from_rotation_z(0.45)).to_array() };
            for fov in [[-0.4, 0.55, -0.32, 0.45], [-0.85, 0.9, -0.75, 0.8], [-1.2, 1.1, -1., 1.1]] {
                for distance in [0.3, 0.6, 2.] {
                    let mut scene = Scene::new("FOV"); scene.config.anchor_mode = anchor;
                    scene.config.bar_distance_m = distance;
                    scene.config.upper_bar_height_deg = 20.; scene.config.taskbar_height_deg = 20.;
                    scene.add("Window", "fixture");
                    let eyes = eyes(head, fov);
                    let near = distance.min(scene.config.radius_m);
                    let viewport = xr_input::shared_viewport(head, &eyes, near).unwrap();
                    for (top, bottom) in [(36, 60), (320, 350), (800, 0)] {
                        let layout = ShellLayout { width: 1280, height: 800, top, bottom,
                            notifications: Some([890, 48, 360, 650]) };
                        let original = xr_input::shell_quads_in_fov(&scene, head, &layout, viewport);
                        for amount in [0.01, 0.5, 1.] {
                            let frame = Frame { top: amount, bottom: amount, opacity: 1. };
                            let mut geometry = original.clone(); frame.apply(&mut geometry, head, &scene.config);
                            assert_eq!(original.len(), geometry.len());
                            for q in &geometry {
                                assert!(frame.surface_opacity(q.id) > 0.);
                                for u in 0..=32 { for v in 0..=8 {
                                    let uv = [u as f32 / 32., v as f32 / 8.];
                                    let point = q.point(uv);
                                    let head_point = local(head, point);
                                    assert!(-head_point.z >= near - 0.0001);
                                    for (eye, _) in &eyes {
                                        let p = local(*eye, point);
                                        assert!(p.z < 0.);
                                        let x = p.x / -p.z; let y = p.y / -p.z;
                                        assert!(x >= fov[0].tan() - 0.0001 && x <= fov[1].tan() + 0.0001,
                                            "surface {} clipped horizontally: {x}, {fov:?}", q.id);
                                        assert!(y >= fov[2].tan() - 0.0001 && y <= fov[3].tan() + 0.0001,
                                            "surface {} clipped vertically: {y}, {fov:?}", q.id);
                                    }
                                }}
                                for uv in [[0.01, 0.01], [0.5, 0.5], [0.99, 0.99]] {
                                    let point = q.point(uv);
                                    let ray = (point - Vec3::from_array(head.position_m)).normalize();
                                    let hit = q.hit(Vec3::from_array(head.position_m), ray, false).unwrap();
                                    assert!((hit[0] - uv[0]).abs() < 0.001 && (hit[1] - uv[1]).abs() < 0.001);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn notifications_are_bottom_centered_above_the_taskbar_regardless_of_desktop_crop() {
    let scene = Scene::new("notifications");
    let head = Pose::default();
    let viewport = [-0.7, 0.9, -0.65, 0.8];
    let mut layout = ShellLayout { width: 1280, height: 800, top: 36, bottom: 60,
        notifications: Some([900, 48, 360, 180]) };
    let geometry = xr_input::shell_quads_in_fov(&scene, head, &layout, viewport);
    let n = geometry.iter().find(|q| q.id == NOTIFICATIONS).unwrap();
    let center = local(head, n.point([0.5, 0.5]));
    let bottom = local(head, n.point([0.5, 1.]));
    assert!(bottom.y < 0.);
    assert!(bottom.y.atan2(-bottom.z) > viewport[2] + 0.08);
    assert!((n.size[0] / n.size[1] - 2.).abs() < 0.001);
    let task = geometry.iter().find(|q| q.id == TASK_BAR).unwrap();
    for u in [0., 0.5, 1.] {
        let p = local(head, task.point([u, 0.]));
        assert!(bottom.y.atan2(-bottom.z) > p.y.atan2(-p.z) + 0.03);
    }
    layout.notifications = Some([0, 500, 360, 180]);
    let moved = xr_input::shell_quads_in_fov(&scene, head, &layout, viewport);
    let other = moved.iter().find(|q| q.id == NOTIFICATIONS).unwrap();
    assert_eq!(n.pose, other.pose); assert_eq!(n.size, other.size);
    let horizontal_center = (viewport[0] + 3_f32.to_radians()).tan()
        + (viewport[1] - 3_f32.to_radians()).tan();
    assert!((center.x / -center.z - horizontal_center / 2.).abs() < 0.001);
}

#[test]
fn invalid_or_missing_optics_never_create_an_unbounded_shell() {
    let head = Pose::default();
    assert!(xr_input::shared_viewport(head, &[], 0.3).is_none());
    assert!(xr_input::shared_viewport(head, &eyes(head, [f32::NAN, 0.7, -0.6, 0.6]), 0.3).is_none());
    let layout = ShellLayout { width: 1280, height: 800, top: 36, bottom: 60, notifications: None };
    assert!(xr_input::shell_quads_in_fov(&Scene::new("invalid"), head, &layout, [0.; 4]).is_empty());
    assert_eq!(Frame { top: 0.5, bottom: 0.25, opacity: 0.8 }.surface_opacity(TOP_BAR), 0.4);
}
