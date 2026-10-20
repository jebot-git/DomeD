use dome::{
    spatial_popup::{self, Popup, POPUP_ID_BASE},
    xr_input, Pose, Projection, Rect, Scene,
};
use glam::{Quat, Vec3};

fn fixture(projection: Projection) -> (Scene, u64, u64) {
    let mut scene = Scene::new("test");
    scene.config.projection = projection;
    let root = scene.add("Parent", "test");
    let popup = POPUP_ID_BASE + 1;
    scene.popups.push(Popup {
        context_at: None,
        viewport: None,
        id: popup,
        parent: root,
        root,
        anchor: Rect {
            x: 200.,
            y: 100.,
            w: 240.,
            h: 180.,
        },
    });
    (scene, root, popup)
}

#[test]
fn desktop_prefers_right_panel_and_falls_back_in_front_within_work_area() {
    let work = Rect {
        x: 0.,
        y: 36.,
        w: 1280.,
        h: 704.,
    };
    let parent = Rect {
        x: 100.,
        y: 80.,
        w: 720.,
        h: 460.,
    };
    let side = spatial_popup::desktop_rect(parent, [240., 180.], work);
    assert_eq!(side.x, 828.);
    assert_eq!(side.y, 80.);
    let front = spatial_popup::desktop_rect(Rect { x: 500., ..parent }, [240., 180.], work);
    assert!(front.x >= 500. && front.x + front.w <= 1220.);
    let tall = spatial_popup::desktop_rect(parent, [240., 700.], work);
    assert!(tall.y >= work.y && tall.y + tall.h <= work.y + work.h);
    let exact = spatial_popup::desktop_rect(parent, [452., 180.], work);
    assert_eq!(exact.x, 828.);
}

#[test]
fn desktop_menus_use_configured_geometry_and_nested_flipped_edges() {
    let (mut scene, root, popup) = fixture(Projection::RoomDome);
    let work = Rect {
        x: 0.,
        y: 36.,
        w: 1280.,
        h: 704.,
    };
    let child = POPUP_ID_BASE + 2;
    scene.popups.insert(
        0,
        Popup {
            id: child,
            parent: popup,
            root,
            context_at: None,
            viewport: None,
            anchor: Rect {
                x: -120.,
                y: 25.,
                w: 120.,
                h: 100.,
            },
        },
    );
    let rects = scene.popup_desktop_rects(work);
    let parent = scene.window(root).unwrap().desktop;
    assert_eq!(rects[0].0, popup);
    assert_eq!(rects[1].0, child);
    assert_eq!(
        (rects[0].1.x, rects[0].1.y),
        (parent.x + 200., parent.y + 100.)
    );
    assert_eq!(rects[1].1.x + rects[1].1.w, rects[0].1.x);
    assert_eq!(rects[1].1.y, rects[0].1.y + 25.);
    scene
        .windows
        .iter_mut()
        .find(|w| w.id == root)
        .unwrap()
        .maximized = true;
    let rects = scene.popup_desktop_rects(work);
    assert_eq!((rects[0].1.x, rects[0].1.y), (work.x + 200., work.y + 100.));
}

#[test]
fn dome_popup_is_flat_and_ray_targets_match_its_surface() {
    let (scene, root, popup) = fixture(Projection::RoomDome);
    let head = Pose::default();
    let quads = xr_input::quads(&scene, head);
    let parent = quads.iter().find(|q| q.id == root).unwrap();
    let panel = quads.iter().find(|q| q.id == popup).unwrap();
    assert!(panel.dome.is_none());
    let offset = Quat::from_array(parent.pose.orientation_xyzw).conjugate()
        * (Vec3::from_array(panel.pose.position_m) - Vec3::from_array(parent.pose.position_m));
    assert!((offset.z - 0.14).abs() < 1e-5);
    for uv in [[0.1, 0.2], [0.5, 0.5], [0.9, 0.8]] {
        let origin = Vec3::from_array(head.position_m);
        let actual = panel
            .hit(origin, (panel.point(uv) - origin).normalize(), false)
            .unwrap();
        assert!((actual[0] - uv[0]).abs() < 1e-5 && (actual[1] - uv[1]).abs() < 1e-5);
    }
    assert!(!spatial_popup::beams(&scene, &quads, head).is_empty());
}

