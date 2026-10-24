use dome::{xr_input, xr_presentation::Frame, xr_transport::{ShellLayout, NOTIFICATIONS, TOP_BAR, TASK_BAR, TOP_PANEL, BOTTOM_PANEL, ShellPullout}, AnchorMode, Pose, Scene};
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
fn expanded_controls_stay_in_both_eyes_after_head_motion_and_during_visibility_transitions() {
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
                            notifications: Some([890, 48, 360, 650]), pullout: Some(ShellPullout { top_attached: false, anchor: head, viewport }) };
                        let original = xr_input::shell_quads_in_fov(&scene, head, &layout, viewport);
                        for amount in [0.01, 0.5, 1.] {
                            let frame = Frame { top: amount, bottom: amount, opacity: 1. };
                            let mut geometry = original.clone(); frame.apply(&mut geometry, head, &scene.config);
                            assert_eq!(original.len(), geometry.len());
                            for q in geometry.iter().filter(|q| q.id == NOTIFICATIONS
                                || (q.id == TOP_PANEL && top > 36)
                                || (q.id == BOTTOM_PANEL && bottom > 60)) {
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
        notifications: Some([900, 48, 360, 180]), pullout: None };
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
    let horizontal_center = (viewport[0] + 3_f32.to_radians()).max(-40_f32.to_radians()).tan()
        + (viewport[1] - 3_f32.to_radians()).min(40_f32.to_radians()).tan();
    assert!((center.x / -center.z - horizontal_center / 2.).abs() < 0.001);
}

#[test]
fn invalid_or_missing_optics_never_create_an_unbounded_shell() {
    let head = Pose::default();
    assert!(xr_input::shared_viewport(head, &[], 0.3).is_none());
    assert!(xr_input::shared_viewport(head, &eyes(head, [f32::NAN, 0.7, -0.6, 0.6]), 0.3).is_none());
    let layout = ShellLayout { width: 1280, height: 800, top: 36, bottom: 60, notifications: None, pullout: None };
    assert!(xr_input::shell_quads_in_fov(&Scene::new("invalid"), head, &layout, [0.; 4]).is_empty());
    assert_eq!(Frame { top: 0.5, bottom: 0.25, opacity: 0.8 }.surface_opacity(TOP_BAR), 0.4);
}

#[test]
fn expanded_controls_remain_in_central_view_and_every_control_is_mouse_reachable() {
    // Binocular viewport from the affected live WiVRn session, plus wider and
    // narrower optics. Optical containment alone missed the peripheral bars.
    for viewport in [
        [-0.6880466, 0.6880466, -0.9326899, 0.7228465],
        [-1.1, 1.2, -1.15, 0.95],
        [-0.4, 0.45, -0.32, 0.4],
    ] {
        for pitch in [-1.0, 0., 1.0] {
            let head = Pose { position_m: [0.8, 1.6, -0.3],
                orientation_xyzw: (Quat::from_rotation_y(1.2) * Quat::from_rotation_x(pitch)
                    * Quat::from_rotation_z(0.3)).to_array() };
            let mut scene = Scene::new("reachable shell");
            scene.add("overlapping application", "fixture");
            for (top, bottom) in [(36, 60), (456, 60), (36, 390), (768, 60)] {
                let shell = ShellLayout { width: 1024, height: 768, top, bottom, notifications: None, pullout: Some(ShellPullout { top_attached: false, anchor: head, viewport }) };
                let bars = xr_input::shell_quads_in_fov(&scene, head, &shell, viewport);
                let mut surfaces = xr_input::quads_in_fov(&scene, head, Some(viewport));
                surfaces.extend(bars.clone());
                for bar in bars.iter().filter(|q| (q.id == TOP_PANEL && top > 36)
                    || (q.id == BOTTOM_PANEL && bottom > 60)) {
                    for u in 0..=24 { for v in 0..=8 {
                        let uv = [u as f32 / 24., v as f32 / 8.];
                        let point = bar.point(uv);
                        let p = local(head, point);
                        assert!((p.x / -p.z).atan().abs() <= 40_f32.to_radians() + 0.0001);
                        assert!((p.y / -p.z).atan().abs() <= 30_f32.to_radians() + 0.0001,
                            "bar {} at optical edge: {:?}", bar.id, p);
                        let original = xr_input::mouse_angles_at(head, point);
                        let mut angles = original;
                        xr_input::confine_mouse(&mut angles, viewport);
                        assert!((angles[0] - original[0]).abs() < 0.0001);
                        assert!((angles[1] - original[1]).abs() < 0.0001);
                        let ray = xr_input::mouse_ray(head, angles);
                        let hit = bar.hit(Vec3::from_array(head.position_m), ray, false).unwrap();
                        assert!((hit[0] - uv[0]).abs() < 0.001 && (hit[1] - uv[1]).abs() < 0.001);
                        // Expanded panels may overlap the other bar, but chrome
                        // must always win against the application underneath.
                        let target = xr_input::mouse_target(&surfaces,
                            Vec3::from_array(head.position_m), ray, None, false).unwrap();
                        assert!(dome::xr_transport::is_shell(target.0));
                    }}
                }
            }
        }
    }
}

