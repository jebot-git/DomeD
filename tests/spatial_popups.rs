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
fn oversized_xr_popups_use_half_resolution_and_keep_full_surface_input() {
    for resolution in dome::xr_window_size::Resolution::ALL {
        let (mut scene, root, popup) = fixture(Projection::RoomDome);
        scene.config.xr_window_resolution = resolution;
        let [width,height] = resolution.pixels();
        let window = scene.windows.iter_mut().find(|w| w.id == root).unwrap();
        window.desktop.w = width as f32;
        window.desktop.h = height as f32;
        scene.popups[0].anchor.w = width as f32;
        scene.popups[0].anchor.h = height as f32;
        let original = scene.popups[0].anchor;
        let head = Pose::default();
        let quads = xr_input::quads(&scene,head);
        let parent = quads.iter().find(|q| q.id == root).unwrap();
        let panel = quads.iter().find(|q| q.id == popup).unwrap();
        assert!(panel.size[0] <= parent.size[0] * 0.5 + 1e-5);
        assert!(panel.size[1] <= parent.size[1] * 0.5 + 1e-5);
        assert!((panel.size[0]/parent.size[0] - panel.size[1]/parent.size[1]).abs() < 1e-5);
        assert_eq!(scene.popups[0].anchor,original);
        for uv in [[0.05,0.1],[0.5,0.5],[0.95,0.9]] {
            let origin = Vec3::from_array(head.position_m);
            let hit = panel.hit(origin,(panel.point(uv)-origin).normalize(),false).unwrap();
            assert!((hit[0]-uv[0]).abs()<1e-5 && (hit[1]-uv[1]).abs()<1e-5);
        }
    }
}

#[test]
fn small_popups_keep_their_size_across_resolution_presets() {
    let (mut scene,_,popup) = fixture(Projection::RoomDome);
    let mut previous = None;
    for resolution in dome::xr_window_size::Resolution::ALL {
        scene.config.xr_window_resolution = resolution;
        let quads = xr_input::quads(&scene,Pose::default());
        let panel = quads.iter().find(|q| q.id == popup).unwrap();
        if let Some(size) = previous { assert_eq!(panel.size,size); }
        previous = Some(panel.size);
    }
}

