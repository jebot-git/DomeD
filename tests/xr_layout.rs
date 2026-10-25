use dome::{
    xr_input::{self},
    *,
};
use glam::{Quat, Vec3};
fn scene() -> Scene {
    let mut s = Scene::new("test");
    s.add("First", "app");
    s
}

#[test]
fn xr_shell_hits_match_overlay_order_and_modals_override_application_capture() {
    let app = xr_input::Quad {
        id: 1,
        pose: Pose { position_m: [0., 0., -0.5], ..Pose::default() },
        size: [4., 4.], dome: None,
    };
    for id in [dome::xr_transport::TOP_BAR, dome::xr_transport::TASK_BAR,
        dome::xr_transport::NOTIFICATIONS]
    {
        let overlay = xr_input::Quad {
            id, pose: Pose { position_m: [0., 0., -2.], ..Pose::default() },
            size: [1., 1.], dome: None,
        };
        let surfaces = [app.clone(), overlay];
        assert_eq!(xr_input::mouse_target(&surfaces, Vec3::ZERO, -Vec3::Z, None, false).unwrap().0, id);
        assert_eq!(xr_input::mouse_target(&surfaces, Vec3::ZERO, -Vec3::Z, Some(1), false).unwrap().0, 1);
        if id == dome::xr_transport::TOP_BAR {
            assert_eq!(xr_input::mouse_target(&surfaces, Vec3::ZERO, -Vec3::Z, Some(1), true).unwrap().0, id);
            let outside = Vec3::new(1., 0., -1.).normalize();
            assert!(xr_input::mouse_target(&surfaces, Vec3::ZERO, outside, None, true).is_none());
            assert_eq!(xr_input::mouse_target(&surfaces, Vec3::ZERO, outside, None, false).unwrap().0, 1);
        }
    }
}
#[test]
fn dome_curvature_and_mouse_uv_use_identical_open_cylinder() {
    let s = scene();
    let q = xr_input::quads(&s, Pose::default()).remove(0);
    let d = q.dome.as_ref().unwrap();
    let center = Vec3::from_array(d.frame.position_m);
    for uv in [[0.1, 0.1], [0.5, 0.5], [0.9, 0.9], [0.1, 0.9]] {
        let p = q.point(uv);
        let local = d.frame.inverse_rotate((p - center).to_array());
        assert!((local[0].hypot(local[2]) - d.radius).abs() < 0.0001);
        let hit = q.hit(center, (p - center).normalize(), false).unwrap();
        assert!((uv[0] - hit[0]).abs() < 0.0001 && (uv[1] - hit[1]).abs() < 0.0001);
    }
    let normal = Quat::from_array(q.pose.orientation_xyzw) * Vec3::Z;
    assert!((q.point([0., 0.5]) - q.point([0.5, 0.5])).dot(normal) > 0.05);
}
#[test]
fn dome_is_curved_and_room_anchored_by_default() {
    let s = scene();
    let head = Pose::default();
    let q = xr_input::quads(&s, head).remove(0);
    assert!(q.dome.is_some());
    let moved = xr_input::quads(
        &s,
        Pose {
            position_m: [1., 1.4, 0.],
            ..head
        },
    )
    .remove(0);
    assert_eq!(moved.pose, q.pose);
}

#[test]
fn two_slots_are_world_anchored_and_third_pull_is_atomic() {
    let mut s = scene();
    let a = s.windows[0].id;
    let b = s.add("Second", "app");
    let c = s.add("Third", "app");
    let head = Pose {
        position_m: [0., 1.6, 0.],
        ..Pose::default()
    };
    s.apply(Action::PullOut { id: a, head }).unwrap();
    s.apply(Action::PullOut { id: b, head }).unwrap();
    let revision = s.revision;
    assert!(s.apply(Action::PullOut { id: c, head }).is_err());
    assert_eq!(s.revision, revision);
    let before = xr_input::quads(&s, head);
    let after = xr_input::quads(
        &s,
        Pose {
            position_m: [1., 1.6, 1.],
            ..head
        },
    );
    for id in [a, b] {
        let q = before.iter().find(|q| q.id == id).unwrap();
        assert!(q.dome.is_some());
        assert_eq!(q.pose, after.iter().find(|q| q.id == id).unwrap().pose);
        let p = q.point([0.5, 0.5]);
        assert!((p.x.hypot(p.z) - 2.).abs() < 0.001);
    }
    let left = before.iter().find(|q| q.id == a).unwrap();
    let right = before.iter().find(|q| q.id == b).unwrap();
    let pixel = 1.3 / s.window(a).unwrap().desktop.w;
    assert!((right.point([0., 0.5]).x - left.point([1., 0.5]).x - 8. * pixel).abs() < 0.0001);
    assert!((left.size[0] - 1.3).abs() < 0.001);
    // Curvature is visible yet shallow across each front panel.
    let normal = left
        .dome
        .as_ref()
        .unwrap()
        .inward_normal(left.point([0.5, 0.5]));
    let sag = (left.point([0., 0.5]) - left.point([0.5, 0.5])).dot(normal);
    assert!((0.05..0.15).contains(&sag));
    s.apply(Action::ReturnToDome { id: a }).unwrap();
    s.apply(Action::PullOut { id: c, head }).unwrap();
    s.apply(Action::Close { id: b }).unwrap();
    s.apply(Action::ReturnToDome { id: c }).unwrap();
    assert!(s.pullout_anchor.is_none());
}
#[test]
fn legacy_projection_migration_preserves_pullout() {
    let mut s = scene();
    let id = s.windows[0].id;
    s.apply(Action::PullOut {
        id,
        head: Pose::default(),
    })
    .unwrap();
    let mut value = serde_json::to_value(&s.config).unwrap();
    value["projection"] = serde_json::json!("in_view");
    let config = serde_json::from_value(value).unwrap();
    s.apply(Action::Configure { config }).unwrap();
    assert!(s.window(id).unwrap().pullout_slot.is_some());
    assert!(xr_input::quads(&s, Pose::default())
        .iter()
        .all(|q| q.dome.is_some()));
}

