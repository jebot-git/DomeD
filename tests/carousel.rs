use dome::{Action, Scene};

#[test]
fn arrows_skip_invisible_lane_entries_and_move_rendered_geometry() {
    let mut scene = Scene::new("test");
    let hidden = scene.add("desktop only", "desktop");
    let a = scene.add("a", "a");
    let minimized = scene.add("minimized", "minimized");
    let b = scene.add("b", "b");
    let pulled = scene.add("pulled", "pulled");
    let c = scene.add("c", "c");
    scene.workspaces[0].lanes[1].windows = vec![hidden, a, minimized, b, pulled, c];
    scene.windows.iter_mut().find(|w| w.id == hidden).unwrap().xr_visible = false;
    scene.windows.iter_mut().find(|w| w.id == minimized).unwrap().minimized = true;
    scene.windows.iter_mut().find(|w| w.id == pulled).unwrap().pullout_slot = Some(0);
    scene.pullout_anchor = Some(Default::default());
    scene.focused = Some(b);
    let before = scene.windows.clone();
    let pose = |scene: &Scene| dome::xr_input::render_quads(scene, scene.view_pose)
        .into_iter().find(|q| q.id == b).unwrap().pose;
    let first_pose = pose(&scene);
    scene.apply(Action::Reorder { id: b, delta: -1 }).unwrap();
    assert_eq!(scene.carousel_order(b), vec![b, a, c]);
    assert_ne!(pose(&scene).orientation_xyzw, first_pose.orientation_xyzw);
    assert_eq!(scene.workspaces[0].lanes[1].windows, vec![hidden, b, minimized, a, pulled, c]);
    let left_pose = pose(&scene);
    // At the visible edge, wrap past the desktop-only entry to the other end.
    scene.apply(Action::Reorder { id: b, delta: -1 }).unwrap();
    assert_eq!(scene.carousel_order(b), vec![a, c, b]);
    assert_ne!(pose(&scene).orientation_xyzw, left_pose.orientation_xyzw);
    scene.apply(Action::Reorder { id: b, delta: 1 }).unwrap();
    assert_eq!(scene.carousel_order(b), vec![b, a, c]);
    scene.apply(Action::Reorder { id: b, delta: 1 }).unwrap();
    assert_eq!(pose(&scene).orientation_xyzw, first_pose.orientation_xyzw);
    assert_eq!(scene.workspaces[0].lanes[1].windows, vec![hidden, a, minimized, b, pulled, c]);
    assert_eq!(serde_json::to_value(scene.windows).unwrap(), serde_json::to_value(before).unwrap());
    assert_eq!(scene.focused, Some(b));
}

#[test]
fn invisible_window_cannot_reorder_the_carousel() {
    let mut scene = Scene::new("test");
    let hidden = scene.add("desktop only", "desktop");
    let visible = scene.add("visible", "visible");
    scene.windows.iter_mut().find(|w| w.id == hidden).unwrap().xr_visible = false;
    let before = scene.workspaces[0].lanes[1].windows.clone();
    assert!(scene.apply(Action::Reorder { id: hidden, delta: -1 }).is_err());
    assert_eq!(scene.workspaces[0].lanes[1].windows, before);
    assert_eq!(scene.carousel_order(visible), vec![visible]);
    scene.apply(Action::Reorder { id: visible, delta: -1 }).unwrap();
    assert_eq!(scene.workspaces[0].lanes[1].windows, before);
}
