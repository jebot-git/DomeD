use dome::{Action,Scene,shortcuts::Mods,super_mmb::*};
#[test]
fn only_unassigned_exact_super_middle_starts() {
    let logo=Mods{logo:true,..Default::default()};
    assert!(starts(MIDDLE,logo));assert!(!starts(MIDDLE,Default::default()));
    for mods in [Mods{ctrl:true,..logo},Mods{alt:true,..logo},Mods{shift:true,..logo}] {assert!(!starts(MIDDLE,mods));}
    assert!(!starts(0x110,logo));assert!(lane_delta(f64::NAN).is_none());
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