#[test]
fn dome_popup_growth_keeps_bar_edges_fixed() {
    let mut s = scene();
    s.config.projection = Projection::RoomDome;
    let layout = dome::xr_transport::ShellLayout {
        width: 1280,
        height: 800,
        top: 36,
        bottom: 60,
        notifications: None, pullout: None
    };
    let head = Pose::default();
    let bars = xr_input::shell_quads(&s, head, &layout);
    let expanded = xr_input::shell_quads(
        &s,
        head,
        &dome::xr_transport::ShellLayout {
            top: 300,
            bottom: 350,
            notifications: None,
            ..layout
        },
    );
    assert!(
        bars[0]
            .point([0.5, 0.])
            .distance(expanded[0].point([0.5, 0.]))
            < 0.0001
    );
    assert!(
        bars[1]
            .point([0.5, 1.])
            .distance(expanded[1].point([0.5, 1.]))
            < 0.0001
    );
}
#[test]
fn pulled_scaling_keeps_panels_side_by_side_and_restores_dome_size() {
    let mut s = scene();
    let a = s.windows[0].id;
    let b = s.add("two", "app");
    s.apply(Action::SpatialTransform {
        id: a,
        pose: None,
        scale: 0.6,
    })
    .unwrap();
    for id in [a, b] {
        s.apply(Action::PullOut {
            id,
            head: Pose::default(),
        })
        .unwrap();
    }
    s.apply(Action::SpatialTransform {
        id: a,
        pose: None,
        scale: 2.,
    })
    .unwrap();
    let q = xr_input::quads(&s, Pose::default());
    let left = q.iter().find(|q| q.id == a).unwrap();
    let right = q.iter().find(|q| q.id == b).unwrap();
    assert!(left.size[0] > 1.5);
    // Focus expands four pixels into the shared gap at the same angular pitch.
    let pixel = 1.3 / s.window(b).unwrap().desktop.w / s.config.pullout_distance_m;
    let l = left.point([1., 0.5]);
    let r = right.point([0., 0.5]);
    let gap = r.x.atan2(-r.z) - l.x.atan2(-l.z);
    assert!((gap - 4. * pixel).abs() < 0.00001);
    s.apply(Action::ReturnToDome { id: a }).unwrap();
    assert_eq!(s.window(a).unwrap().xr_scale, 0.6);
}
#[test]
fn radius_flattens_cards_and_compact_spacing_is_preserved() {
    let mut s = scene();
    s.focused = None;
    s.add("Second", "other");
    s.focused = None;
    let before = xr_input::quads(&s, Pose::default());
    let p = s.panels();
    let gap = (p[1].yaw_deg - p[0].yaw_deg).abs() - (p[0].width_deg + p[1].width_deg) / 2.;
    assert!((gap / (p[0].width_deg / s.windows[0].desktop.w) - 8.).abs() < 0.001);
    s.config.radius_m *= 2.;
    let after = xr_input::quads(&s, Pose::default());
    assert!((before[0].size[1] - after[0].size[1]).abs() < 0.001);
    let sag = |q: &xr_input::Quad| {
        let n = Quat::from_array(q.pose.orientation_xyzw) * Vec3::Z;
        (q.point([0., 0.5]) - q.point([0.5, 0.5])).dot(n)
    };
    assert!(sag(&after[0]) < sag(&before[0]) * 0.7);
    assert_eq!(after[0].dome.as_ref().unwrap().radius, s.config.radius_m);
}
#[test]
fn focused_panel_lifts_and_still_hits_its_displayed_surface() {
    let mut s = scene();
    let a = xr_input::quads(&s, Pose::default()).remove(0);
    s.focused = None;
    let b = xr_input::quads(&s, Pose::default()).remove(0);
    assert!(
        (b.dome.as_ref().unwrap().radius - a.dome.as_ref().unwrap().radius - 0.001).abs() < 0.0001
    );
    let base = b.dome.as_ref().unwrap();
    let raised = a.dome.as_ref().unwrap();
    assert!(
        ((raised.horizontal - base.horizontal) / (base.horizontal / s.windows[0].desktop.w) - 8.)
            .abs()
            < 0.001
    );
    assert!(
        ((raised.top - base.top) / ((base.top - base.bottom) / s.windows[0].desktop.h) - 4.).abs()
            < 0.001
    );
    let center = Vec3::from_array(a.dome.as_ref().unwrap().frame.position_m);
    let p = a.point([0.5, 0.5]);
    let uv = a.hit(center, (p - center).normalize(), false).unwrap();
    assert!((uv[0] - 0.5).abs() < 0.0001 && (uv[1] - 0.5).abs() < 0.0001);
}
#[test]
fn xr_contingency_promotes_existing_clients_once_to_primary_desktop() {
    let mut s = scene();
    let desktop = s.windows[0].id;
    let xr = s.add("XR only", "xr-app");
    s.windows[1].origin = Realm::Xr;
    s.windows[1].desktop_visible = false;
    s.apply(Action::Move {
        id: xr,
        lane: 2,
        workspace: 1,
    })
    .unwrap();
    s.apply(Action::Minimize { id: xr }).unwrap();
    assert!(s.desktop_scene().window(xr).is_none());
    assert!(s.desktop_scene().window(desktop).is_some());
    assert_eq!(s.desktop_fallback(1280., 800.), 1);
    let w = s.window(xr).unwrap();
    assert_eq!(w.origin, Realm::Xr);
    assert!(w.desktop_visible && !w.minimized);
    assert_eq!(w.workspace, 0);
    assert!(w.desktop.x + w.desktop.w <= 1280. && w.desktop.y + w.desktop.h <= 740.);
    assert_eq!(s.desktop_fallback(1280., 800.), 0);
}

#[test]
fn pixel_gaps_stay_bounded_for_different_sizes_radii_and_crowded_lanes() {
    for radius in [0.6, 2.2, 4.4, 8.] {
        for count in [2, 8, 40] {
            let mut s = Scene::new("test");
            s.config.radius_m = radius;
            for n in 0..count {
                let id = s.add("Window", "app");
                s.apply(Action::DesktopResize {
                    id,
                    w: 400. + n as f32 * 80.,
                    h: 300. + n as f32 * 25.,
                })
                .unwrap();
            }
            s.focused = None;
            let panels = s.panels();
            // Packing order can cross the angular seam. Compare a wrapped delta.
            let ids = &s.workspaces[0].lanes[1].windows;
            for pair in ids.windows(2) {
                let a = panels.iter().find(|p| p.id == pair[0]).unwrap();
                let b = panels.iter().find(|p| p.id == pair[1]).unwrap();
                let delta = (b.yaw_deg - a.yaw_deg).rem_euclid(360.);
                let gap = delta - (a.width_deg + b.width_deg) / 2.;
                for p in [a, b] {
                    let pixel = p.width_deg / s.window(p.id).unwrap().desktop.w;
                    assert!(gap / pixel <= 8.01 && gap / pixel >= -0.01);
                }
            }
        }
    }
}
#[test]
fn lane_gap_is_also_limited_to_eight_content_pixels() {
    let mut s = scene();
    for lane in [0, 2] {
        let id = s.add("Other lane", "app");
        s.apply(Action::Move {
            id,
            lane,
            workspace: 0,
        })
        .unwrap();
    }
    s.focused = None;
    for r in [0.6, 2.2, 4.4, 8.] {
        s.config.radius_m = r;
        let p = s.panels();
        let middle = p.iter().find(|p| p.lane == 1).unwrap();
        for lane in [0, 2] {
            let outer = p.iter().find(|p| p.lane == lane).unwrap();
            let gap = if lane == 0 {
                outer.bottom_deg - middle.top_deg
            } else {
                middle.bottom_deg - outer.top_deg
            };
            for panel in [outer, middle] {
                assert!(
                    gap / ((panel.top_deg - panel.bottom_deg)
                        / s.window(panel.id).unwrap().desktop.h)
                        <= 8.001
                );
            }
        }
    }
}
#[test]
fn borders_are_inside_content_and_never_exceed_two_pixels() {
    for thickness in [0, 1, 2, 8] {
        let mut pixels = vec![10; 20 * 12 * 4];
        xr_input::paint_border(&mut pixels, 20, 12, thickness);
        let actual = thickness.min(2) as usize;
        for y in 0..12 {
            for x in 0..20 {
                let border = x < actual || y < actual || x >= 20 - actual || y >= 12 - actual;
                assert_eq!(pixels[(y * 20 + x) * 4], if border { 146 } else { 10 });
            }
        }
    }
    let mut config = Config::default();
    config.window_border_px = 3;
    assert!(config.validate().is_err());
    config.window_border_px = 2;
    config.window_gap_px = 8.1;
    assert!(config.validate().is_err());
}