#[test]
fn dome_popup_floats_above_and_forward_with_beam_back_to_origin() {
    let (scene, root, popup) = fixture(Projection::RoomDome);
    let head = Pose::default();
    let quads = xr_input::quads(&scene, head);
    let parent = quads.iter().find(|q| q.id == root).unwrap();
    let panel = quads.iter().find(|q| q.id == popup).unwrap();
    let delta = Quat::from_array(parent.pose.orientation_xyzw).conjugate()
        * (Vec3::from_array(panel.pose.position_m) - Vec3::from_array(parent.pose.position_m));
    assert!(parent.dome.is_some() && panel.dome.is_none());
    assert!(delta.y > parent.size[1] / 2. + panel.size[1] / 2.);
    assert!((delta.z - 0.14).abs() < 1e-5);
    let beams = spatial_popup::beams(&scene, &quads, head);
    assert_eq!(beams.len(), 1);
    assert!(beams[0].point([0.5, 0.]).distance(panel.point([0.5, 1.])) < 0.003);
    let origin = parent.point([200. / 720., 100. / 460.]);
    assert!(beams[0].point([0.5, 1.]).distance(origin) < 0.003);
    assert!(!quads.iter().any(|q| q.id == beams[0].id));
}

#[test]
fn nested_popups_follow_parent_and_inherit_visibility_without_lane_slots() {
    let (mut scene, root, popup) = fixture(Projection::RoomDome);
    let child = POPUP_ID_BASE + 2;
    scene.popups.insert(
        0,
        Popup {
            context_at: None,
            viewport: None,
            id: child,
            parent: popup,
            root,
            anchor: Rect {
                x: 20.,
                y: 20.,
                w: 120.,
                h: 100.,
            },
        },
    );
    let head = Pose::default();
    let before = xr_input::quads(&scene, head);
    assert_eq!(before.len(), 3);
    assert!(before.iter().position(|q| q.id == child) > before.iter().position(|q| q.id == popup));
    let pose = Pose {
        position_m: [0.5, 0., -1.],
        ..head
    };
    scene
        .apply(dome::Action::SpatialTransform {
            id: root,
            pose: Some(pose),
            scale: 1.5,
        })
        .unwrap();
    let after = xr_input::quads(&scene, head);
    let old = before.iter().find(|q| q.id == child).unwrap();
    let new = after.iter().find(|q| q.id == child).unwrap();
    assert!(new.size[0] > old.size[0]);
    assert_ne!(new.pose.position_m, old.pose.position_m);
    scene.apply(dome::Action::Minimize { id: root }).unwrap();
    assert!(xr_input::quads(&scene, head).is_empty());
    assert!(scene
        .popup_desktop_rects(Rect {
            x: 0.,
            y: 36.,
            w: 1280.,
            h: 704.
        })
        .is_empty());
    scene.apply(dome::Action::Focus { id: root }).unwrap();
    scene
        .apply(dome::Action::SwitchWorkspace { workspace: 1 })
        .unwrap();
    assert!(xr_input::quads(&scene, head).is_empty());
    scene.apply(dome::Action::Close { id: root }).unwrap();
    assert!(xr_input::quads(&scene, head).is_empty());
}

#[test]
fn parented_dialog_is_a_popup_instead_of_an_independent_dome_card() {
    let (mut scene, root, _) = fixture(Projection::RoomDome);
    let dialog = scene.add("Dialog", "test");
    scene.popups.push(Popup {
        context_at: None,
        viewport: None,
        id: dialog,
        parent: root,
        root,
        anchor: Rect {
            x: 100.,
            y: 100.,
            w: 320.,
            h: 200.,
        },
    });
    assert_eq!(scene.panels().len(), 1);
    let quads = xr_input::quads(&scene, Pose::default());
    assert_eq!(quads.iter().filter(|q| q.id == dialog).count(), 1);
    assert!(quads
        .iter()
        .find(|q| q.id == dialog)
        .unwrap()
        .dome
        .is_none());
    scene.apply(dome::Action::Close { id: root }).unwrap();
    assert!(scene.windows.is_empty() && scene.popups.is_empty());
}
