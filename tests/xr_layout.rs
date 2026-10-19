use dome::{
    xr_input::{self, Hand, Interaction},
    *,
};
use glam::{Quat, Vec3};
fn scene() -> Scene {
    let mut s = Scene::new("test");
    s.add("First", "app");
    s
}
#[test]
fn dome_curvature_and_mouse_uv_use_identical_sphere() {
    let s = scene();
    let q = xr_input::quads(&s, Pose::default()).remove(0);
    let d = q.dome.as_ref().unwrap();
    let center = Vec3::from_array(d.frame.position_m);
    for uv in [[0.1, 0.1], [0.5, 0.5], [0.9, 0.9], [0.1, 0.9]] {
        let p = q.point(uv);
        assert!((p.distance(center) - d.radius).abs() < 0.0001);
        let hit = q.hit(center, (p - center).normalize(), false).unwrap();
        assert!((uv[0] - hit[0]).abs() < 0.0001 && (uv[1] - hit[1]).abs() < 0.0001);
    }
    let normal = Quat::from_array(q.pose.orientation_xyzw) * Vec3::Z;
    assert!((q.point([0., 0.5]) - q.point([0.5, 0.5])).dot(normal) > 0.05);
}
#[test]
fn direct_view_is_flat_and_moves_with_view() {
    let mut s = scene();
    s.config.projection = Projection::InView;
    let head = Pose::default();
    let q = xr_input::quads(&s, head).remove(0);
    assert!(q.dome.is_none());
    let normal = Quat::from_array(q.pose.orientation_xyzw) * Vec3::Z;
    assert!((q.point([0., 0.5]) - q.point([0.5, 0.5])).dot(normal).abs() < 0.0001);
    let next = Pose {
        position_m: [1., 1.4, 0.],
        ..head
    };
    let moved = xr_input::quads(&s, next).remove(0);
    assert!((moved.pose.position_m[0] - q.pose.position_m[0] - 1.).abs() < 0.0001);
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
        assert!(q.dome.is_none());
        assert_eq!(q.pose, after.iter().find(|q| q.id == id).unwrap().pose);
        assert!((q.pose.position_m[2] + 1.).abs() < 0.001);
    }
    let left = before.iter().find(|q| q.id == a).unwrap();
    let right = before.iter().find(|q| q.id == b).unwrap();
    assert!(
        (right.pose.position_m[0] - left.pose.position_m[0] - (0.78 + 8. * 0.78 / 720.)).abs()
            < 0.001
    );
    assert!(left.size[0] < 0.86);
    s.apply(Action::ReturnToDome { id: a }).unwrap();
    s.apply(Action::PullOut { id: c, head }).unwrap();
    s.apply(Action::Close { id: b }).unwrap();
    s.apply(Action::ReturnToDome { id: c }).unwrap();
    assert!(s.pullout_anchor.is_none());
}
fn apply(s: &mut Scene, p: xr_input::Packet) {
    for a in p.actions {
        s.apply(a).unwrap();
    }
}
#[test]
fn pulling_then_pushing_a_gripped_panel_returns_to_its_dome_home() {
    let mut s = scene();
    let head = Pose::default();
    let id = s.windows[0].id;
    let q = xr_input::quads(&s, head).remove(0);
    let target = q.point([0.5, 0.5]);
    let inward = (Vec3::from_array(head.position_m) - target).normalize();
    let mut hand = Hand {
        valid: true,
        position: (target + inward * 0.5).to_array(),
        ray: (-inward).to_array(),
        ..Hand::default()
    };
    let mut i = Interaction::default();
    let p = i.update(1., true, [hand, Hand::default()], &s, head);
    apply(&mut s, p);
    hand.grip = true;
    let p = i.update(1.01, true, [hand, Hand::default()], &s, head);
    apply(&mut s, p);
    for t in 0..3 {
        hand.position = (Vec3::from_array(hand.position) + inward * 0.075).to_array();
        let p = i.update(
            1.02 + t as f64 * 0.01,
            true,
            [hand, Hand::default()],
            &s,
            head,
        );
        apply(&mut s, p);
    }
    assert_eq!(s.window(id).unwrap().pullout_slot, Some(0));
    let pose = xr_input::quads(&s, head).remove(0).pose;
    hand.grip = false;
    let p = i.update(1.05, true, [hand, Hand::default()], &s, head);
    apply(&mut s, p);
    let target = Vec3::from_array(pose.position_m);
    let inward = (Vec3::from_array(head.position_m) - target).normalize();
    hand.position = (target + inward * 0.2).to_array();
    hand.ray = (-inward).to_array();
    i.update(1.06, true, [hand, Hand::default()], &s, head);
    hand.grip = true;
    let p = i.update(1.07, true, [hand, Hand::default()], &s, head);
    apply(&mut s, p);
    for t in 0..3 {
        hand.position = (Vec3::from_array(hand.position) - inward * 0.075).to_array();
        let p = i.update(
            1.08 + t as f64 * 0.01,
            true,
            [hand, Hand::default()],
            &s,
            head,
        );
        apply(&mut s, p);
    }
    assert!(s.window(id).unwrap().pullout_slot.is_none());
    assert!(xr_input::quads(&s, head).remove(0).dome.is_some());
}
#[test]
fn switching_projection_hides_pullout_without_losing_dome_slot() {
    let mut s = scene();
    let id = s.windows[0].id;
    s.apply(Action::PullOut {
        id,
        head: Pose::default(),
    })
    .unwrap();
    s.config.projection = Projection::InView;
    assert!(xr_input::quads(&s, Pose::default())
        .iter()
        .all(|p| p.dome.is_none()));
    s.config.projection = Projection::RoomDome;
    assert!(s.window(id).unwrap().pullout_slot.is_some());
}
#[test]
fn shell_insets_and_popup_growth_keep_bar_edges_fixed() {
    let s = scene();
    let layout = dome::xr_transport::ShellLayout {
        width: 1280,
        height: 800,
        top: 36,
        bottom: 60,
    };
    let head = Pose::default();
    let bars = xr_input::shell_quads(&s, head, &layout);
    let top = head
        .inverse_rotate((bars[0].point([0.5, 0.]) - Vec3::from_array(head.position_m)).to_array());
    assert!(
        (top[1].atan2(-top[2]).to_degrees()
            - (s.config.vertical_fov_deg / 2. - s.config.upper_inset_deg))
            .abs()
            < 0.001
    );
    let bottom = head
        .inverse_rotate((bars[1].point([0.5, 1.]) - Vec3::from_array(head.position_m)).to_array());
    assert!(
        (bottom[1].atan2(-bottom[2]).to_degrees()
            - (-s.config.vertical_fov_deg / 2. + s.config.lower_inset_deg))
            .abs()
            < 0.001
    );
    let expanded = xr_input::shell_quads(
        &s,
        head,
        &dome::xr_transport::ShellLayout {
            top: 300,
            bottom: 350,
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
    // Eight pixels minus four consumed by the focused right panel's outset.
    let pixel = 0.78 / s.window(b).unwrap().desktop.w;
    assert!((right.point([0., 0.5]).x - left.point([1., 0.5]).x - 4. * pixel).abs() < 0.00001);
    s.apply(Action::ReturnToDome { id: a }).unwrap();
    assert_eq!(s.window(a).unwrap().xr_scale, 0.6);
}
#[test]
fn left_pinch_can_click_while_right_hand_hovers() {
    let mut s = scene();
    s.config.projection = Projection::InView;
    let head = Pose::default();
    let q = xr_input::quads(&s, head).remove(0);
    let point = q.point([0.5, 0.5]);
    let origin = point + Quat::from_array(q.pose.orientation_xyzw) * Vec3::Z;
    let mut hands = [Hand {
        valid: true,
        position: origin.to_array(),
        ray: (point - origin).normalize().to_array(),
        ..Hand::default()
    }; 2];
    let mut input = Interaction::default();
    input.update(1., true, hands, &s, head);
    hands[0].pinch[0] = true;
    let packet = input.update(1.01, true, hands, &s, head);
    assert!(packet.events.iter().any(|e| matches!(
        e,
        xr_input::SeatEvent::Button {
            button: 0x110,
            pressed: true
        }
    )));
}
#[test]
fn two_hand_translation_can_pull_out_without_accidental_scale() {
    let mut s = scene();
    let head = Pose::default();
    let id = s.windows[0].id;
    let q = xr_input::quads(&s, head).remove(0);
    let target = q.point([0.5, 0.5]);
    let inward = (Vec3::from_array(head.position_m) - target).normalize();
    let mut hands = std::array::from_fn(|i| {
        let origin = target + inward * 0.5 + Vec3::X * if i == 0 { -0.08 } else { 0.08 };
        Hand {
            valid: true,
            position: origin.to_array(),
            ray: (target - origin).normalize().to_array(),
            ..Hand::default()
        }
    });
    let mut input = Interaction::default();
    input.update(1., true, hands, &s, head);
    hands.iter_mut().for_each(|h| h.grip = true);
    let packet = input.update(1.01, true, hands, &s, head);
    apply(&mut s, packet);
    for t in 0..3 {
        for h in &mut hands {
            h.position = (Vec3::from_array(h.position) + inward * 0.075).to_array();
        }
        let packet = input.update(1.02 + t as f64 * 0.01, true, hands, &s, head);
        apply(&mut s, packet);
    }
    assert!(s.window(id).unwrap().pullout_slot.is_some());
    assert_eq!(s.window(id).unwrap().xr_scale, 1.);
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
            // Check the actual spherical patches submitted to OpenXR, not only model bounds.
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
fn hiding_xr_desktop_releases_hand_buttons_and_preserves_window_routing() {
    let mut s = scene();
    let head = Pose::default();
    let q = xr_input::quads(&s, head).remove(0);
    let hand = Hand {
        valid: true,
        position: head.position_m,
        ray: (Vec3::from_array(q.pose.position_m) - Vec3::from_array(head.position_m))
            .normalize()
            .to_array(),
        ..Hand::default()
    };
    let mut input = Interaction::default();
    input.update(1., true, [hand, Hand::default()], &s, head);
    let pinch = Hand {
        pinch: [true, false, false],
        ..hand
    };
    let press = input.update(1.01, true, [pinch, Hand::default()], &s, head);
    assert!(press
        .events
        .iter()
        .any(|e| matches!(e, xr_input::SeatEvent::Button { pressed: true, .. })));
    let id = s.windows[0].id;
    let revision = s.revision;
    s.apply(Action::XrPresentation {
        top: Some(false),
        bottom: None,
        desktop: Some(false),
    })
    .unwrap();
    assert!(s.revision > revision && s.window(id).unwrap().desktop_visible);
    let reset = input.update(1.02, true, [pinch, Hand::default()], &s, head);
    assert!(matches!(
        reset.events.as_slice(),
        [xr_input::SeatEvent::Reset]
    ));
    s.apply(Action::XrPresentation {
        top: None,
        bottom: None,
        desktop: Some(true),
    })
    .unwrap();
    assert!(!s.presentation.top && s.presentation.bottom);
    let held = input.update(1.03, true, [pinch, Hand::default()], &s, head);
    assert!(!held
        .events
        .iter()
        .any(|e| matches!(e, xr_input::SeatEvent::Button { pressed: true, .. })));
}