#[test]
fn focused_middle_lane_consumes_four_of_eight_gap_pixels() {
    let mut s = scene();
    let middle_id = s.windows[0].id;
    for lane in [0, 2] {
        let id = s.add("Other lane", "app");
        s.apply(Action::Move {
            id,
            lane,
            workspace: 0,
        })
        .unwrap();
    }
    s.focused = None;
    let before = xr_input::quads(&s, Pose::default());
    let middle = before
        .iter()
        .find(|q| q.id == middle_id)
        .unwrap()
        .dome
        .as_ref()
        .unwrap();
    let upper_id = s.windows.iter().find(|w| w.lane == 0).unwrap().id;
    let upper = before
        .iter()
        .find(|q| q.id == upper_id)
        .unwrap()
        .dome
        .as_ref()
        .unwrap();
    let gap = upper.bottom - middle.top;
    s.focused = Some(middle_id);
    let raised = xr_input::quads(&s, Pose::default());
    let raised = raised.iter().find(|q| q.id == middle_id).unwrap();
    assert!(((raised.dome.as_ref().unwrap().top - middle.top) / gap - 0.5).abs() < 0.0001);
    let point = raised.point([0.5, 0.]);
    let center = Vec3::from_array(raised.dome.as_ref().unwrap().frame.position_m);
    assert!(raised
        .hit(center, (point - center).normalize(), false)
        .is_some());
}

#[test]
fn room_dome_render_surfaces_use_pixel_spacing_in_every_lane() {
    for radius in [0.6, 2.2, 4.4, 8.] {
        for spacing in [0., 4., 8.] {
            let mut s = Scene::new("test");
            s.config.projection = Projection::RoomDome;
            s.config.radius_m = radius;
            s.config.window_gap_px = spacing;
            for lane in 0..3 {
                for n in 0..2 {
                    let id = s.add("Dome window", "app");
                    s.apply(Action::Move {
                        id,
                        lane,
                        workspace: 0,
                    })
                    .unwrap();
                    s.apply(Action::DesktopResize {
                        id,
                        w: 640. + n as f32 * 320.,
                        h: 480.,
                    })
                    .unwrap();
                }
            }
            s.focused = None;
            // Check the actual cylindrical patches submitted to OpenXR, not only model bounds.
            let rendered = xr_input::quads(&s, Pose::default());
            for lane in 0..3 {
                let ids = &s.workspaces[0].lanes[lane].windows;
                let a = rendered
                    .iter()
                    .find(|q| q.id == ids[0])
                    .unwrap()
                    .dome
                    .as_ref()
                    .unwrap();
                let b = rendered
                    .iter()
                    .find(|q| q.id == ids[1])
                    .unwrap()
                    .dome
                    .as_ref()
                    .unwrap();
                let yaw = |d: &xr_input::DomePatch| {
                    let forward = d.frame.rotate([0., 0., -1.]);
                    forward[0].atan2(-forward[2])
                };
                let separation = (yaw(b) - yaw(a)).rem_euclid(std::f32::consts::TAU);
                let gap = separation - (a.horizontal + b.horizontal) / 2.;
                let pixel = (a.horizontal / s.window(ids[0]).unwrap().desktop.w)
                    .min(b.horizontal / s.window(ids[1]).unwrap().desktop.w);
                assert!(
                    (gap / pixel - spacing).abs() < 0.002,
                    "dome radius {radius}, lane {lane}, gap {}px",
                    gap / pixel
                );
                s.focused = Some(ids[0]);
                let raised = xr_input::quads(&s, Pose::default());
                let raised = raised
                    .iter()
                    .find(|q| q.id == ids[0])
                    .unwrap()
                    .dome
                    .as_ref()
                    .unwrap();
                assert!(((raised.horizontal - a.horizontal) / 2. / pixel - 4.).abs() < 0.002);
                s.focused = None;
            }
            let patch = |lane| {
                rendered
                    .iter()
                    .find(|q| s.window(q.id).unwrap().lane == lane)
                    .unwrap()
                    .dome
                    .as_ref()
                    .unwrap()
            };
            let middle = patch(1);
            for lane in [0, 2] {
                let outer = patch(lane);
                let gap = if lane == 0 {
                    outer.bottom - middle.top
                } else {
                    middle.bottom - outer.top
                };
                let pixel =
                    ((outer.top - outer.bottom) / 480.).min((middle.top - middle.bottom) / 480.);
                assert!(gap / pixel <= spacing + 0.002 && gap / pixel >= -0.002);
            }
        }
    }
}

#[test]
fn dome_bars_border_lanes_and_orbit_with_horizontal_head_yaw() {
    let mut s = scene();
    let layout = dome::xr_transport::ShellLayout {
        width: 1280,
        height: 800,
        top: 36,
        bottom: 60,
        notifications: None, pullout: None
    };
    let head = Pose {
        position_m: s.config.center_m,
        ..Pose::default()
    };
    let center = Vec3::from_array(s.config.center_m);
    s.config.dome_upper_tilt_deg = 0.;
    let original = xr_input::shell_quads(&s, head, &layout);
    let spans = s.packed_lane_spans(s.active_workspace);
    let (bottom, top) = (spans[2][0], spans[0][1]);
    for (i, uv, angle) in [(0, [0.5, 1.], top), (1, [0.5, 0.], bottom)] {
        let p = original[i].point(uv) - center;
        let gap = (p.y / s.config.radius_m - angle.to_radians()).abs();
        let pitch = (spans[if i == 0 { 0 } else { 2 }][1] - spans[if i == 0 { 0 } else { 2 }][0]).to_radians() / 460.;
        assert!(gap > 0. && gap / pitch <= 8.001);
        assert!((p.x.hypot(p.z) - s.config.radius_m).abs() < 0.001);
    }
    for yaw in [-170_f32, -90., 45., 170.] {
        let rotation = Quat::from_rotation_y(yaw.to_radians());
        let turned = Pose {
            orientation_xyzw: (rotation * Quat::from_rotation_x(0.4)).to_array(),
            ..head
        };
        let bars = xr_input::shell_quads(&s, turned, &layout);
        for (a, b) in original.iter().zip(&bars) {
            let expected = center + rotation * (Vec3::from_array(a.pose.position_m) - center);
            assert!(expected.distance(Vec3::from_array(b.pose.position_m)) < 0.001);
            // Pitch does not pull either bar over a room-anchored lane.
            assert!((a.pose.position_m[1] - b.pose.position_m[1]).abs() < 0.001);
        }
        // The room lanes retain their own anchor while bars track the view.
        assert!(
            xr_input::quads(&s, head)[0]
                .point([0.5, 0.5])
                .distance(xr_input::quads(&s, turned)[0].point([0.5, 0.5]))
                < 0.001
        );
    }
    let grown = xr_input::shell_quads(
        &s,
        head,
        &dome::xr_transport::ShellLayout {
            top: 300,
            bottom: 350,
            notifications: None,
            ..layout.clone()
        },
    );
    assert!(
        grown[0]
            .point([0.5, 0.])
            .distance(original[0].point([0.5, 0.]))
            < 0.001
    );
    assert!(
        grown[1]
            .point([0.5, 1.])
            .distance(original[1].point([0.5, 1.]))
            < 0.001
    );
    s.config.anchor_mode = AnchorMode::Head;
    let moved = Pose {
        position_m: [3., 2., -4.],
        ..head
    };
    let bars = xr_input::shell_quads(&s, moved, &layout);
    for (a, b) in original.iter().zip(&bars) {
        assert!(
            (Vec3::from_array(a.pose.position_m) - center)
                .distance(Vec3::from_array(b.pose.position_m) - Vec3::from_array(moved.position_m))
                < 0.001
        );
    }
}

