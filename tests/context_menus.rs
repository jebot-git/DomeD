use dome::{
    context_menu::{self, Viewport},
    spatial_popup::{Popup, POPUP_ID_BASE},
    xr_input, Pose, Projection, Rect, Scene,
};
use glam::{Quat, Vec3};

#[test]
fn above_click_placement_clamps_to_work_area() {
    let parent = Rect {
        x: 100.,
        y: 80.,
        w: 720.,
        h: 460.,
    };
    let work = Rect {
        x: 0.,
        y: 36.,
        w: 1280.,
        h: 704.,
    };
    let r = context_menu::rect(parent, [500., 300.], [240., 180.], work);
    assert_eq!((r.x, r.y), (480., 192.));
    assert_eq!(r.y + r.h + context_menu::GAP, parent.y + 300.);
    let r = context_menu::rect(parent, [-100., 0.], [240., 704.], work);
    assert_eq!((r.x, r.y), (0., 36.));
}

#[test]
fn scroll_viewport_keeps_controls_out_of_content_and_reveals_last_item() {
    let mut v = Viewport::new(1800., 704., 0.);
    assert_eq!(v.band(), 24.);
    assert_eq!(v.control(12.), Some(-1));
    assert_eq!(v.control(692.), Some(1));
    assert_eq!(v.control(200.), None);
    v.scroll(96.);
    assert_eq!(v.content_y(44.), 116.);
    v.reveal(1760., 32.);
    assert!(v.offset <= 1760. && v.offset + v.body() >= 1792.);
    v.scroll(10000.);
    assert_eq!(v.offset, v.max_offset());
    v.scroll(-10000.);
    assert_eq!(v.offset, 0.);
    assert_eq!(Viewport::new(180., 704., 0.).band(), 0.);
    assert!(Viewport::new(1800., 20., 0.).height <= 20.);
}

#[test]
fn context_panel_tracks_the_click_in_both_xr_projections_and_dome_beam_ends_there() {
    for projection in [Projection::RoomDome] {
        let mut scene = Scene::new("test");
        scene.config.projection = projection;
        let root = scene.add("parent", "test");
        let point = [500., 300.];
        let id = POPUP_ID_BASE + 1;
        scene.popups.push(Popup {
            id,
            parent: root,
            root,
            anchor: Rect {
                x: 0.,
                y: 0.,
                w: 240.,
                h: 180.,
            },
            context_at: Some(point),
            viewport: Some(Viewport::new(180., 704., 0.)),
        });
        let head = Pose {
            position_m: if projection == Projection::RoomDome {
                [0., 1.4, 0.]
            } else {
                [0., 0., 0.]
            },
            ..Pose::default()
        };
        let panels = xr_input::quads(&scene, head);
        let parent = panels.iter().find(|q| q.id == root).unwrap();
        let menu = panels.iter().find(|q| q.id == id).unwrap();
        let click = parent.point([point[0] / 720., point[1] / 460.]);
        let delta = Quat::from_array(parent.pose.orientation_xyzw).conjugate()
            * (menu.point([0.5, 1.]) - click);
        assert!(delta.x.abs() < 1e-5);
        assert!((delta.y - context_menu::GAP * parent.size[1] / 460.).abs() < 1e-5);
        assert!(
            (delta.z
                - if projection == Projection::RoomDome {
                    0.14
                } else {
                    0.06
                })
            .abs()
                < 1e-5
        );
        if projection == Projection::RoomDome {
            let beam = context_menu_beam(&scene, &panels, head);
            assert!(beam.point([0.5, 1.]).distance(click) < 0.003);
        }
        let origin = Vec3::from_array(head.position_m);
        let uv = menu
            .hit(origin, (menu.point([0.5, 0.5]) - origin).normalize(), false)
            .unwrap();
        assert!((uv[0] - 0.5).abs() < 1e-5 && (uv[1] - 0.5).abs() < 1e-5);
    }
}

fn context_menu_beam(scene: &Scene, panels: &[xr_input::Quad], head: Pose) -> xr_input::Quad {
    dome::spatial_popup::beams(scene, panels, head).remove(0)
}

#[test]
fn cropped_texture_and_content_coordinates_share_one_viewport() {
    let mut rgba = Vec::new();
    for y in 0..1000 {
        for _ in 0..16 {
            rgba.extend_from_slice(&[(y % 255) as u8, 80, 120, 255]);
        }
    }
    let v = Viewport::new(1000., 400., 200.);
    let out = context_menu::texture(&rgba, 16, v);
    assert_eq!(out.len(), 16 * 400 * 4);
    let y = 100;
    assert_eq!(
        out[(y * 16 + 2) * 4],
        (v.content_y(y as f32) as usize % 255) as u8
    );
    assert_ne!(&out[..16 * 24 * 4], &rgba[..16 * 24 * 4]);
}

#[test]
fn large_xr_context_menu_stays_between_shell_bars() {
    let config = dome::Config::default();
    let head = Pose::default();
    let mut menu = xr_input::Quad {
        id: 1,
        pose: Pose {
            position_m: [0., 2., -1.],
            ..head
        },
        size: [1., 4.],
        dome: None,
    };
    context_menu::fit_xr(&mut menu, head, &config);
    let upper =
        (config.vertical_fov_deg / 2. - config.upper_bar_height_deg)
            .to_radians();
    let lower =
        (-config.vertical_fov_deg / 2. + config.taskbar_height_deg)
            .to_radians();
    for uv in [[0., 0.], [1., 0.], [0., 1.], [1., 1.]] {
        let p = menu.point(uv) - Vec3::from_array(head.position_m);
        let angle = p.y.atan2(-p.z);
        assert!(angle <= upper + 1e-5 && angle >= lower - 1e-5);
    }
}
