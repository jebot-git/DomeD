//! Versioned tracked-input extension to the browser projection protocol.
//! The renderer must explicitly advertise this only after attaching real actions.
use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::{Pose, Tracking};

pub const INPUT_STREAM_VERSION: u32 = 1;
const MAX_HISTORY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputHandedness {
    None,
    Left,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackedInput {
    pub id: u32,
    pub handedness: InputHandedness,
    pub profiles: Vec<String>,
    pub supports_grip: bool,
    pub aim: Option<Pose>,
    pub grip: Option<Pose>,
    pub select: bool,
    pub squeeze: bool,
    #[serde(default = "active_by_default")]
    pub select_active: bool,
    #[serde(default = "active_by_default")]
    pub squeeze_active: bool,
    pub buttons: Vec<f32>,
    #[serde(default)]
    pub touches: Vec<bool>,
    pub axes: Vec<f32>,
}

fn active_by_default() -> bool { true }

impl TrackedInput {
    fn valid(&self) -> bool {
        self.id != 0
            && !self.profiles.is_empty()
            && self.profiles.len() <= 4
            && self.profiles.iter().all(|profile| {
                !profile.is_empty()
                    && profile.len() <= 96
                    && profile.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            })
            && self.aim.as_ref().is_none_or(Pose::valid)
            && self.grip.as_ref().is_none_or(Pose::valid)
            && (self.supports_grip || self.grip.is_none())
            && self.buttons.len() <= 8
            && (self.touches.is_empty() || self.touches.len() == self.buttons.len())
            && self.axes.len() <= 4
            && self.buttons.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self.axes.iter().all(|v| v.is_finite() && (-1.0..=1.0).contains(v))
    }

    fn same_actions(&self, other: &Self) -> bool {
        self.id == other.id
            && self.handedness == other.handedness
            && self.profiles == other.profiles
            && self.supports_grip == other.supports_grip
            && self.buttons.len() == other.buttons.len()
            && self.touches.len() == other.touches.len()
            && self.axes.len() == other.axes.len()
            && self.aim.is_some() == other.aim.is_some()
            && self.grip.is_some() == other.grip.is_some()
            && self.select_active == other.select_active
            && self.squeeze_active == other.squeeze_active
            && self.select == other.select
            && self.squeeze == other.squeeze
    }
}

pub(super) fn valid_inputs(inputs: &[TrackedInput]) -> bool {
    inputs.len() <= 2
        && inputs.iter().all(TrackedInput::valid)
        && (inputs.len() != 2 || inputs[0].id != inputs[1].id)
}

fn same_actions(a: &Tracking, b: &Tracking) -> bool {
    a.visible == b.visible
        && a.focused == b.focused
        && (a.head_flags & 3) == (b.head_flags & 3)
        && (a.view_flags & 3) == (b.view_flags & 3)
        && a.inputs.len() == b.inputs.len()
        && a.inputs.iter().all(|input| {
            b.inputs.iter().find(|other| input.id == other.id)
                .is_some_and(|other| input.same_actions(other))
        })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputUpdate {
    pub sequence: u64,
    /// Capture poses and time with the transition, rather than using the later
    /// animation frame after the browser has caught up with the headset.
    pub frame: Tracking,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputBatch {
    pub version: u32,
    pub through: u64,
    /// Cancel outstanding gestures and establish the outer frame as a baseline.
    /// Never turn incomplete history into a synthetic selection.
    pub reset: bool,
    pub updates: Vec<InputUpdate>,
}

pub(super) struct InputQueue {
    sequence: u64,
    baseline: Option<Tracking>,
    updates: VecDeque<InputUpdate>,
}

impl InputQueue {
    pub fn new(baseline: Option<&Tracking>) -> Self {
        Self { sequence: 0, baseline: baseline.cloned(), updates: VecDeque::new() }
    }

    pub fn observe(&mut self, frame: &Tracking) {
        if self.baseline.as_ref().is_none_or(|last| !same_actions(last, frame)) {
            self.sequence = self.sequence.checked_add(1).expect("WebXR input sequence exhausted");
            self.updates.push_back(InputUpdate { sequence: self.sequence, frame: frame.clone() });
            while self.updates.len() > MAX_HISTORY { self.updates.pop_front(); }
        }
        self.baseline = Some(frame.clone());
    }

    pub fn batch(&self, after: u64) -> Result<InputBatch, String> {
        if after > self.sequence { return Err("Input cursor is ahead of this session".into()); }
        let reset = self.updates.front().is_some_and(|first| after < first.sequence - 1);
        Ok(InputBatch {
            version: INPUT_STREAM_VERSION,
            through: self.sequence,
            reset,
            updates: if reset { vec![] } else {
                self.updates.iter().filter(|update| update.sequence > after).cloned().collect()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn source() -> TrackedInput {
        TrackedInput { id: 1, handedness: InputHandedness::Left,
            profiles: vec!["generic-trigger".into()], supports_grip: true,
            aim: Some(Pose::default()), grip: Some(Pose::default()),
            select: false, squeeze: false, select_active: true, squeeze_active: true, buttons: vec![0.; 4], axes: vec![0.; 4], touches: vec![false; 4] }
    }

    fn frame(id: u64, select: bool) -> Tracking {
        let mut f = super::super::tests::tracking(id);
        let mut input = source(); input.select = select; input.buttons[0] = if select { 1. } else { 0. };
        f.inputs.push(input); f
    }

    #[test]
    fn transitions_between_browser_frames_keep_their_original_poses_and_times() {
        let mut queue = InputQueue::new(Some(&frame(1, true)));
        assert!(queue.batch(0).unwrap().updates.is_empty(), "held baseline is not a new press");
        for (id, pressed) in [(2, false), (3, true), (4, false)] {
            let mut f = frame(id, pressed); f.inputs[0].aim.as_mut().unwrap().position[0] = id as f32;
            queue.observe(&f);
        }
        let batch = queue.batch(0).unwrap();
        assert!(!batch.reset); assert_eq!(batch.through, 3);
        assert_eq!(batch.updates.iter().map(|v| v.frame.inputs[0].select).collect::<Vec<_>>(), [false, true, false]);
        for update in batch.updates {
            assert_eq!(update.frame.inputs[0].aim.unwrap().position[0], update.frame.frame_id as f32);
            assert_eq!(update.frame.time, update.frame.frame_id as i64 * 13_888_888);
        }
        assert_eq!(queue.batch(1).unwrap().updates.len(), 2);
        assert!(queue.batch(3).unwrap().updates.is_empty());
        assert!(queue.batch(4).is_err());
    }

    #[test]
    fn pose_and_analog_motion_do_not_overflow_transition_history() {
        let mut queue = InputQueue::new(Some(&frame(1, false)));
        for id in 2..500 {
            let mut f = frame(id, false); f.inputs[0].axes[0] = 0.5;
            f.inputs[0].buttons[0] = 0.2; f.inputs[0].aim.as_mut().unwrap().position[0] = id as f32;
            queue.observe(&f);
        }
        assert_eq!(queue.batch(0).unwrap().through, 0);
        let mut f = frame(500, false); f.inputs[0].profiles = vec!["oculus-touch-v3".into()];
        queue.observe(&f); f.inputs.clear(); queue.observe(&f);
        assert_eq!(queue.batch(0).unwrap().updates.len(), 2);
    }

    #[test]
    fn overflow_cancels_history_and_new_session_does_not_replay_old_actions() {
        let mut queue = InputQueue::new(Some(&frame(1, false)));
        for id in 2..100 { queue.observe(&frame(id, id % 2 == 0)); }
        assert!(queue.updates.len() <= MAX_HISTORY);
        let batch = queue.batch(0).unwrap(); assert!(batch.reset); assert!(batch.updates.is_empty());
        assert!(!queue.batch(batch.through).unwrap().reset);
        let fresh = InputQueue::new(Some(&frame(100, true)));
        assert_eq!(fresh.batch(0).unwrap().through, 0);
        assert!(fresh.batch(0).unwrap().updates.is_empty());
    }

    #[test]
    fn focus_and_tracking_loss_are_retained_even_when_the_button_stays_down() {
        let mut f = frame(1, true); let mut queue = InputQueue::new(Some(&f));
        f.focused = false; queue.observe(&f);
        f.focused = true; queue.observe(&f);
        f.head_flags = 0; queue.observe(&f);
        f.inputs[0].aim = None; queue.observe(&f);
        assert_eq!(queue.batch(0).unwrap().updates.len(), 4);
    }

    #[test]
    fn malformed_or_oversized_input_samples_are_not_published() {
        let mut inputs = vec![source()]; assert!(valid_inputs(&inputs));
        inputs[0].axes[0] = f32::NAN; assert!(!valid_inputs(&inputs));
        inputs[0] = source(); inputs[0].profiles[0] = "x".repeat(97); assert!(!valid_inputs(&inputs));
        inputs[0] = source(); inputs.push(source()); assert!(!valid_inputs(&inputs));
        inputs[1].id = 2; assert!(valid_inputs(&inputs));
        inputs.push(source()); assert!(!valid_inputs(&inputs));
    }
}