#[test]
fn desktop_uses_hierarchy_anchor_or_cursor_and_keeps_popups_on_screen() {
    let work = Rect { x: 0., y: 36., w: 1280., h: 704. };
    let parent = Rect { x: 100., y: 80., w: 720., h: 460. };
    let anchored = spatial_popup::desktop_rect(parent, Some([120., 30.]), [900., 400.], [240., 180.], work);
    assert_eq!((anchored.x, anchored.y), (220., 110.));
    let cursor = spatial_popup::desktop_rect(parent, None, [450., 250.], [240., 180.], work);
    assert_eq!((cursor.x, cursor.y), (450., 258.));
    let edge = spatial_popup::desktop_rect(parent, None, [1270., 730.], [240., 180.], work);
    assert_eq!((edge.x, edge.y), (1040., 560.));
    let oversized = spatial_popup::desktop_rect(parent, None, [450., 250.], [1400., 900.], work);
    assert_eq!((oversized.x, oversized.y, oversized.w, oversized.h), (0., 36., 1400., 900.));
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
fn dome_popup_centers_slightly_down_and_forward_with_beam_back_to_origin() {
    let (scene, root, popup) = fixture(Projection::RoomDome);
    let head = Pose::default();
    let quads = xr_input::quads(&scene, head);
    let parent = quads.iter().find(|q| q.id == root).unwrap();
    let panel = quads.iter().find(|q| q.id == popup).unwrap();
    let delta = Quat::from_array(parent.pose.orientation_xyzw).conjugate()
        * (Vec3::from_array(panel.pose.position_m) - Vec3::from_array(parent.pose.position_m));
    assert!(parent.dome.is_some() && panel.dome.is_none());
    let distance = parent.point([0.5,0.5]).distance(Vec3::from_array(head.position_m));
    assert!(delta.x.abs() < 1e-5);
    assert!((delta.y + distance * 5_f32.to_radians().tan()).abs() < 1e-5);
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
    scene.apply(dome::Action::PullOut { id: root, head }).unwrap();
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

#[test]
fn every_popup_stays_between_actual_bar_edges_with_shared_ray_geometry() {
    for lane in 0..3 {
        for tilt in [0., 25.] {
            let (mut scene, root, popup)=fixture(Projection::RoomDome);
            scene.config.dome_upper_tilt_deg=tilt;
            scene.apply(dome::Action::Move{id:root,lane,workspace:0}).unwrap();
            scene.popups[0].anchor.h=5000.;
            let child=POPUP_ID_BASE+2;
            scene.popups.push(Popup{id:child,parent:popup,root,context_at:None,viewport:None,
                anchor:Rect{x:0.,y:0.,w:400.,h:3000.}});
            let head=Pose{orientation_xyzw:(Quat::from_rotation_y(0.4)*Quat::from_rotation_x(0.25)).to_array(),..Pose::default()};
            let shell=dome::xr_transport::ShellLayout{width:1024,height:768,top:36,bottom:60,..Default::default()};
            let bars=xr_input::shell_quads_in_view(&scene,head,&shell,100.);
            let frame=head.upright();
            let slope=|p:Vec3| {let p=frame.inverse_rotate((p-Vec3::from_array(frame.position_m)).to_array());p[1]/p[0].hypot(p[2])};
            let top=bars.iter().find(|q|q.id==dome::xr_transport::TOP_BAR).unwrap();
            let bottom=bars.iter().find(|q|q.id==dome::xr_transport::TASK_BAR).unwrap();
            let upper=[0.,0.5,1.].map(|u|slope(top.point([u,1.]))).into_iter().fold(f32::INFINITY,f32::min);
            let lower=[0.,0.5,1.].map(|u|slope(bottom.point([u,0.]))).into_iter().fold(f32::NEG_INFINITY,f32::max);
            let quads=xr_input::quads(&scene,head);
            for id in [popup,child] {
                let q=quads.iter().find(|q|q.id==id).unwrap();
                for uv in [[0.,0.],[1.,0.],[0.,1.],[1.,1.]] {
                    let value=slope(q.point(uv));
                    assert!(value>=lower-1e-5 && value<=upper+1e-5,"lane {lane}: {value} outside {lower}..{upper}");
                }
                let origin=Vec3::from_array(head.position_m);
                let target=q.point([0.3,0.7]);
                let hit=q.hit(origin,(target-origin).normalize(),false).unwrap();
                assert!((hit[0]-0.3).abs()<1e-4 && (hit[1]-0.7).abs()<1e-4);
            }
        }
    }
}

#[test]
fn popup_centers_below_the_explicit_click_origin_without_adding_its_height() {
    let (mut scene, root, popup) = fixture(Projection::RoomDome);
    scene.popups[0].context_at = Some([200.,100.]);
    let head = Pose { position_m: [0.,1.4,0.], ..Pose::default() };
    let quads = xr_input::quads(&scene,head);
    let parent = quads.iter().find(|q|q.id==root).unwrap();
    let panel = quads.iter().find(|q|q.id==popup).unwrap();
    let origin = parent.point([200./720.,100./460.]);
    let delta = Quat::from_array(parent.pose.orientation_xyzw).conjugate()
        * (panel.point([0.5,0.5])-origin);
    assert!(delta.x.abs()<1e-5 && delta.y<0. && delta.y.abs()<0.4);
    assert!((delta.z-0.14).abs()<1e-5);
}

#[test]
fn nested_dialogs_fit_narrow_asymmetric_binocular_fov_and_keep_render_input_identical() {
    for anchor in [dome::AnchorMode::Room,dome::AnchorMode::Head] {
        for lane in 0..3 {
            let (mut scene,root,popup)=fixture(Projection::RoomDome);
            scene.config.anchor_mode=anchor;
            scene.config.dome_upper_tilt_deg=25.;
            scene.apply(dome::Action::Move{id:root,lane,workspace:0}).unwrap();
            scene.popups[0].anchor.w=1920.;scene.popups[0].anchor.h=1080.;
            let child=POPUP_ID_BASE+2;
            scene.popups.push(Popup{id:child,parent:popup,root,context_at:None,viewport:None,
                anchor:Rect{x:0.,y:0.,w:960.,h:540.}});
            let head=Pose{position_m:[0.2,1.4,0.1],orientation_xyzw:
                (Quat::from_rotation_y(0.4)*Quat::from_rotation_x(-0.2)*Quat::from_rotation_z(0.1)).to_array()};
            let fov=[-0.40_f32,0.47,-0.32,0.36];
            let eyes=[-1.,1.].map(|side|(Pose{
                position_m:head.transform([side*0.036,0.,0.01]),
                orientation_xyzw:(Quat::from_array(head.orientation_xyzw)*Quat::from_rotation_y(side*0.08)).to_array(),
            },fov));
            let viewport=xr_input::shared_viewport(head,&eyes,1.).unwrap();
            let input=xr_input::quads_in_fov(&scene,head,Some(viewport));
            let rendered=xr_input::render_quads_in_fov(&scene,head,Some(viewport));
            for id in [popup,child] {
                let panel=input.iter().find(|q|q.id==id).unwrap();
                let draw=rendered.iter().find(|q|q.id==id).unwrap();
                assert_eq!(panel.pose,draw.pose);assert_eq!(panel.size,draw.size);
                for uv in [[0.,0.],[1.,0.],[0.,1.],[1.,1.],[0.5,0.5]] {
                    for (eye,fov) in eyes {
                        let p=eye.inverse_rotate((panel.point(uv)-Vec3::from_array(eye.position_m)).to_array());
                        assert!(p[2]<0.);
                        let x=p[0]/-p[2];let y=p[1]/-p[2];
                        assert!(x>=fov[0].tan()-1e-4 && x<=fov[1].tan()+1e-4,"lane {lane}: x={x}");
                        assert!(y>=fov[2].tan()-1e-4 && y<=fov[3].tan()+1e-4,"lane {lane}: y={y}");
                    }
                }
                let eye=Vec3::from_array(head.position_m);
                for uv in [[0.1,0.2],[0.5,0.5],[0.9,0.8]] {
                    let hit=panel.hit(eye,(panel.point(uv)-eye).normalize(),false).unwrap();
                    assert!((hit[0]-uv[0]).abs()<1e-4 && (hit[1]-uv[1]).abs()<1e-4);
                }
            }
        }
    }
}

#[test]
fn restarted_xr_helper_negotiates_popup_geometry_with_the_running_desktop() {
    let (scene, root, popup)=fixture(Projection::RoomDome);
    let head=Pose{position_m:[0.,1.4,0.],..Pose::default()};
    let bridge=dome::xr_transport::Bridge::default();
    let current=bridge.control();assert!(current.centered_popups);
    let mut wire=serde_json::to_value(&current).unwrap();
    wire.as_object_mut().unwrap().remove("centered_popups");
    let legacy: dome::xr_transport::Control=serde_json::from_value(wire).unwrap();
    assert!(!legacy.centered_popups);
    let old=xr_input::render_quads_for_control(&scene,head,None,&legacy);
    let new=xr_input::render_quads_for_control(&scene,head,None,&current);
    let root=old.iter().find(|q|q.id==root).unwrap();
    let old=old.iter().find(|q|q.id==popup).unwrap();
    let new=new.iter().find(|q|q.id==popup).unwrap();
    assert!(old.pose.position_m[1]>root.pose.position_m[1]);
    assert!(new.pose.position_m[1]<root.pose.position_m[1]);
}

#[test]
fn offscreen_parent_dialog_remains_readable_and_at_its_original_viewing_distance() {
    let (mut scene, root, popup)=fixture(Projection::RoomDome);
    let head=Pose{position_m:[0.,1.4,0.],..Pose::default()};
    scene.config.yaw_deg=-90.;
    let quads=xr_input::quads_in_fov(&scene,head,Some([-0.4,0.47,-0.32,0.36]));
    let dialog=quads.iter().find(|q|q.id==popup).unwrap();
    let local=head.inverse_rotate((dialog.point([0.5,0.5])-Vec3::from_array(head.position_m)).to_array());
    assert!(-local[2]>1.8 && local[1]<0., "dialog center {local:?}");
    let normal=Quat::from_array(dialog.pose.orientation_xyzw)*Vec3::Z;
    assert!(normal.dot(Vec3::Z)>0.99);
}