#[test]
fn tilted_bars_stay_level_and_rigid_when_turning_the_head() {
    let layout = dome::xr_transport::ShellLayout {
        width: 1024, height: 768, top: 36, bottom: 60, notifications: None, pullout: None
    };
    for populated in [false, true] {
        for anchor_mode in [AnchorMode::Room, AnchorMode::Head] {
            let mut s = Scene::new("level bars");
            s.config.anchor_mode = anchor_mode;
            s.config.dome_upper_tilt_deg = 20.;
            s.config.yaw_deg = 27.;
            if populated {
                for lane in [0, 2] {
                    let id = s.add("window", "fixture");
                    s.apply(Action::Move { id, workspace: 0, lane }).unwrap();
                    s.apply(Action::DomeOffset { id, yaw_deg: 35., pitch_deg: 0. }).unwrap();
                }
            }
            let head = Pose { position_m: s.config.center_m, ..Pose::default() };
            let original = xr_input::shell_quads(&s, head, &layout);
            let center = Vec3::from_array(head.position_m);
            for yaw in [-170_f32, -90., -45., 0., 45., 90., 170.] {
                let turn = Quat::from_rotation_y(yaw.to_radians());
                let moved = Pose {
                    orientation_xyzw: (turn * Quat::from_rotation_x(0.4)
                        * Quat::from_rotation_z(-0.3)).to_array(),
                    ..head
                };
                let bars = xr_input::shell_quads(&s, moved, &layout);
                for (a, b) in original.iter().zip(&bars) {
                    for v in [0., 0.5, 1.] {
                        assert!((b.point([0., v]).y - b.point([1., v]).y).abs() < 0.00001,
                            "slanted bar {}, yaw {yaw}, anchor {anchor_mode:?}", b.id);
                        for u in [0., 0.25, 0.5, 0.75, 1.] {
                            let expected = center + turn * (a.point([u, v]) - center);
                            let p = b.point([u, v]);
                            assert!(expected.distance(p) < 0.00001,
                                "deformed bar {}, yaw {yaw}, anchor {anchor_mode:?}", b.id);
                            let hit = b.hit(center, (p - center).normalize(), false).unwrap();
                            assert!((hit[0] - u).abs() < 0.0001 && (hit[1] - v).abs() < 0.0001);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn bars_follow_rendered_lane_edges_with_at_most_eight_logical_pixels_of_clearance() {
    let layout = dome::xr_transport::ShellLayout {
        width: 1024, height: 768, top: 36, bottom: 60, notifications: None, pullout: None
    };
    for radius in [0.6, 2.2, 4.4, 8.] {
        for count in [1, 3, 20] {
            for lean in [0., 8., 20.] {
                let mut s = Scene::new("bar gaps");
                s.config.radius_m = radius;
                s.config.dome_vertical_span_deg = if count == 1 { 30. } else if count == 3 { 110. } else { 170. };
                s.config.upper_bar_height_deg = if count == 1 { 1. } else if count == 3 { 5. } else { 20. };
                s.config.taskbar_height_deg = if count == 1 { 20. } else if count == 3 { 7. } else { 1. };
                s.config.dome_upper_tilt_deg = lean;
                s.workspaces[0].shares = [0.3, 0.45, 0.25];
                for lane in [0, 1, 2] {
                    for n in 0..count {
                        let id = s.add("window", "fixture");
                        s.apply(Action::Move { id, workspace: 0, lane }).unwrap();
                        s.apply(Action::DesktopResize { id, w: 480. + n as f32 * 100., h: 480. + n as f32 * 20. }).unwrap();
                    }
                }
                let head = Pose { position_m: s.config.center_m, ..Pose::default() };
                for focused in [None, Some(s.workspaces[0].lanes[0].windows[0])] {
                    s.focused = focused;
                    let windows = xr_input::quads(&s, head);
                    // Exercise the actual renderer/input entry point, not just its
                    // preferred-layout helper: fitting must preserve lane attachment.
                    let bars = xr_input::shell_quads_in_fov(&s, head, &layout, [-0.9, 0.9, -0.9, 0.9]);
                    for (bar, lane, edge_v, nominal) in [(&bars[0], 0, 1., 36.), (&bars[1], 2, 0., 60.)] {
                        let adjacent = windows.iter()
                            .filter(|q| s.window(q.id).unwrap().lane == lane)
                            .min_by(|a, b| {
                                let yaw = |q: &xr_input::Quad| {
                                    let p = q.point([0.5, 0.5]) - Vec3::from_array(head.position_m);
                                    p.x.atan2(-p.z).abs()
                                };
                                yaw(a).total_cmp(&yaw(b))
                            }).unwrap();
                        let d = adjacent.dome.as_ref().unwrap();
                        let lane_pixel = d.radius * (d.top - d.bottom) / s.window(adjacent.id).unwrap().desktop.h;
                        let bar_pixel = bar.size[1] / nominal;
                        for u in [0., 0.25, 0.5, 0.75, 1.] {
                            let p = bar.point([u, edge_v]);
                            // Compare unwrapped lane heights in the bar's own
                            // frame: its lean follows the view heading, while
                            // an off-centre adjacent panel retains its heading.
                            let frame = bar.dome.as_ref().unwrap().frame;
                            let local = frame.inverse_rotate((p - Vec3::from_array(frame.position_m)).to_array());
                            let edge = if lane == 0 { d.top } else { d.bottom } * d.radius;
                            let gap = if lane == 0 { local[1] - edge } else { edge - local[1] };
                            assert!(gap >= -0.00001 && gap / lane_pixel <= 8.01,
                                "radius {radius}, windows {count}, lean {lean}, lane {lane}, gap {}px", gap / lane_pixel);
                            assert!(gap / bar_pixel <= 8.01);
                            assert!((local[0].hypot(local[2]) - d.radius).abs() < 0.00001);
                            let ray = (p - Vec3::from_array(head.position_m)).normalize();
                            let hit = bar.hit(Vec3::from_array(head.position_m), ray, false).unwrap_or_else(|| panic!("miss radius {radius}, count {count}, lean {lean}, lane {lane}, uv {u}/{edge_v}, raw {:?}", bar.hit(Vec3::from_array(head.position_m), ray, true)));
                            assert!((hit[0] - u).abs() < 0.0001 && (hit[1] - edge_v).abs() < 0.0001);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn legacy_bar_position_settings_are_ignored_and_no_longer_saved_or_exposed() {
    let config: Config = serde_json::from_str(r#"{
        "upper_bar_x_deg":30,"taskbar_x_deg":-30,
        "upper_inset_deg":100,"lower_inset_deg":100,
        "upper_bar_max_width_deg":45,"taskbar_max_width_deg":160
    }"#).unwrap();
    assert_eq!(config, Config::default());
    config.validate().unwrap();
    let saved = serde_json::to_value(&config).unwrap();
    for key in ["upper_bar_x_deg", "taskbar_x_deg", "upper_inset_deg", "lower_inset_deg",
        "upper_bar_max_width_deg", "taskbar_max_width_deg"] {
        assert!(saved.get(key).is_none());
    }
    let labels: Vec<_> = geometry_settings::rows(&config).into_iter().map(|(label, _)| label).collect();
    assert!(!labels.iter().any(|s| s.contains("inset") || s.contains("horizontal") || s.contains("bar width")));
    assert!(labels.contains(&"Upper tray height") && labels.contains(&"Taskbar height"));
}

#[test]
fn mouse_crosses_bars_empty_space_and_lanes_without_depth_or_size_jumps() {
    for projection in [Projection::RoomDome] {
        let mut s = scene();
        s.config.projection = projection;
        let head = Pose {
            position_m: s.config.center_m,
            ..Pose::default()
        };
        let layout = dome::xr_transport::ShellLayout {
            width: 1280,
            height: 800,
            top: 36,
            bottom: 60,
            notifications: None, pullout: None
        };
        let mut surfaces = xr_input::quads(&s, head);
        surfaces.extend(xr_input::shell_quads(&s, head, &layout));
        let mut angles = [0., 1.25];
        let mut seen = std::collections::HashSet::new();
        let mut gaps = 0;
        let mut previous: Option<Vec3> = None;
        for _ in 0..1000 {
            let ray = xr_input::mouse_ray(head, angles);
            let reticle = xr_input::mouse_reticle(head, angles);
            let offset =
                Vec3::from_array(reticle.pose.position_m) - Vec3::from_array(head.position_m);
            assert!((offset.length() - 0.4).abs() < 0.00001);
            assert!(offset.normalize().distance(ray) < 0.00001);
            assert_eq!(reticle.size, [0.012, 0.012]);
            if let Some(p) = previous {
                assert!(p.distance(offset) < 0.0011);
            }
            previous = Some(offset);
            if let Some((q, _)) = surfaces
                .iter()
                .filter_map(|q| {
                    q.hit(Vec3::from_array(head.position_m), ray, false)
                        .map(|uv| (q, uv))
                })
                .min_by(|(a, ua), (b, ub)| {
                    a.point(*ua)
                        .distance(Vec3::from_array(head.position_m))
                        .total_cmp(&b.point(*ub).distance(Vec3::from_array(head.position_m)))
                })
            {
                seen.insert(q.id);
            } else {
                gaps += 1;
            }
            xr_input::move_mouse(&mut angles, 0., 1.);
        }
        assert!(seen.contains(&dome::xr_transport::TOP_BAR));
        assert!(seen.contains(&dome::xr_transport::TASK_BAR));
        assert!(seen.contains(&s.windows[0].id));
        assert!(gaps > 0);
    }
}

#[test]
fn dome_bar_extreme_geometry_stays_finite_and_mouse_angles_stop_at_boundary() {
    let mut s = scene();
    let layout = dome::xr_transport::ShellLayout {
        width: 1280,
        height: 800,
        top: 800,
        bottom: 800,
        notifications: None, pullout: None
    };
    for span in [30., 110., 170.] {
        s.config.dome_vertical_span_deg = span;
        s.config.upper_bar_height_deg = 20.;
        s.config.taskbar_height_deg = 20.;
        for q in xr_input::shell_quads(&s, Pose::default(), &layout) {
            assert!(q.size.iter().all(|v| v.is_finite() && *v > 0.));
            assert!(q.pose.position_m.iter().all(|v| v.is_finite()));
        }
    }
    let mut angles = [3.14, 0.];
    let before = xr_input::mouse_ray(Pose::default(), angles);
    xr_input::move_mouse(&mut angles, 4., 0.);
    assert_eq!(angles[0], std::f32::consts::PI);
    assert!(before.distance(xr_input::mouse_ray(Pose::default(), angles)) < 0.011);
    let old = angles;
    xr_input::move_mouse(&mut angles, f64::NAN, 1.);
    assert_eq!(angles, old);
}

#[test]
fn mouse_stays_inside_asymmetric_viewport_and_returns_from_edges_immediately() {
    let viewport = [-0.7, 0.9, -0.6, 0.8];
    for dx in [-1e9, 0., 1e9] {
        for dy in [-1e9, 0., 1e9] {
            let mut angles = [0., 0.];
            xr_input::move_mouse(&mut angles, dx, dy);
            xr_input::confine_mouse(&mut angles, viewport);
            let ray = xr_input::mouse_ray(Pose::default(), angles);
            let horizontal = ray.x.atan2(-ray.z);
            let vertical = ray.y.atan2(-ray.z);
            assert!((viewport[0]..viewport[1]).contains(&horizontal));
            assert!((viewport[2]..viewport[3]).contains(&vertical));
            let edge = angles;
            xr_input::move_mouse(&mut angles, -dx.signum(), -dy.signum());
            xr_input::confine_mouse(&mut angles, viewport);
            if dx != 0. { assert!(angles[0].abs() < edge[0].abs()); }
            if dy != 0. { assert!(angles[1].abs() < edge[1].abs()); }
        }
    }
    // Rebasing a stationary cursor after a head turn must also recover it.
    let mut angles = xr_input::mouse_angles_at(
        Pose { orientation_xyzw: Quat::from_rotation_y(2.).to_array(), ..Pose::default() },
        Vec3::new(0., 0., -2.),
    );
    xr_input::confine_mouse(&mut angles, viewport);
    assert!((viewport[0]..viewport[1]).contains(&angles[0]));
}

#[test]
fn rendered_mouse_recovers_when_captured_surface_or_new_head_pose_leaves_viewport() {
    let viewport = [-0.7, 0.9, -0.6, 0.8];
    let reticle = xr_input::mouse_reticle(Pose::default(), [0., 0.]);
    let unchanged = xr_input::confine_mouse_reticle(reticle.clone(), Pose::default(), viewport);
    assert_eq!(unchanged.pose.position_m, reticle.pose.position_m);
    for head in [
        Pose { orientation_xyzw: Quat::from_rotation_y(2.).to_array(), ..Pose::default() },
        Pose { orientation_xyzw: Quat::from_rotation_x(-1.2).to_array(), ..Pose::default() },
    ] {
        let recovered = xr_input::confine_mouse_reticle(reticle.clone(), head, viewport);
        let ray = Vec3::from_array(head.inverse_rotate(
            (Vec3::from_array(recovered.pose.position_m) - Vec3::from_array(head.position_m)).to_array(),
        ));
        assert!((viewport[0]..viewport[1]).contains(&ray.x.atan2(-ray.z)));
        assert!((viewport[2]..viewport[3]).contains(&ray.y.atan2(-ray.z)));
    }
}

#[test]
fn open_tube_keeps_width_and_vertical_pixel_spacing_at_every_height() {
    let mut s = Scene::new("test");
    for lane in 0..3 {
        let id = s.add("Cylinder", "app");
        s.apply(Action::Move {
            id,
            lane,
            workspace: 0,
        })
        .unwrap();
    }
    s.focused = None;
    for q in xr_input::quads(&s, Pose::default()) {
        let d = q.dome.as_ref().unwrap();
        let width = q.point([0., 0.5]).distance(q.point([1., 0.5]));
        for v in [0., 0.25, 0.5, 0.75, 1.] {
            assert!((q.point([0., v]).distance(q.point([1., v])) - width).abs() < 0.00001);
            for u in [0., 0.3, 1.] {
                let expected = q.point([u, 0.]) * (1. - v) + q.point([u, 1.]) * v;
                assert!(q.point([u, v]).distance(expected) < 0.00001);
                // Reconstruct exactly what the native cylinder layer submits.
                let p = d.layer_pose().transform([
                    d.radius * ((u - 0.5) * d.horizontal).sin(),
                    d.radius * (d.top - d.bottom) * (0.5 - v),
                    -d.radius * ((u - 0.5) * d.horizontal).cos(),
                ]);
                assert!(q.point([u, v]).distance(Vec3::from_array(p)) < 0.00001);
            }
        }
        let w = s.window(q.id).unwrap();
        assert!((d.aspect_ratio() - w.desktop.w / w.desktop.h).abs() < 0.00001);
        let axis = Vec3::from_array(d.frame.rotate([0., 1., 0.]));
        let origin = Vec3::from_array(d.frame.position_m);
        assert!(q.hit(origin, axis, false).is_none());
        assert!(q.hit(origin, -axis, false).is_none());
    }
}

#[test]
fn upper_lane_leans_inward_about_lower_edge_and_pointer_hits_follow_it() {
    let mut s = Scene::new("test");
    for lane in 0..3 {
        let id = s.add("Lean", "app");
        s.apply(Action::Move {
            id,
            lane,
            workspace: 0,
        })
        .unwrap();
    }
    s.focused = None;
    s.config.dome_upper_tilt_deg = 0.;
    let upright = xr_input::quads(&s, Pose::default());
    s.config.dome_upper_tilt_deg = 8.;
    let leaned = xr_input::quads(&s, Pose::default());
    for (a, b) in upright.iter().zip(&leaned) {
        if s.window(a.id).unwrap().lane == 0 {
            let low = a.point([0.5, 1.]).y;
            for u in [0., 0.5, 1.] {
                assert!(b.point([u, 1.]).y >= low - 0.00001);
            }
            assert!((a.point([0.5, 1.]).z - b.point([0.5, 1.]).z).abs() < 0.00001);
            let delta = a
                .pose
                .inverse_rotate((b.point([0.5, 0.]) - a.point([0.5, 0.])).to_array());
            assert!(delta[2] > 0.05); // Toward the viewer, without shrinking width.
            assert_eq!(a.size, b.size);
            let origin = Vec3::from_array(s.config.center_m) + Vec3::new(0.1, 0.15, -0.1);
            for uv in [[0.1, 0.1], [0.5, 0.5], [0.9, 0.9], [0., 0.], [1., 1.]] {
                let p = b.point(uv);
                let hit = b.hit(origin, (p - origin).normalize(), false).unwrap();
                assert!((uv[0] - hit[0]).abs() < 0.00001 && (uv[1] - hit[1]).abs() < 0.00001);
                let actual = s
                    .hit_test(origin.to_array(), (p - origin).normalize().to_array())
                    .unwrap();
                assert_eq!(actual.id, b.id);
            }
        } else {
            for uv in [[0., 0.], [0.5, 0.5], [1., 1.]] {
                assert!(a.point(uv).distance(b.point(uv)) < 0.00001);
            }
        }
    }
}

#[test]
fn cylinder_packing_preserves_texture_aspect_ratio_in_crowded_lanes() {
    for count in [1, 3, 20] {
        let mut s = Scene::new("test");
        for n in 0..count {
            let id = s.add("Aspect", "app");
            s.apply(Action::DesktopResize {
                id,
                w: 480. + n as f32 * 70.,
                h: 480.,
            })
            .unwrap();
        }
        s.focused = None;
        for q in xr_input::quads(&s, Pose::default()) {
            let w = s.window(q.id).unwrap();
            assert!(
                (q.dome.as_ref().unwrap().aspect_ratio() - w.desktop.w / w.desktop.h).abs()
                    < 0.00001
            );
        }
    }
}

#[test]
fn upper_lean_configuration_is_compatible_and_bounded() {
    let old: Config = serde_json::from_str("{}").unwrap();
    assert_eq!(old.dome_upper_tilt_deg, 8.);
    let adjusted = dome::geometry_settings::adjust(&old, 13, 1.).unwrap();
    assert_eq!(adjusted.dome_upper_tilt_deg, 9.);
    let mut c = old;
    for invalid in [-1., 21., f32::NAN, f32::INFINITY] {
        c.dome_upper_tilt_deg = invalid;
        assert!(c.validate().is_err());
    }
    for valid in [0., 8., 20.] {
        c.dome_upper_tilt_deg = valid;
        c.validate().unwrap();
    }
}

#[test]
fn expanded_settings_fit_view_pane_and_follow_projection_preferences() {
    use dome::{AnchorMode, Pose, Projection, Scene};
    let mut scene = Scene::default();
    let head = Pose {
        position_m: [1., 1.7, 2.],
        ..Pose::default()
    };
    let layout = dome::xr_transport::ShellLayout {
        width: 1024,
        height: 768,
        top: 768,
        bottom: 60,
        notifications: None, pullout: None
    };
    for projection in [Projection::RoomDome] {
        scene.config.projection = projection;
        scene.config.anchor_mode = AnchorMode::Head;
        let q = dome::xr_input::shell_quads(&scene, head, &layout);
        assert_eq!(q.len(), 1);
        assert!((q[0].size[0] / q[0].size[1] - 4. / 3.).abs() < 0.001);
        let d = if projection == Projection::RoomDome {
            scene.config.radius_m
        } else {
            scene.config.pullout_distance_m
        };
        assert!((q[0].pose.position_m[2] - (head.position_m[2] - d)).abs() < 0.001);
        assert!(
            2. * (q[0].size[1] / (2. * d)).atan().to_degrees() <= scene.config.vertical_fov_deg
        );
        assert!(2. * (q[0].size[0] / (2. * d)).atan().to_degrees() <= 80.01);
    }
}

#[test]
fn notification_pane_follows_head_and_maps_its_crop_without_bar_stretching() {
    for projection in [Projection::RoomDome] {
        let mut s = scene();
        s.config.projection = projection;
        let head = Pose {
            position_m: [1., 1.7, 2.],
            orientation_xyzw: glam::Quat::from_rotation_y(0.7).to_array(),
        };
        let layout = dome::xr_transport::ShellLayout {
            width: 1024,
            height: 768,
            top: 36,
            bottom: 60,
            notifications: Some([652, 48, 360, 274]), pullout: None
        };
        let quads = xr_input::shell_quads(&s, head, &layout);
        let n = quads
            .iter()
            .find(|q| q.id == dome::xr_transport::NOTIFICATIONS)
            .unwrap();
        assert!((n.size[0] / n.size[1] - 360. / 274.).abs() < 0.0001);
        assert!(n.dome.is_none());
        let local = head.inverse_rotate(std::array::from_fn(|i| {
            n.pose.position_m[i] - head.position_m[i]
        }));
        assert!(local[0] > 0. && local[1] > 0. && local[2] < 0.);
        assert!(n.size[0] < 2. && n.size[1] < 2.);
        let mut hidden = dome::xr_presentation::Presentation::default();
        hidden.top = false;
        assert!(!hidden.interactive(n.id));
        assert!(dome::xr_transport::is_shell(n.id));
        let empty = dome::xr_transport::ShellLayout {
            notifications: None,
            ..layout
        };
        let bars = xr_input::shell_quads(&s, head, &empty);
        assert_eq!(bars.len() + 1, quads.len());
        for (a, b) in bars.iter().zip(&quads) {
            assert_eq!(a.size, b.size);
            assert_eq!(a.pose, b.pose);
        }
    }
}

#[test]
fn mouse_rebases_to_the_same_uv_after_head_and_carousel_movement() {
    let mut s = scene();
    let id = s.windows[0].id;
    s.apply(Action::Move {
        id,
        lane: 0,
        workspace: 0,
    })
    .unwrap();
    let head = Pose {
        position_m: [0.2, 1.6, 0.1],
        orientation_xyzw: Quat::from_rotation_y(0.4).to_array(),
    };
    for rotation in [0., 25., -40.] {
        s.workspaces[0].lanes[0].rotation_deg = rotation;
        let q = xr_input::quads(&s, head).remove(0);
        for uv in [[0.1, 0.2], [0.5, 0.5], [0.9, 0.8]] {
            let point = q.point(uv);
            let angles = xr_input::mouse_angles_at(head, point);
            let actual = q
                .hit(
                    Vec3::from_array(head.position_m),
                    xr_input::mouse_ray(head, angles),
                    false,
                )
                .unwrap();
            assert!((actual[0] - uv[0]).abs() < 0.0001 && (actual[1] - uv[1]).abs() < 0.0001);
            let reticle = xr_input::surface_reticle(&q, uv, u64::MAX);
            assert!(
                (Vec3::from_array(reticle.pose.position_m).distance(point) - 0.002).abs() < 0.00001
            );
            let normal = q.dome.as_ref().unwrap().inward_normal(point);
            assert!(
                (Quat::from_array(reticle.pose.orientation_xyzw) * Vec3::Z).distance(normal)
                    < 0.00001
            );
        }
    }
}
#[test]
fn settings_persist_height_and_bindings_and_reject_conflicts_atomically() {
    let folder = std::env::temp_dir().join(format!(
        "dome-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path = folder.join("xr.json");
    let mut c = Config::default();
    c.center_m[1] = 1.75;
    c.dome_vertical_span_deg = 130.;
    let index = c
        .keybindings
        .iter()
        .position(|b| b.command == dome::shortcuts::Command::Launch)
        .unwrap();
    c.keybindings[index].key = 'l' as u32;
    dome::geometry_settings::save(&path, &c).unwrap();
    assert_eq!(dome::geometry_settings::load(&path).unwrap().unwrap(), c);
    let saved = std::fs::read(&path).unwrap();
    c.keybindings.push(c.keybindings[0].clone());
    assert!(dome::geometry_settings::save(&path, &c).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn curved_pullouts_keep_mouse_uv_and_reticle_after_move_and_resize() {
    let mut s = scene();
    let id = s.windows[0].id;
    let anchor = Pose {
        position_m: [0.2, 1.6, 0.1],
        orientation_xyzw: Quat::from_rotation_y(0.4).to_array(),
    };
    s.apply(Action::PullOut { id, head: anchor }).unwrap();
    for scale in [0.6, 1., 1.8] {
        s.apply(Action::SpatialTransform {
            id,
            pose: None,
            scale,
        })
        .unwrap();
        for head in [
            anchor,
            Pose {
                position_m: [-0.1, 1.7, 0.3],
                orientation_xyzw: Quat::from_rotation_y(-0.3).to_array(),
            },
        ] {
            let q = xr_input::quads(&s, head).remove(0);
            for uv in [[0.05, 0.2], [0.5, 0.5], [0.95, 0.8]] {
                let point = q.point(uv);
                let ray = xr_input::mouse_ray(head, xr_input::mouse_angles_at(head, point));
                let actual = q
                    .hit(Vec3::from_array(head.position_m), ray, false)
                    .unwrap();
                assert!((actual[0] - uv[0]).abs() < 0.0001 && (actual[1] - uv[1]).abs() < 0.0001);
                let cursor = xr_input::surface_reticle(&q, uv, u64::MAX);
                assert!(
                    (Vec3::from_array(cursor.pose.position_m).distance(point) - 0.002).abs()
                        < 0.00001
                );
            }
        }
    }
}

#[test]
fn saved_shortcuts_gain_pullout_without_overriding_a_custom_chord() {
    use dome::shortcuts::{Command, Mods};
    let mut old = Config::default();
    old.keybindings.retain(|b| b.command != Command::PullOut);
    let migrated: Config = serde_json::from_value(serde_json::to_value(&old).unwrap()).unwrap();
    let binding = migrated
        .keybindings
        .iter()
        .find(|b| b.command == Command::PullOut)
        .unwrap();
    assert!(binding.enabled);
    assert_eq!(binding.key, 'p' as u32);
    old.keybindings[0].key = 'p' as u32;
    old.keybindings[0].mods = Mods {
        logo: true,
        ..Mods::default()
    };
    let migrated: Config = serde_json::from_value(serde_json::to_value(&old).unwrap()).unwrap();
    assert!(
        !migrated
            .keybindings
            .iter()
            .find(|b| b.command == Command::PullOut)
            .unwrap()
            .enabled
    );
    migrated.validate().unwrap();
}

#[test]
fn mouse_cursor_stays_on_compositor_surface_when_render_head_moves() {
    let s = scene();
    let surfaces = xr_input::quads(&s, Pose::default());
    let surface = &surfaces[0];
    let uv = [0.85, 0.2];
    let cursor = dome::xr_transport::Cursor { id: surface.id, uv };
    let head = Pose { position_m: [1., 1., 0.],
        orientation_xyzw: Quat::from_rotation_y(2.).to_array() };
    let reticle = xr_input::mouse_cursor_reticle(&surfaces, Some(&cursor), head, [0., 0.], &s);
    let point = surface.point(uv);
    assert!((Vec3::from_array(reticle.pose.position_m) - point).length() < 0.003);
    let normal = Quat::from_array(reticle.pose.orientation_xyzw) * Vec3::Z;
    assert!(normal.dot(surface.dome.as_ref().unwrap().inward_normal(point)) > 0.999);
    let fallback = xr_input::mouse_cursor_reticle(&[], Some(&cursor), head, [0., 0.], &s);
    let dome = xr_input::mouse_dome(&s, head);
    let point = Vec3::from_array(fallback.pose.position_m);
    let local = dome.dome.as_ref().unwrap().frame.inverse_rotate(point.to_array());
    assert!((local[0].hypot(local[2]) - s.config.radius_m).abs() < 0.003);
}

#[test]
fn retired_tracking_settings_are_ignored_and_not_saved() {
    let config: Config = serde_json::from_str(r#"{"xr_pointer_mode":"gaze","gaze_smoothing":true}"#).unwrap();
    assert_eq!(config, Config::default());
    let saved = serde_json::to_value(&config).unwrap();
    assert!(saved.get("xr_pointer_mode").is_none());
    assert!(saved.get("gaze_smoothing").is_none());
    assert_eq!(geometry_settings::rows(&config).len(), 16);
}

#[test]
fn empty_dome_mouse_cursor_remains_world_anchored_all_around() {
    let s = scene();
    let head = Pose::default();
    let background = xr_input::mouse_dome(&s, head);
    for yaw in [-3., -1.5, 0., 1.5, 3.] {
        let uv = background.hit(Vec3::ZERO, xr_input::mouse_ray(head, [yaw, 0.2]), false).unwrap();
        let cursor = dome::xr_transport::Cursor { id: background.id, uv };
        let before = xr_input::mouse_cursor_reticle(&[], Some(&cursor), head, [yaw, 0.2], &s);
        let moved = Pose { position_m: [0.2, 0.3, 0.1],
            orientation_xyzw: Quat::from_rotation_y(0.5).to_array() };
        let after = xr_input::mouse_cursor_reticle(&[], Some(&cursor), moved, [yaw, 0.2], &s);
        assert_eq!(before.pose, after.pose);
        assert!((Vec3::from_array(after.pose.position_m).length() - s.config.radius_m).abs() < 0.1);
    }
}

#[test]
fn enlarged_windows_follow_focus_in_render_and_pointer_order() {
    let mut s = scene();
    let a = s.windows[0].id;
    let b = s.add("Second", "app");
    for panel in s.panels() {
        s.apply(Action::DomeOffset { id: panel.id, yaw_deg: -panel.yaw_deg, pitch_deg: 0. }).unwrap();
    }
    s.apply(Action::SpatialTransform { id: a, pose: None, scale: 2. }).unwrap();
    for focus in [a, b, a] {
        s.apply(Action::Focus { id: focus }).unwrap();
        let quads = xr_input::quads(&s, Pose::default());
        assert_eq!(quads.last().unwrap().id, focus);
        let point = quads.iter().find(|q| q.id == focus).unwrap().point([0.5, 0.5]);
        assert_eq!(s.hit_test([0.; 3], point.normalize().to_array()).unwrap().id, focus);
    }
}

#[test]
fn inactive_workspaces_render_as_deeper_faded_domes_without_accepting_input() {
    let mut s = Scene::default();
    let a = s.add("active", "app");
    let b = s.add("background", "app");
    s.apply(Action::Move { id: b, lane: 1, workspace: 1 }).unwrap();
    s.apply(Action::AddWorkspace { name: "Third".into() }).unwrap();
    let c = s.add("farther", "app");
    let third = s.workspaces.len() - 1;
    s.apply(Action::Move { id: c, lane: 1, workspace: third }).unwrap();
    s.apply(Action::SwitchWorkspace { workspace: 0 }).unwrap();
    let render = xr_input::render_quads(&s, Pose::default());
    let radius = |id| render.iter().find(|q| q.id == id).unwrap().dome.as_ref().unwrap().radius;
    assert!(radius(a) < radius(b) && radius(b) < radius(c));
    let input = xr_input::quads(&s, Pose::default());
    assert_eq!(serde_json::to_value(render.iter().find(|q| q.id == a).unwrap()).unwrap(),
        serde_json::to_value(input.iter().find(|q| q.id == a).unwrap()).unwrap());
    let panels = s.panels();
    let opacity = |id| panels.iter().find(|p| p.id == id).unwrap().opacity;
    assert_eq!(opacity(a), 1.);
    assert!(opacity(b) > opacity(c) && opacity(c) > 0.);
    assert_eq!(xr_input::quads(&s, Pose::default()).iter().map(|q| q.id).collect::<Vec<_>>(), vec![a]);
    s.apply(Action::SwitchWorkspace { workspace: 1 }).unwrap();
    assert_eq!(xr_input::quads(&s, Pose::default()).iter().map(|q| q.id).collect::<Vec<_>>(), vec![b]);
    s.apply(Action::Minimize { id: c }).unwrap();
    assert!(!xr_input::render_quads(&s, Pose::default()).iter().any(|q| q.id == c));
    s.windows.iter_mut().find(|w| w.id == a).unwrap().xr_visible = false;
    assert!(!xr_input::render_quads(&s, Pose::default()).iter().any(|q| q.id == a));
}

#[test]
fn render_density_is_validated_persisted_and_independent_of_spatial_geometry() {
    let mut s = scene();
    let default: Config = serde_json::from_str("{}").unwrap();
    assert_eq!(default.xr_render_scale, 1.5);
    let before = xr_input::quads(&s, Pose::default());
    s.config.xr_render_scale = 2.;
    let after = xr_input::quads(&s, Pose::default());
    for (a,b) in before.iter().zip(&after) {
        assert_eq!(a.id,b.id); assert_eq!(a.pose,b.pose); assert_eq!(a.size,b.size);
        for uv in [[0.,0.], [0.5,0.5], [1.,1.]] { assert_eq!(a.point(uv),b.point(uv)); }
    }
    let json = serde_json::to_string(&s.config).unwrap();
    assert_eq!(serde_json::from_str::<Config>(&json).unwrap().xr_render_scale, 2.);
    for invalid in [0.75, 2.25, f32::NAN, f32::INFINITY] {
        s.config.xr_render_scale = invalid; assert!(s.config.validate().is_err());
    }
    assert_eq!(geometry_settings::adjust(&default,14,1.).unwrap().xr_render_scale,1.75);
    let mut max = default; max.xr_render_scale=2.;
    assert!(geometry_settings::adjust(&max,14,1.).is_err());
}

#[test]
fn comic_stereo_keeps_standard_dome_geometry_and_input_alignment() {
    let mut s = Scene::new("Comic stereo");
    let id = s.add("Comic", dome::stereo::COMIC_CHAT_SBS_APP_ID);
    let head = dome::Pose { position_m:[0.25,1.8,0.5],
        orientation_xyzw:glam::Quat::from_rotation_y(0.7).to_array() };
    s.apply(Action::PullOut { id, head }).unwrap();
    for distance in [0.6,1.2,2.8] {
        s.config.pullout_distance_m=distance;
        let rendered=dome::xr_input::render_quads(&s,head);
        let input=dome::xr_input::quads(&s,head);
        let a=rendered.iter().find(|q|q.id==id).unwrap();
        let b=input.iter().find(|q|q.id==id).unwrap();
        assert!(a.dome.is_some());
        let mut ordinary=s.clone();
        ordinary.windows.iter_mut().find(|w|w.id==id).unwrap().app_id="ordinary.application".into();
        ordinary.windows.iter_mut().find(|w|w.id==id).unwrap().stereo=dome::stereo::Stereo::Mono;
        let normal=dome::xr_input::quads(&ordinary,head);
        let normal=normal.iter().find(|q|q.id==id).unwrap();
        assert_eq!(serde_json::to_value(a).unwrap(),serde_json::to_value(normal).unwrap());
        assert_eq!(serde_json::to_value(a).unwrap(),serde_json::to_value(b).unwrap());
        assert!((a.dome.as_ref().unwrap().radius-distance).abs()<0.002);
        let origin=glam::Vec3::from_array(head.position_m);
        for uv in [[0.1,0.2],[0.5,0.5],[0.8,0.7]] {
            let target=a.point(uv);
            let hit=b.hit(origin,(target-origin).normalize(),false).unwrap();
            assert!((hit[0]-uv[0]).abs()<1e-5 && (hit[1]-uv[1]).abs()<1e-5);
        }
    }
}

#[test]
fn xr_launch_resolution_is_independent_of_density_and_survives_config_roundtrip() {
    use dome::xr_window_size::Resolution;
    let default = Config::default();
    assert_eq!(default.xr_window_resolution, Resolution::FullHd);
    assert_eq!(serde_json::from_str::<Config>("{}").unwrap(), default);
    let hd = geometry_settings::adjust(&default,15,-1.).unwrap();
    let hd = geometry_settings::adjust(&hd,15,-1.).unwrap();
    assert_eq!(hd.xr_window_resolution, Resolution::Hd);
    assert_eq!(hd.xr_render_scale, default.xr_render_scale);
    assert_eq!(hd.radius_m, default.radius_m);
    assert_eq!(hd.dome_vertical_span_deg, default.dome_vertical_span_deg);
    assert_eq!(serde_json::from_str::<Config>(&serde_json::to_string(&hd).unwrap()).unwrap(),hd);
    assert!(serde_json::from_str::<Config>(r#"{"xr_window_resolution":"2160p"}"#).is_err());
}
