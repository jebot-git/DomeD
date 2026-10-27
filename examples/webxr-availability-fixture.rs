//! Private test host for the real DomeD WebXR bridge, without a display session.
use dome::webxr_bridge::{Capabilities, Fov, Pose, Server, Tracking, View};
use std::io::{self, BufRead, Write};
fn tracking(frame_id: u64) -> Tracking {
    let view = View {
        pose: Pose::default(),
        fov: Fov {
            left: -0.7,
            right: 0.7,
            up: 0.7,
            down: -0.7,
        },
    };
    Tracking {
        frame_id,
        time: frame_id as i64 * 11_111_111,
        monotonic_time: None,
        period: 11_111_111,
        should_render: true,
        visible: true,
        focused: true,
        head_flags: 15,
        view_flags: 15,
        head: Pose::default(),
        views: [view.clone(), view],
        inputs: vec![],
        reference_space: None,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Fixture socket required")?;
    let server = Server::bind(
        std::path::Path::new(&path),
        Capabilities {
            name: "DomeD availability fixture".into(),
            width: 128,
            height: 96,
            view_sizes: [[64, 96]; 2],
            max_width: 4096,
            max_height: 4096,
            blend_modes: vec!["opaque".into()],
            stage: false,
            period: 11_111_111,
            input_stream_version: 0,
        },
    )?;
    let mut frame_id = 0;
    println!("{}", serde_json::json!({"ready":true}));
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        let command: serde_json::Value = serde_json::from_str(&line?)?;
        match command["op"].as_str() {
            Some("active" | "publish") => {
                frame_id += 1;
                server.publish(tracking(frame_id));
            }
            Some("suspend") => server.suspend(),
            Some("quit") => break,
            _ => return Err("Unknown fixture operation".into()),
        }
        println!(
            "{}",
            serde_json::json!({"ok":true,"token":command["token"]})
        );
        io::stdout().flush()?;
    }
    Ok(())
}