#[test]
fn rendered_idle_bars_preserve_dome_edges_when_head_pitches_rolls_or_moves() {
    let mut scene = Scene::new("dome anchored bars");
    scene.config.anchor_mode = AnchorMode::Room;
    scene.config.center_m = [0.8, 1.6, -0.3];
    for lane in [0, 1, 2] {
        let id = scene.add("lane", "fixture");
        scene.apply(dome::Action::Move { id, workspace: 0, lane }).unwrap();
    }
    let shell = ShellLayout { width: 1024, height: 768, top: 36, bottom: 60, notifications: None, pullout: None };
    let viewport = [-0.6880466, 0.6880466, -0.9326899, 0.7228465];
    for yaw in [-2.4, 0., 1.5] {
        let level = Pose { position_m: scene.config.center_m,
            orientation_xyzw: Quat::from_rotation_y(yaw).to_array() };
        let original = xr_input::shell_quads_in_fov(&scene, level, &shell, viewport);
        for pitch in [-1.0, 0., 1.0] { for roll in [-0.4, 0., 0.4] {
            let head = Pose { position_m: [1.2, 1.9, -0.5],
                orientation_xyzw: (Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch)
                    * Quat::from_rotation_z(roll)).to_array() };
            let actual = xr_input::shell_quads_in_fov(&scene, head, &shell, viewport);
            for (a, b) in original.iter().zip(&actual) {
                for uv in [[0., 0.], [0.5, 0.5], [1., 1.]] {
                    assert!(a.point(uv).distance(b.point(uv)) < 0.0001,
                        "idle bar {} detached from dome at pitch {pitch}, roll {roll}", a.id);
                }
            }
            for (top, bottom, unchanged) in [(456, 60, TASK_BAR), (36, 390, TOP_BAR)] {
                let expanded = ShellLayout { top, bottom, ..shell.clone() };
                let opened = xr_input::shell_quads_in_fov(&scene, head, &expanded, viewport);
                let a = actual.iter().find(|q| q.id == unchanged).unwrap();
                let b = opened.iter().find(|q| q.id == unchanged).unwrap();
                assert!(a.point([0.5, 0.5]).distance(b.point([0.5, 0.5])) < 0.0001);
            }
        }}
        // Looking towards a resting bar brings it into the mouse's optical
        // viewport; the bar must not retreat with the viewer's pitch.
        for bar in &original {
            let point = bar.point([0.5, 0.5]);
            let delta = point - Vec3::from_array(level.position_m);
            let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, delta.normalize());
            let looking = Pose { orientation_xyzw: rotation.to_array(), ..level };
            let bars = xr_input::shell_quads_in_fov(&scene, looking, &shell, viewport);
            let target = bars.iter().find(|q| q.id == bar.id).unwrap();
            let point = target.point([0.5, 0.5]);
            let mut angles = xr_input::mouse_angles_at(looking, point);
            let original_angles = angles;
            xr_input::confine_mouse(&mut angles, viewport);
            assert!((angles[0] - original_angles[0]).abs() < 0.0001);
            assert!((angles[1] - original_angles[1]).abs() < 0.0001);
            assert!(target.hit(Vec3::from_array(looking.position_m),
                xr_input::mouse_ray(looking, angles), false).is_some());
        }
    }
}

