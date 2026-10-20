//! Gaze pointing with adaptive smoothing in head coordinates.
//! Independently implemented from the behavior of the local KWin VR prototype:
//! eye noise is damped while fast looks catch up, without delaying head motion.
use crate::{xr_input::Hand, Pose};
use glam::{Quat, Vec3};

#[derive(Default)]
pub struct GazePointer {
    last: Option<f64>,
    eye_since: Option<f64>,
    eye_active: bool,
    rotation: Option<Quat>,
    previous: Quat,
    position: Vec3,
    velocity: Vec3,
}
impl GazePointer {
    /// Invalid or unavailable eyes fall back to head direction immediately.
    /// Reacquisition requires 80 ms of consecutive valid eye samples.
    /// Returns a reset flag when the pointing source changes during interaction.
    pub fn sample(
        &mut self,
        time: f64,
        head: Pose,
        eyes: Option<Pose>,
        smoothing: bool,
    ) -> (Hand, bool) {
        if !time.is_finite() || head.validate().is_err() {
            *self = Self::default();
            return (Hand::default(), true);
        }
        let dt = self.last.map(|old| time - old).unwrap_or(0.);
        let gap = dt <= 0. || dt > 0.15;
        if gap {
            self.rotation = None;
            self.eye_since = None;
            self.velocity = Vec3::ZERO;
        }
        self.last = Some(time);
        let eyes = eyes.filter(|p| p.validate().is_ok());
        if eyes.is_none() {
            self.eye_since = None;
            self.rotation = None;
        }
        let active = if eyes.is_some() {
            time - *self.eye_since.get_or_insert(time) >= 0.08
        } else {
            false
        };
        let changed = active != self.eye_active;
        self.eye_active = active;
        let hq = Quat::from_array(head.orientation_xyzw).normalize();
        let hp = Vec3::from_array(head.position_m);
        // Head fallback is immediate; smoothing head-local eye samples must not
        // introduce lag into the headset's own tracked pose.
        let (position, direction) = if let Some(eye) = eyes.filter(|_| active) {
            let raw = (hq.conjugate() * Quat::from_array(eye.orientation_xyzw)).normalize();
            let local = hq.conjugate() * (Vec3::from_array(eye.position_m) - hp);
            if let Some(rotation) = self.rotation.filter(|_| !gap && smoothing) {
                let low_pass = |hz: f32| 1. - (-std::f32::consts::TAU * hz * dt as f32).exp();
                let signed_speed = (raw * -Vec3::Z - self.previous * -Vec3::Z)
                    * (180. / std::f32::consts::PI / dt as f32);
                self.velocity = self.velocity.lerp(signed_speed, low_pass(1.));
                let error = (rotation * -Vec3::Z)
                    .dot(raw * -Vec3::Z)
                    .clamp(-1., 1.)
                    .acos()
                    .to_degrees();
                let hz = (1.5 + self.velocity.length() * 0.04 + (error - 1.5).max(0.) * 12.)
                    .clamp(1.5, 40.);
                let amount = low_pass(hz);
                self.rotation = Some(rotation.slerp(raw, amount).normalize());
                self.position = self.position.lerp(local, amount);
            } else {
                self.rotation = Some(raw);
                self.position = local;
                self.velocity = Vec3::ZERO;
            }
            self.previous = raw;
            (
                hp + hq * self.position,
                hq * (self.rotation.unwrap() * -Vec3::Z),
            )
        } else {
            (hp, hq * -Vec3::Z)
        };
        (
            Hand {
                valid: true,
                position: position.to_array(),
                ray: direction.to_array(),
                ..Hand::default()
            },
            changed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn eye(yaw: f32) -> Pose {
        Pose {
            orientation_xyzw: Quat::from_rotation_y(yaw.to_radians()).to_array(),
            ..Pose::default()
        }
    }
    #[test]
    fn eye_recovery_and_loss_fall_back_without_stale_click_targets() {
        let mut p = GazePointer::default();
        assert!(!p.sample(1., Pose::default(), Some(eye(20.)), true).1);
        let (ray, changed) = p.sample(1.09, Pose::default(), Some(eye(20.)), true);
        assert!(changed && ray.ray[0] < -0.3);
        let (ray, changed) = p.sample(1.10, Pose::default(), None, true);
        assert!(changed);
        assert_eq!(ray.ray, [0., 0., -1.]);
        assert!(!p.sample(1.11, Pose::default(), Some(eye(20.)), true).1);
        // A long sample gap restarts the recovery interval.
        assert!(!p.sample(2., Pose::default(), Some(eye(20.)), true).1);
    }
    #[test]
    fn fixation_jitter_is_damped_and_head_rotation_is_immediate() {
        let mut p = GazePointer::default();
        for n in 0..100 {
            p.sample(1. + n as f64 / 90., Pose::default(), Some(eye(0.)), true);
        }
        let (ray, _) = p.sample(1. + 100. / 90., Pose::default(), Some(eye(1.)), true);
        assert!(ray.ray[0].abs() < 0.005);
        let head = eye(30.);
        let gaze = Pose {
            orientation_xyzw: head.orientation_xyzw,
            ..head
        };
        let (ray, _) = p.sample(1. + 101. / 90., head, Some(gaze), true);
        assert!(ray.ray[0] < -0.49);
        let (ray, _) = p.sample(1. + 102. / 90., head, Some(eye(50.)), false);
        assert!((ray.ray[0] + 50f32.to_radians().sin()).abs() < 0.0001);
    }
}
