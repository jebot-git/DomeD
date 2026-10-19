use dome::*;
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}
#[test]
fn view_geometry_uses_fov_heights_and_both_insets() {
    let mut c = Config {
        projection: Projection::InView,
        ..Config::default()
    };
    let spans = c.lane_spans([0.25, 0.5, 0.25]);
    near(spans[0][1], 37.);
    near(spans[2][0], -35.);
    c.upper_inset_deg += 3.;
    near(c.lane_extent().1, 34.);
    c.lower_inset_deg += 4.;
    near(c.lane_extent().0, -31.);
}
#[test]
fn room_carousel_height_is_independent_of_trays() {
    let mut c = Config::default();
    let before = c.lane_spans([0.23, 0.54, 0.23]);
    c.vertical_fov_deg = 120.;
    c.upper_inset_deg = 12.;
    c.taskbar_height_deg = 12.;
    c.upper_bar_max_width_deg = 180.;
    assert_eq!(before, c.lane_spans([0.23, 0.54, 0.23]));
    c.dome_vertical_span_deg = 130.;
    assert_ne!(before, c.lane_spans([0.23, 0.54, 0.23]));
}
#[test]
fn upper_and_lower_widths_are_independent_and_dont_change_lanes() {
    let mut c = Config {
        projection: Projection::InView,
        ..Config::default()
    };
    let before = c.lane_extent();
    c.upper_bar_max_width_deg = 50.;
    c.taskbar_max_width_deg = 160.;
    c.validate().unwrap();
    assert_eq!(before, c.lane_extent());
}
#[test]
fn invalid_geometry_is_atomic() {
    let mut s = Scene::demo();
    let old = s.config.clone();
    let mut c = old.clone();
    c.upper_inset_deg = 100.;
    assert!(s.apply(Action::Configure { config: c }).is_err());
    assert_eq!(s.config, old);
    let mut c = old.clone();
    c.radius_m = f32::NAN;
    assert!(c.validate().is_err());
    let mut c = old;
    c.upper_bar_max_width_deg = 361.;
    assert!(c.validate().is_err());
}
#[test]
fn boundaries_give_height_to_only_the_adjacent_lane() {
    let mut s = Scene::demo();
    let before = s.config.lane_spans(s.workspaces[0].shares);
    s.apply(Action::ResizeBoundary {
        boundary: 0,
        delta_deg: 3.,
    })
    .unwrap();
    let after = s.config.lane_spans(s.workspaces[0].shares);
    near(before[2][0], after[2][0]);
    near(before[2][1], after[2][1]);
    near(after[0][1] - after[0][0], before[0][1] - before[0][0] + 3.);
    near(s.workspaces[0].shares.iter().sum(), 1.);
    assert!(s
        .apply(Action::ResizeBoundary {
            boundary: 0,
            delta_deg: 1000.
        })
        .is_err());
}
#[test]
fn new_window_is_centered_without_overlaps_on_the_ring() {
    let mut s = Scene::default();
    for i in 0..20 {
        let id = s.add(&i.to_string(), "Test");
        let panels = s.panels();
        near(panels.iter().find(|p| p.id == id).unwrap().yaw_deg, 0.);
        for a in &panels {
            for b in &panels {
                if a.id != b.id {
                    let distance = wrap(a.yaw_deg - b.yaw_deg).abs();
                    assert!(distance + 0.001 >= (a.width_deg + b.width_deg) / 2.);
                }
            }
        }
    }
}
#[test]
fn minimize_recenters_the_remaining_lane_and_restores_from_taskbar() {
    let mut s = Scene::default();
    let a = s.add("a", "x");
    let b = s.add("b", "x");
    s.apply(Action::Minimize { id: b }).unwrap();
    near(s.panels().iter().find(|p| p.id == a).unwrap().yaw_deg, 0.);
    assert_eq!(s.focused, Some(a));
    s.apply(Action::Focus { id: b }).unwrap();
    assert!(!s.window(b).unwrap().minimized);
    assert_eq!(s.focused, Some(b));
}
#[test]
fn swapping_windows_preserves_exact_slots() {
    let mut s = Scene::default();
    let a = s.add("a", "x");
    let b = s.add("b", "x");
    s.apply(Action::Move {
        id: b,
        lane: 0,
        workspace: 1,
    })
    .unwrap();
    s.apply(Action::Swap { a, b }).unwrap();
    assert_eq!(
        (s.window(a).unwrap().lane, s.window(a).unwrap().workspace),
        (0, 1)
    );
    assert_eq!(
        (s.window(b).unwrap().lane, s.window(b).unwrap().workspace),
        (1, 0)
    );
    assert_eq!(s.workspaces[1].lanes[0].windows, vec![a]);
}
#[test]
fn whole_lane_swap_preserves_sizes_and_order() {
    let mut s = Scene::demo();
    let upper = s.workspaces[0].lanes[0].windows.clone();
    let lower = s.workspaces[0].lanes[2].windows.clone();
    let shares = s.workspaces[0].shares;
    s.apply(Action::SwapLanes { a: 0, b: 2 }).unwrap();
    assert_eq!(s.workspaces[0].lanes[2].windows, upper);
    assert_eq!(s.workspaces[0].lanes[0].windows, lower);
    assert_eq!(s.workspaces[0].shares, shares);
    for w in &s.windows {
        assert!(s.workspaces[w.workspace].lanes[w.lane]
            .windows
            .contains(&w.id));
    }
}
#[test]
fn inactive_desktops_are_behind_and_not_ray_targets() {
    let mut s = Scene::default();
    let a = s.add("a", "x");
    let b = s.add("b", "x");
    s.apply(Action::Move {
        id: b,
        lane: 1,
        workspace: 1,
    })
    .unwrap();
    let p = s.panels();
    assert!(
        p.iter().find(|p| p.id == b).unwrap().radius_m
            > p.iter().find(|p| p.id == a).unwrap().radius_m
    );
    let hit = s.hit_test(s.config.center_m, [0., 0., -1.]).unwrap();
    assert_eq!(hit.id, a);
    s.apply(Action::SwitchWorkspace { workspace: 1 }).unwrap();
    assert_eq!(s.hit_test(s.config.center_m, [0., 0., -1.]).unwrap().id, b);
}
#[test]
fn maximization_uses_entire_available_height_then_restores() {
    let mut s = Scene::default();
    let id = s.add("a", "x");
    let before = s.panels()[0].clone();
    s.apply(Action::Maximize { id }).unwrap();
    let p = s.panels()[0].clone();
    let (bottom, top) = s.config.lane_extent();
    near(p.bottom_deg, bottom);
    near(p.top_deg, top);
    assert!(p.width_deg > before.width_deg);
    s.apply(Action::Maximize { id }).unwrap();
    near(s.panels()[0].width_deg, before.width_deg);
}
#[test]
fn deleting_desktop_moves_clients_and_keeps_valid_indices() {
    let mut s = Scene::demo();
    let ids: Vec<_> = s.windows.iter().map(|w| w.id).collect();
    s.apply(Action::RemoveWorkspace {
        workspace: 0,
        move_to: 1,
    })
    .unwrap();
    assert_eq!(s.workspaces.len(), 2);
    assert_eq!(s.windows.iter().map(|w| w.id).collect::<Vec<_>>(), ids);
    assert!(s.windows.iter().all(|w| w.workspace < 2));
    for w in &s.windows {
        assert!(s.workspaces[w.workspace].lanes[w.lane]
            .windows
            .contains(&w.id));
    }
    s.apply(Action::RemoveWorkspace {
        workspace: 1,
        move_to: 0,
    })
    .unwrap();
    assert!(s
        .apply(Action::RemoveWorkspace {
            workspace: 0,
            move_to: 0
        })
        .is_err());
}
#[test]
fn desktop_manager_add_rename_and_switch() {
    let mut s = Scene::default();
    s.apply(Action::AddWorkspace {
        name: "Build".into(),
    })
    .unwrap();
    s.apply(Action::RenameWorkspace {
        workspace: 3,
        name: "Play".into(),
    })
    .unwrap();
    s.apply(Action::SwitchWorkspace { workspace: 3 }).unwrap();
    assert_eq!(s.workspaces[s.active_workspace].name, "Play");
    assert!(s.apply(Action::AddWorkspace { name: " ".into() }).is_err());
}
#[test]
fn mode_switch_keeps_windows_order_and_focus() {
    let mut s = Scene::demo();
    let old = s.clone();
    let mut c = s.config.clone();
    c.projection = Projection::InView;
    s.apply(Action::Configure { config: c }).unwrap();
    assert_eq!(s.focused, old.focused);
    for (a, b) in s.workspaces.iter().zip(old.workspaces.iter()) {
        assert_eq!(a.shares, b.shares);
        for (a, b) in a.lanes.iter().zip(b.lanes.iter()) {
            assert_eq!(a.windows, b.windows);
        }
    }
    assert_ne!(s.panels()[0].top_deg, old.panels()[0].top_deg);
}
#[test]
fn view_pose_moves_only_in_view_lanes() {
    let mut s = Scene::default();
    s.add("a", "x");
    let before = s.panels()[0].center_m;
    let pose = Pose {
        position_m: [3., 2., 1.],
        orientation_xyzw: [0., -0.5, 0., (3f32).sqrt() / 2.],
    };
    s.apply(Action::ViewPose { pose }).unwrap();
    assert_eq!(s.panels()[0].center_m, before);
    let mut c = s.config.clone();
    c.projection = Projection::InView;
    s.apply(Action::Configure { config: c }).unwrap();
    assert_ne!(s.panels()[0].center_m, before);
    assert_eq!(
        s.hit_test(pose.position_m, pose.rotate([0., 0., -1.]))
            .unwrap()
            .id,
        1
    );
}
#[test]
fn room_raycast_honors_rotated_and_translated_anchor() {
    let mut s = Scene::default();
    s.add("a", "x");
    s.config.center_m = [4., 1., -2.];
    s.config.yaw_deg = 90.;
    let hit = s.hit_test([4., 1., -2.], [1., 0., 0.]).unwrap();
    assert_eq!(hit.id, 1);
    near(hit.uv[0], 0.5);
    assert!(s.hit_test([4., 1., -2.], [-1., 0., 0.]).is_none());
    assert!(s.hit_test([f32::NAN, 0., 0.], [1., 0., 0.]).is_none());
}
#[test]
fn curved_mesh_stays_on_open_cylinder() {
    let mut s = Scene::default();
    s.add("a", "x");
    let p = &s.panels()[0];
    let mesh = s.mesh(p, 32);
    assert_eq!(mesh.len(), 66);
    for v in mesh {
        near(
            (v[0] - s.config.center_m[0]).hypot(v[2] - s.config.center_m[2]),
            s.config.radius_m,
        );
    }
}
#[test]
fn live_mode_never_synthesizes_client_windows() {
    let mut s = Scene::new("wayland");
    assert!(s
        .apply(Action::Add {
            title: "x".into(),
            app_id: "x".into()
        })
        .is_err());
    assert!(s.windows.is_empty());
}

