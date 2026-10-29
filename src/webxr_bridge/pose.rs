//! Preserve independently meaningful OpenXR pose components for transport.
use super::{Pose, Tracking};
const ORIENTATION: u64 = 1;
const POSITION: u64 = 2;
const ORIENTATION_TRACKED: u64 = 4;
const POSITION_TRACKED: u64 = 8;
fn orientation_valid(p: &Pose) -> bool {
    p.orientation.iter().all(|v| v.is_finite()) &&
        (p.orientation.iter().map(|v|v*v).sum::<f32>() - 1.).abs() < 0.01
}
fn position_valid(p: &Pose) -> bool {
    p.position.iter().all(|v| v.is_finite() && v.abs() < 10000.)
}
fn flags(flags:u64, orientation:bool, position:bool) -> u64 {
    let flags = if orientation { flags } else { flags & !(ORIENTATION|ORIENTATION_TRACKED) };
    if position { flags } else { flags & !(POSITION|POSITION_TRACKED) }
}
fn apply(p:&mut Pose, flags:u64) {
    if flags & ORIENTATION == 0 { p.orientation = [0.,0.,0.,1.]; }
    if flags & POSITION == 0 { p.position = [0.;3]; }
}
pub fn sanitize_poses(frame:&mut Tracking) {
    frame.head_flags = flags(frame.head_flags,
        frame.head_flags & ORIENTATION != 0 && orientation_valid(&frame.head),
        frame.head_flags & POSITION != 0 && position_valid(&frame.head));
    apply(&mut frame.head,frame.head_flags);
    frame.view_flags = flags(frame.view_flags,
        frame.view_flags & ORIENTATION != 0 && frame.views.iter().all(|v|orientation_valid(&v.pose)),
        frame.view_flags & POSITION != 0 && frame.views.iter().all(|v|position_valid(&v.pose)));
    for eye in &mut frame.views { apply(&mut eye.pose,frame.view_flags); }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Tracking {
        serde_json::from_value(serde_json::json!({"frame_id":1,"time":1,"period":1,"should_render":true,"visible":true,"focused":true,"head_flags":15,"view_flags":15,
            "head":{"position":[1.,2.,3.],"orientation":[0.,0.,0.,1.]},
            "views":[{"pose":{"position":[0.97,2.,3.],"orientation":[0.,0.,0.,1.]},"fov":{"left":-0.7,"right":0.7,"up":0.7,"down":-0.7}},
                     {"pose":{"position":[1.03,2.,3.],"orientation":[0.,0.,0.,1.]},"fov":{"left":-0.7,"right":0.7,"up":0.7,"down":-0.7}}]})).unwrap()
    }
    #[test]
    fn meaningful_orientation_survives_unspecified_position() {
        let mut f=frame(); f.head_flags=5;f.head.position=[f32::NAN;3];
        f.head.orientation=[0.,std::f32::consts::FRAC_1_SQRT_2,0.,std::f32::consts::FRAC_1_SQRT_2];let q=f.head.orientation;
        sanitize_poses(&mut f);assert_eq!(f.head_flags,5);assert_eq!(f.head.orientation,q);assert_eq!(f.head.position,[0.;3]);
    }
    #[test]
    fn meaningful_position_survives_unspecified_orientation() {
        let mut f=frame();f.head_flags=10;f.head.orientation=[f32::NAN;4];
        sanitize_poses(&mut f);assert_eq!(f.head_flags,10);assert_eq!(f.head.position,[1.,2.,3.]);assert_eq!(f.head.orientation,[0.,0.,0.,1.]);
    }
    #[test]
    fn invalid_flagged_component_only_clears_its_valid_and_tracked_bits() {
        let mut f=frame();f.head.position=[f32::NAN;3];sanitize_poses(&mut f);assert_eq!(f.head_flags,5);
        let mut f=frame();f.head.orientation=[0.;4];sanitize_poses(&mut f);assert_eq!(f.head_flags,10);assert_eq!(f.head.position,[1.,2.,3.]);
    }
    #[test]
    fn common_view_flags_require_the_component_in_both_eyes() {
        let mut f=frame();f.views[0].pose.position=[f32::NAN;3];f.views[1].pose.orientation=[0.;4];
        sanitize_poses(&mut f);assert_eq!(f.view_flags,0);assert_eq!(f.head_flags,15);
        for eye in &f.views {assert_eq!(eye.pose.position,[0.;3]);assert_eq!(eye.pose.orientation,[0.,0.,0.,1.]);}
    }
    #[test]
    fn untracked_valid_components_are_preserved_without_inventing_tracking() {
        for mask in [3,7,15] {let mut f=frame();f.head_flags=mask;f.view_flags=mask;sanitize_poses(&mut f);assert_eq!(f.head_flags,mask);assert_eq!(f.view_flags,mask);assert_eq!(f.head.position,[1.,2.,3.]);}
    }
    #[test]
    fn total_loss_remains_unavailable_but_transport_is_numeric() {
        let mut f=frame();f.head_flags=0;f.view_flags=0;f.head.position=[f32::NAN;3];f.head.orientation=[f32::NAN;4];
        for eye in &mut f.views {eye.pose.position=[f32::NAN;3];eye.pose.orientation=[f32::NAN;4];}
        sanitize_poses(&mut f);assert_eq!(f.head_flags,0);assert_eq!(f.view_flags,0);
        assert!(f.head.valid());assert!(f.views.iter().all(|v|v.pose.valid()));
        assert!(serde_json::from_str::<Tracking>(&serde_json::to_string(&f).unwrap()).is_ok());
    }
}