#[test]
fn pulled_menus_freeze_opening_pose_and_leave_both_bars_on_the_dome() {
    assert!(!dome::xr_transport::is_shell(xr_input::MOUSE_DOME));
    assert_ne!(TOP_PANEL, BOTTOM_PANEL);
    let scene = Scene::new("pulled menus");
    let viewport = [-0.68, 0.68, -0.93, 0.72];
    let opening = Pose { position_m: scene.config.center_m,
        orientation_xyzw: (Quat::from_rotation_y(0.6) * Quat::from_rotation_x(0.4)).to_array() };
    let idle = ShellLayout { width: 1024, height: 768, top: 36, bottom: 60,
        notifications: None, pullout: None };
    let mut shell = ShellLayout { top: 456, bottom: 390,
        pullout: Some(ShellPullout { top_attached: false, anchor: opening.upright(), viewport }), ..idle.clone() };
    for top in [456, 768] {
        shell.top = top;
        let original = xr_input::shell_quads_in_fov(&scene, opening, &shell, viewport);
        for pitch in [-0.8, 0., 0.8] {
            let moved = Pose { position_m: [1.4, 2.1, -0.5],
                orientation_xyzw: (Quat::from_rotation_y(-1.2) * Quat::from_rotation_x(pitch)
                    * Quat::from_rotation_z(0.4)).to_array() };
            let quads = xr_input::shell_quads_in_fov(&scene, moved, &shell, viewport);
            let bars = xr_input::shell_quads_in_fov(&scene, moved, &idle, viewport);
            for id in [TOP_BAR, TASK_BAR] {
                let expected = bars.iter().find(|q| q.id == id).unwrap();
                let actual = quads.iter().find(|q| q.id == id).unwrap();
                for uv in [[0.,0.], [0.5,0.5], [1.,1.]] {
                    assert!(expected.point(uv).distance(actual.point(uv)) < 0.0001);
                }
            }
            for id in [TOP_PANEL, BOTTOM_PANEL] {
                let before = original.iter().find(|q| q.id == id).unwrap();
                let after = quads.iter().find(|q| q.id == id).unwrap();
                assert!(before.point([0.5,0.5]).distance(after.point([0.5,0.5])) < 0.0001);
                assert_eq!(before.size, after.size);
                assert!(dome::xr_presentation::Presentation::default().interactive(id));
                assert_eq!(Frame::default().surface_opacity(id), 1.);
            }
        }
        assert_eq!(shell.surface_rect(TOP_BAR), Some([0,0,1024,36]));
        assert_eq!(shell.surface_rect(TASK_BAR), Some([0,708,1024,60]));
        assert_eq!(shell.surface_rect(TOP_PANEL), Some([0,36,1024,top.min(708)-36]));
        assert_eq!(shell.surface_rect(BOTTOM_PANEL), Some([0,378,1024,330]));
        assert!(idle.surface_rect(TOP_PANEL).is_none());
    }
}

#[test]
fn universal_dropdown_stays_below_its_menu_bar_instead_of_using_pullout() {
    let mut scene=Scene::new("anchored menu");
    scene.config.dome_upper_tilt_deg=25.;
    let head=Pose::default();
    let viewport=[-0.7,0.7,-0.65,0.65];
    let shell=ShellLayout{width:1024,height:768,top:740,bottom:60,notifications:None,
        pullout:Some(ShellPullout{top_attached:true,anchor:head,viewport})};
    for yaw in [0.,0.6] {
        let moved=Pose{orientation_xyzw:Quat::from_rotation_y(yaw).to_array(),..head};
        let quads=xr_input::shell_quads_in_fov(&scene,moved,&shell,viewport);
        let bar=quads.iter().find(|q|q.id==TOP_BAR).unwrap();
        let panel=quads.iter().find(|q|q.id==TOP_PANEL).unwrap();
        let a=bar.dome.as_ref().unwrap();let b=panel.dome.as_ref().unwrap();
        assert_eq!(a.frame,b.frame);
        assert!((a.horizontal-b.horizontal).abs()<1e-6);
        assert!((b.top-a.bottom+0.001).abs()<1e-6);
        let bottom=quads.iter().find(|q|q.id==TASK_BAR).unwrap();
        let frame=moved.upright();
        let slope=|point:Vec3|{let p=frame.inverse_rotate((point-Vec3::from_array(frame.position_m)).to_array());p[1]/p[0].hypot(p[2])};
        let lower=[0.,0.5,1.].map(|u|slope(bottom.point([u,0.]))).into_iter().fold(f32::NEG_INFINITY,f32::max);
        for u in [0.,0.25,0.5,0.75,1.] {assert!(slope(panel.point([u,1.]))>=lower-1e-5);}
        let origin=Vec3::from_array(moved.position_m);
        let p=panel.point([0.3,0.6]);
        let uv=panel.hit(origin,(p-origin).normalize(),false).unwrap();
        assert!((uv[0]-0.3).abs()<1e-4 && (uv[1]-0.6).abs()<1e-4);
    }
}
