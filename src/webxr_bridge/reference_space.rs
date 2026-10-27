//! Cumulative native-origin state, applied at the runtime's scheduled change time.
//! A browser may skip display frames without losing the total origin transform.
use super::Pose;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ReferenceSpaceState {
    pub version: u32,
    pub revision: u64,
    /// Increments when the runtime cannot relate the new origin to the old one.
    pub epoch: u64,
    /// Current native origin expressed in this epoch's initial native coordinates.
    pub origin: Pose,
}
impl Default for ReferenceSpaceState {
    fn default() -> Self {
        Self { version: 1, revision: 0, epoch: 0, origin: Pose::default() }
    }
}
impl ReferenceSpaceState {
    pub fn valid(&self) -> bool {
        self.version == 1 && self.epoch <= self.revision && self.origin.valid()
    }
}
#[derive(Default)]
pub struct ReferenceSpaceTimeline {
    state: ReferenceSpaceState,
    pending: Vec<(i64, Option<Pose>)>,
}
impl ReferenceSpaceTimeline {
    pub fn schedule(&mut self, time: i64, delta: Option<Pose>) -> Result<(), &'static str> {
        if self.pending.len() >= 64 {
            return Err("Too many pending XR reference-space changes");
        }
        self.pending.push((time, delta.filter(Pose::valid)));
        self.pending.sort_by_key(|change| change.0);
        Ok(())
    }
    pub fn at(&mut self, time: i64) -> ReferenceSpaceState {
        let count = self.pending.partition_point(|change| change.0 <= time);
        for (_, delta) in self.pending.drain(..count) {
            self.state.revision = self.state.revision.checked_add(1).expect("XR origin revision exhausted");
            let previous = self.state.origin;
            let composed = delta.map(|delta| {
                let rotation = Quat::from_array(previous.orientation);
                Pose {
                    position: (Vec3::from_array(previous.position) + rotation * Vec3::from_array(delta.position)).to_array(),
                    orientation: (rotation * Quat::from_array(delta.orientation)).normalize().to_array(),
                }
            }).filter(Pose::valid);
            if let Some(origin) = composed {
                self.state.origin = origin;
            } else {
                self.state.epoch = self.state.epoch.checked_add(1).expect("XR origin epoch exhausted");
                self.state.origin = Pose::default();
            }
        }
        self.state
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_are_scheduled_and_all_due_changes_compose_in_order() {
        let mut timeline = ReferenceSpaceTimeline::default();
        timeline.schedule(20, Some(Pose { position: [1., 0., 0.], ..Pose::default() })).unwrap();
        timeline.schedule(10, Some(Pose { orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array(), ..Pose::default() })).unwrap();
        assert_eq!(timeline.at(9).revision, 0);
        assert_eq!(timeline.at(10).revision, 1);
        let final_state = timeline.at(25);
        assert_eq!(final_state.revision, 2);
        assert!(final_state.origin.position[0].abs() < 1e-5);
        assert!((final_state.origin.position[2] + 1.).abs() < 1e-5);
        assert_eq!(timeline.at(25).revision, 2);
        assert!(final_state.valid());
    }
    #[test]
    fn unknown_delta_and_invalid_runtime_pose_start_a_new_epoch() {
        let mut timeline = ReferenceSpaceTimeline::default();
        timeline.schedule(10, None).unwrap();
        timeline.schedule(20, Some(Pose { orientation: [0.;4], ..Pose::default() })).unwrap();
        let state = timeline.at(20);
        assert_eq!((state.revision, state.epoch), (2,2));
        assert!(state.valid());
    }
    #[test]
    fn pending_overflow_is_rejected_without_publishing_partial_composition() {
        let mut timeline = ReferenceSpaceTimeline::default();
        for time in 0..64 { timeline.schedule(time, Some(Pose::default())).unwrap(); }
        assert!(timeline.schedule(65, Some(Pose::default())).is_err());
        let state = timeline.at(100);
        assert_eq!(state.revision, 64);
        assert!(state.valid() && timeline.pending.is_empty());
    }
}
