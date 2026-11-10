use dome::{Action,Scene,shortcuts::Mods,super_mmb::*};
#[test]
fn shift_selects_all_lanes_only_in_xr() {
    let logo=Mods{logo:true,..Default::default()};
    for xr in [false,true] {
        assert!(starts(MIDDLE,logo,xr));assert!(!starts(MIDDLE,Default::default(),xr));
        for mods in [Mods{ctrl:true,..logo},Mods{alt:true,..logo}] {assert!(!starts(MIDDLE,mods,xr));}
        assert!(!starts(0x110,logo,xr));
    }
    let shift=Mods{shift:true,..logo};
    assert!(starts(MIDDLE,shift,true));assert!(!starts(MIDDLE,shift,false));
    assert!(!starts(MIDDLE,Mods{ctrl:true,..shift},true));
    assert!(!starts(MIDDLE,Mods{alt:true,..shift},true));
    assert!(lane_delta(f64::NAN).is_none());
}
#[test]
fn all_active_lanes_rotate_together_without_focus_order_or_other_workspace_changes() {
    let mut scene=Scene::demo();let old=scene.clone();
    scene.apply(Action::RotateLanes{workspace:old.active_workspace,delta_deg:lane_delta(100.).unwrap()}).unwrap();
    assert_eq!(scene.focused,old.focused);
    for (i,(a,b)) in scene.workspaces.iter().zip(&old.workspaces).enumerate() {
        for (new,old) in a.lanes.iter().zip(&b.lanes) {
            assert_eq!(new.windows,old.windows);
            if i==scene.active_workspace {assert!(dome::wrap(new.rotation_deg-old.rotation_deg-14.323945).abs()<0.0001);}else{assert_eq!(new.rotation_deg,old.rotation_deg);}
        }
    }
    scene.apply(Action::RotateLanes{workspace:scene.active_workspace,delta_deg:lane_delta(-100.).unwrap()}).unwrap();
    for lane in 0..3 {assert!(dome::wrap(scene.workspaces[scene.active_workspace].lanes[lane].rotation_deg-old.workspaces[old.active_workspace].lanes[lane].rotation_deg).abs()<0.0001);}
    assert!(scene.apply(Action::RotateLanes{workspace:999,delta_deg:1.}).is_err());
    assert!(scene.apply(Action::RotateLanes{workspace:0,delta_deg:f32::INFINITY}).is_err());
}

#[test]
fn each_lane_rotates_independently_and_wraps_without_changing_focus_or_order() {
    for lane in 0..3 {
        let mut scene=Scene::demo();
        let workspace=scene.active_workspace;
        scene.workspaces[workspace].lanes[lane].rotation_deg=179.;
        let old=scene.clone();
        scene.apply(Action::RotateLane{workspace,lane,delta_deg:lane_delta(100.).unwrap()}).unwrap();
        assert_eq!(scene.focused,old.focused);
        for (ws,(new,old)) in scene.workspaces.iter().zip(&old.workspaces).enumerate() {
            for (index,(new,old)) in new.lanes.iter().zip(&old.lanes).enumerate() {
                assert_eq!(new.windows,old.windows);
                if ws==workspace && index==lane {
                    assert!((new.rotation_deg-dome::wrap(old.rotation_deg+14.323945)).abs()<0.0001);
                } else {assert_eq!(new.rotation_deg,old.rotation_deg);}
            }
        }
        scene.apply(Action::RotateLane{workspace,lane,delta_deg:lane_delta(-100.).unwrap()}).unwrap();
        assert!((scene.workspaces[workspace].lanes[lane].rotation_deg-179.).abs()<0.0001);
    }
}
#[test]
fn invalid_single_lane_rotations_leave_the_scene_unchanged() {
    let mut scene=Scene::demo();let old=serde_json::to_value(&scene).unwrap();
    for (workspace,lane,delta_deg) in [(999,0,1.),(0,3,1.),(0,0,f32::NAN),(0,0,f32::INFINITY)] {
        assert!(scene.apply(Action::RotateLane{workspace,lane,delta_deg}).is_err());
        assert_eq!(serde_json::to_value(&scene).unwrap(),old);
    }
}
#[test]
fn xr_target_selects_owner_lane_and_rejects_non_lane_surfaces() {
    let mut scene=Scene::demo();
    let owner=scene.windows.iter().find(|w| w.workspace==scene.active_workspace && !w.minimized && w.xr_visible).unwrap().id;
    let lane=scene.window(owner).unwrap().lane;
    assert_eq!(xr_lane(&scene,Some(owner)),Some(lane));
    let popup=dome::spatial_popup::POPUP_ID_BASE;
    scene.popups.push(dome::spatial_popup::Popup{id:popup,parent:owner,root:owner,
        anchor:dome::Rect{x:0.,y:0.,w:100.,h:100.},rollout:false,context_at:None,viewport:None});
    scene.popups.push(dome::spatial_popup::Popup{id:popup+1,parent:popup,root:owner,
        anchor:dome::Rect{x:0.,y:0.,w:100.,h:100.},rollout:true,context_at:None,viewport:None});
    assert_eq!(xr_lane(&scene,Some(popup)),Some(lane));
    assert_eq!(xr_lane(&scene,Some(popup+1)),Some(lane));
    assert_eq!(xr_lane(&scene,None),None);
    assert_eq!(xr_lane(&scene,Some(u64::MAX)),None);
    assert_eq!(xr_lane(&scene,Some(dome::xr_transport::TASK_BAR)),None);
    let index=scene.windows.iter().position(|w| w.id==owner).unwrap();
    scene.windows[index].pullout_slot=Some(0);
    assert_eq!(xr_lane(&scene,Some(owner)),None);
    assert_eq!(xr_lane(&scene,Some(popup)),None);
    scene.windows[index].pullout_slot=None;
    scene.windows[index].xr_visible=false;
    assert_eq!(xr_lane(&scene,Some(owner)),None);
    scene.windows[index].xr_visible=true;
    scene.windows[index].minimized=true;
    assert_eq!(xr_lane(&scene,Some(popup)),None);
    scene.windows[index].minimized=false;
    scene.active_workspace=(scene.active_workspace+1)%scene.workspaces.len();
    assert_eq!(xr_lane(&scene,Some(owner)),None);
}
