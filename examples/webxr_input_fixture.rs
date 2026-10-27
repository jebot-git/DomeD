//! Private synthetic controller feed for testing the real browser projection transport.
//! No OpenXR instance or device is opened.
use dome::webxr_bridge::{
    Capabilities, Fov, InputHandedness, Pose, Server, TrackedInput, Tracking, View,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    sync::mpsc,
    thread,
    time::Duration,
};

fn apply(frame: &mut Tracking, command: &Value) {
    if let Some(reset) = command.get("reference_reset") {
        let state = frame.reference_space.get_or_insert_with(Default::default);
        state.revision += 1;
        if reset.is_null() {
            state.epoch += 1;
            state.origin = Pose::default();
        } else {
            let delta: Pose = serde_json::from_value(reset.clone()).unwrap();
            assert!(delta.valid());
            let rotation = glam::Quat::from_array(state.origin.orientation);
            state.origin = Pose {
                position: (glam::Vec3::from_array(state.origin.position) + rotation * glam::Vec3::from_array(delta.position)).to_array(),
                orientation: (rotation * glam::Quat::from_array(delta.orientation)).normalize().to_array(),
            };
        }
    }
    if let Some(x) = command["head_x"].as_f64() {
        let delta = x as f32 - frame.head.position[0];
        frame.head.position[0] = x as f32;
        for eye in &mut frame.views { eye.pose.position[0] += delta; }
    }
    if let Some(input) = frame.inputs.first_mut() {
        if let Some(v) = command["select_active"].as_bool() {
            input.select_active = v;
        }
        if let Some(v) = command["squeeze_active"].as_bool() {
            input.squeeze_active = v;
        }
    }
    if let Some(valid) = command["head_valid"].as_bool() {
        frame.head_flags = if valid { 3 } else { 0 };
        frame.view_flags = if valid { 3 } else { 0 };
        let orientation = if valid { [0., 0., 0., 1.] } else { [0.; 4] };
        frame.head.orientation = orientation;
        for eye in &mut frame.views {
            eye.pose.orientation = orientation;
        }
    }
    if let Some(v) = command["focused"].as_bool() {
        frame.focused = v;
    }
    if let Some(v) = command["visible"].as_bool() {
        frame.visible = v;
        if !v {
            frame.focused = false;
        }
    }
    if command["connected"] == false {
        frame.inputs.clear();
    }
    if command["connected"] == true && frame.inputs.is_empty() {
        frame.inputs.push(controller());
    }
    if let Some(input) = frame.inputs.first_mut() {
        if let Some(v) = command["select"].as_bool() {
            input.select = v;
            input.buttons[0] = if v { 1. } else { 0. };
            input.touches[0] = v;
        }
        if let Some(v) = command["squeeze"].as_bool() {
            input.squeeze = v;
            input.buttons[1] = if v { 1. } else { 0. };
            input.touches[1] = v;
        }
        if let Some(v) = command["aim_valid"].as_bool() {
            input.aim = if v { controller().aim } else { None };
        }
        if let Some(v) = command["aim_x"].as_f64() {
            if let Some(aim) = input.aim.as_mut() {
                aim.position[0] = v as f32;
            }
        }
        if let Some(v) = command["profile"].as_str() {
            input.profiles = vec![v.into()];
        }
        if let Some(v) = command["touchpad"].as_bool() {
            input.touches[2] = v;
            input.axes[0] = if v { 0.5 } else { 0. };
            input.axes[1] = if v { -0.5 } else { 0. };
        }
        if let Some(v) = command["simple"].as_bool() {
            if v {
                input.buttons.truncate(1);
                input.touches.truncate(1);
                input.axes.clear();
            }
        }
        if let Some(v) = command["grip"].as_bool() {
            input.supports_grip = v;
            input.grip = if v { controller().grip } else { None };
        }
    }
}
fn controller() -> TrackedInput {
    TrackedInput {
        id: 1,
        handedness: InputHandedness::Left,
        profiles: vec![
            "oculus-touch".into(),
            "generic-trigger-squeeze-thumbstick".into(),
        ],
        supports_grip: true,
        aim: Some(Pose {
            position: [1.5, 1.4, 1.],
            ..Pose::default()
        }),
        grip: Some(Pose {
            position: [1.4, 1.3, 1.],
            ..Pose::default()
        }),
        select: false,
        squeeze: false,
        select_active: true,
        squeeze_active: true,
        buttons: vec![0.; 7],
        touches: vec![false; 7],
        axes: vec![0.; 4],
    }
}
const OPAQUE_EPOCH: i64 = 700_000_000_000_000;
fn now() -> i64 {
    let time = nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC).unwrap();
    time.tv_sec() * 1_000_000_000 + time.tv_nsec()
}
fn stamp(frame: &mut Tracking, monotonic: i64, precise: bool) {
    frame.time = OPAQUE_EPOCH + monotonic;
    frame.monotonic_time = precise.then_some(monotonic);
}
fn publish(server: &Server, frame: &mut Tracking, precise: bool) {
    frame.frame_id += 1;
    stamp(frame, now() + frame.period, precise);
    server.publish(frame.clone());
}
fn main() {
    let path = std::env::args().nth(1).expect("private socket path");
    let stream = if std::env::args().any(|v| v == "--legacy") {
        0
    } else {
        1
    };
    let precise = !std::env::args().any(|v| v == "--legacy-clock" || v == "--legacy");
    let server = Server::bind(
        std::path::Path::new(&path),
        Capabilities {
            name: "Private controller fixture".into(),
            width: 128,
            height: 96,
            view_sizes: [[64, 96]; 2],
            max_width: 4096,
            max_height: 4096,
            blend_modes: vec!["opaque".into()],
            stage: true,
            period: 11_111_111,
            input_stream_version: stream,
        },
    )
    .unwrap();
    let view = View {
        pose: Pose {
            position: [0.97, 1.6, 2.],
            ..Pose::default()
        },
        fov: Fov {
            left: -0.7,
            right: 0.7,
            up: 0.7,
            down: -0.7,
        },
    };
    let mut right = view.clone();
    right.pose.position[0] = 1.03;
    let mut frame = Tracking {
        reference_space: std::env::args().any(|v| v == "--reference-spaces").then(Default::default),
        frame_id: 0,
        time: 0,
        monotonic_time: None,
        period: 11_111_111,
        should_render: true,
        visible: true,
        focused: true,
        head_flags: 3,
        view_flags: 3,
        head: Pose {
            position: [1., 1.6, 2.],
            ..Pose::default()
        },
        views: [view, right],
        inputs: vec![controller()],
    };
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    publish(&server, &mut frame, precise);
    println!("{}", json!({"ready":true,"input_stream_version":stream}));
    io::stdout().flush().unwrap();
    loop {
        for line in rx.try_iter() {
            let value: Value = serde_json::from_str(&line).unwrap();
            if value["quit"] == true {
                return;
            }
            if let Some(burst) = value["burst"].as_array() {
                frame.frame_id += 1;
                let observed = now();
                let mut history = Vec::new();
                for command in burst {
                    apply(&mut frame, command);
                    let mut event = frame.clone();
                    let age = command["event_age_ms"].as_i64().unwrap_or(0);
                    stamp(&mut event, observed - age * 1_000_000, precise);
                    history.push(event);
                }
                let predicted = observed + frame.period;
                stamp(&mut frame, predicted, precise);
                server.publish_with_history(frame.clone(), &history);
            } else {
                apply(&mut frame, &value);
                publish(&server, &mut frame, precise);
            }
            println!(
                "{}",
                json!({"ack":value["ack"],"frame_id":frame.frame_id,"stats":server.stats()})
            );
            io::stdout().flush().unwrap();
        }
        if let Some(submission) = server.take_submission() {
            let accepted = server.composed(&submission);
            submission.complete(if accepted {
                Ok(())
            } else {
                Err("stale fixture frame".into())
            });
        }
        publish(&server, &mut frame, precise);
        thread::sleep(Duration::from_millis(5));
    }
}
