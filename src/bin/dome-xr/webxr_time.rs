//! Runtime-provided conversion; OpenXR time has no assumed system epoch.
use openxr as xr;

pub fn monotonic(instance: &xr::Instance, time: xr::Time) -> Option<i64> {
    let extension = instance.exts().khr_convert_timespec_time.as_ref()?;
    let mut converted = nix::libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let result = unsafe {
        (extension.convert_time_to_timespec_time)(instance.as_raw(), time, &mut converted)
    };
    if result != xr::sys::Result::SUCCESS {
        return None;
    }
    nanos(converted.tv_sec, converted.tv_nsec)
}

fn nanos(seconds: i64, nanoseconds: i64) -> Option<i64> {
    if seconds < 0 || !(0..1_000_000_000).contains(&nanoseconds) {
        return None;
    }
    seconds
        .checked_mul(1_000_000_000)?
        .checked_add(nanoseconds)
        .filter(|value| *value > 0)
}

/// Untracked OpenXR poses have unspecified components. Keep the transport
/// numeric without presenting those placeholders as valid tracking.
pub fn sanitize_poses(frame: &mut dome::webxr_bridge::Tracking) {
    dome::webxr_bridge::sanitize_poses(frame);
}

pub fn event_tracking<G>(
    instance: &xr::Instance,
    session: &xr::Session<G>,
    view: &xr::Space,
    stage: &xr::Space,
    display: &dome::webxr_bridge::Tracking,
    event: super::webxr_input::TimedInputs,
) -> Result<dome::webxr_bridge::Tracking, xr::sys::Result> {
    let head = view.locate(stage, event.time)?;
    let (flags, views) = session.locate_views(super::VIEW_TYPE, event.time, stage)?;
    if views.len() != 2 {
        return Err(xr::sys::Result::ERROR_RUNTIME_FAILURE);
    }
    let mut frame = display.clone();
    frame.time = event.time.as_nanos();
    frame.monotonic_time = monotonic(instance, event.time);
    frame.head = super::bridge_pose(head.pose);
    frame.head_flags = head.location_flags.into_raw();
    frame.view_flags = flags.into_raw();
    for (eye, located) in frame.views.iter_mut().zip(views) {
        eye.pose = super::bridge_pose(located.pose);
        eye.fov = super::bridge_fov(located.fov);
    }
    frame.inputs = event.inputs;
    sanitize_poses(&mut frame);
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undefined_poses_are_transportable_without_claiming_valid_tracking() {
        use dome::webxr_bridge::{Fov, Pose, Tracking, View};
        let eye = View {
            pose: Pose::default(),
            fov: Fov {
                left: -0.7,
                right: 0.7,
                up: 0.7,
                down: -0.7,
            },
        };
        let mut frame = Tracking {
            reference_space: None,
            frame_id: 1,
            time: 1,
            monotonic_time: Some(10),
            period: 10,
            should_render: true,
            visible: true,
            focused: true,
            head_flags: 15,
            view_flags: 15,
            head: Pose::default(),
            views: [eye.clone(), eye],
            inputs: vec![],
        };
        sanitize_poses(&mut frame);
        assert_eq!(frame.head_flags, 15);
        assert_eq!(frame.view_flags, 15);
        frame.head.orientation = [0.; 4];
        frame.views[0].pose.position[0] = f32::NAN;
        sanitize_poses(&mut frame);
        // Invalid orientation leaves meaningful tracked position intact;
        // an invalid eye position leaves meaningful eye orientation intact.
        assert_eq!(frame.head_flags, 10);
        assert_eq!(frame.view_flags, 5);
        assert!(frame.head.valid());
        assert!(frame.views.iter().all(|eye| eye.pose.valid()));
        let wire = serde_json::to_value(&frame).unwrap();
        assert_eq!(wire["monotonic_time"], 10);
        assert!(wire["head"]["orientation"].is_array());
        assert!(wire["views"][0]["pose"]["position"][0].is_number());
    }
    #[test]
    fn timespec_conversion_preserves_nanoseconds_and_rejects_invalid_or_overflowed_clocks() {
        assert_eq!(nanos(42, 123_456_789), Some(42_123_456_789));
        assert_eq!(nanos(0, 1), Some(1));
        for (s, n) in [(0, 0), (-1, 5), (1, -1), (1, 1_000_000_000), (i64::MAX, 0)] {
            assert_eq!(nanos(s, n), None);
        }
    }
}