#[test]
fn in_view_peripheral_windows_fit_between_flat_trays() {
    let mut s = Scene::default();
    s.config.projection = Projection::InView;
    let (_, top) = s.config.lane_extent();
    for yaw in [-70., -35., 0., 35., 70.] {
        let p = s.point(yaw, top, s.config.radius_m);
        let local = s
            .view_pose
            .inverse_rotate(std::array::from_fn(|i| p[i] - s.view_pose.position_m[i]));
        near(local[1].atan2(-local[2]).to_degrees(), top);
    }
}
#[test]
fn geometry_changes_keep_each_lane_central_window_in_place() {
    let mut s = Scene::demo();
    let yaw = s.panels().iter().find(|p| p.id == 3).unwrap().yaw_deg;
    near(yaw, 0.);
    let mut c = s.config.clone();
    c.projection = Projection::InView;
    s.apply(Action::Configure { config: c }).unwrap();
    near(s.panels().iter().find(|p| p.id == 3).unwrap().yaw_deg, yaw);
    s.apply(Action::ResizeBoundary {
        boundary: 0,
        delta_deg: 2.,
    })
    .unwrap();
    near(s.panels().iter().find(|p| p.id == 3).unwrap().yaw_deg, yaw);
}
#[test]
fn swaps_and_maximize_do_not_leave_focus_on_a_hidden_desktop() {
    let mut s = Scene::default();
    let a = s.add("a", "x");
    let b = s.add("b", "x");
    s.apply(Action::Move {
        id: b,
        lane: 1,
        workspace: 1,
    })
    .unwrap();
    s.apply(Action::Swap { a, b }).unwrap();
    assert_eq!(s.focused, Some(b));
    s.apply(Action::Maximize { id: a }).unwrap();
    assert_eq!(s.active_workspace, 1);
    assert_eq!(s.focused, Some(a));
}

#[test]
fn display_profiles_migrate_and_reject_invalid_coordinates_without_mutation() {
    let old: Config = serde_json::from_str("{}").unwrap();
    assert!(old.displays.is_empty());
    let mut scene = Scene::demo();
    let before = scene.config.clone();
    let mut config = before.clone();
    config.displays.push(DisplayConfig {
        name: "0000:03:00.0-DP-5".into(),
        x: Some(i32::MIN),
        ..Default::default()
    });
    assert!(scene
        .apply(Action::Configure {
            config: config.clone()
        })
        .is_err());
    assert_eq!(scene.config, before);
    config.displays[0].x = Some(-1920);
    config.validate().unwrap();
    let encoded = serde_json::to_string(&config).unwrap();
    assert_eq!(serde_json::from_str::<Config>(&encoded).unwrap(), config);
    config.displays.push(config.displays[0].clone());
    assert!(config.validate().is_err());
}
