//! PipeWire/WirePlumber audio control. All subprocess I/O stays on a worker thread.
use serde::Serialize;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Output,
    Input,
}
impl Target {
    fn alias(self) -> &'static str {
        match self {
            Self::Output => "@DEFAULT_AUDIO_SINK@",
            Self::Input => "@DEFAULT_AUDIO_SOURCE@",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Device {
    pub id: u32,
    pub serial: u64,
    pub name: String,
    pub label: String,
    pub selected: bool,
    pub target: Target,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Level {
    pub volume: f32,
    pub muted: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub output: Option<Level>,
    pub input: Option<Level>,
    pub error: String,
}
#[derive(Clone, Copy, Debug)]
pub enum Request {
    Open(bool),
    Volume(Target, f32),
    Step(Target, i8),
    Mute(Target),
    Default {
        target: Target,
        id: u32,
        serial: u64,
    },
}
pub struct Audio {
    state: Arc<Mutex<Snapshot>>,
    send: mpsc::SyncSender<Request>,
    volumes: Arc<Mutex<[Option<f32>; 2]>>,
}
impl Audio {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let (send, receive) = mpsc::sync_channel(32);
        let shared = state.clone();
        let volumes = Arc::new(Mutex::new([None, None]));
        let pending = volumes.clone();
        std::thread::spawn(move || worker(shared, receive, pending));
        Self {
            state,
            send,
            volumes,
        }
    }
    pub fn state(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
    pub fn request(&self, request: Request) {
        if let Request::Volume(target, volume) = request {
            self.volumes.lock().unwrap()[usize::from(target == Target::Input)] = Some(volume);
        }
        let _ = self.send.try_send(request);
    }
}
impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}
fn run(program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("Audio controls require {program}. Install PipeWire tools and WirePlumber.")
            } else {
                format!("Cannot start {program}: {e}")
            }
        })?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut data = vec![];
        stdout
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map(|_| data)
    });
    let stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut data = vec![];
        let _ = stderr.take(65537).read_to_end(&mut data);
        String::from_utf8_lossy(&data).to_ascii_lowercase()
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < Duration::from_secs(2) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let data = reader
        .join()
        .map_err(|_| "Audio query failed".to_string())?
        .map_err(|e| e.to_string())?;
    let errors = errors.join().unwrap_or_default();
    // RTKit and D-Bus library warnings can accompany a successful wpctl call.
    // wpctl also reports some real CLI errors with exit status zero.
    let reported_error = command_error(&errors);
    if data.len() > 8 * 1024 * 1024 {
        return Err("PipeWire returned too much audio data".into());
    }
    match status {
        Some(s) if s.success() && !reported_error => Ok(data),
        Some(_) => Err(format!(
            "{program} could not reach the audio device. Check PipeWire and WirePlumber."
        )),
        None => Err(format!(
            "{program} timed out. Check PipeWire and WirePlumber."
        )),
    }
}
fn command_error(stderr: &str) -> bool {
    stderr.lines().map(str::trim).any(|line| {
        line.starts_with("error:")
            || line.starts_with("translate id error:")
            || line.starts_with("failed to ")
    })
}
fn property_u64(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
fn metadata_name(value: &serde_json::Value) -> Option<String> {
    value
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::to_owned)
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(value.as_str()?)
                .ok()?
                .get("name")?
                .as_str()
                .map(str::to_owned)
        })
}
pub fn parse_devices(bytes: &[u8]) -> Result<Snapshot, String> {
    let nodes: Vec<serde_json::Value> = serde_json::from_slice(bytes)
        .map_err(|_| "PipeWire returned invalid device information".to_string())?;
    let mut output_name = None;
    let mut input_name = None;
    for node in &nodes {
        if node["type"] != "PipeWire:Interface:Metadata"
            || node["props"]["metadata.name"] != "default"
        {
            continue;
        }
        for item in node["metadata"].as_array().into_iter().flatten() {
            if item["subject"].as_u64() != Some(0) {
                continue;
            }
            match item["key"].as_str() {
                Some("default.audio.sink") => output_name = metadata_name(&item["value"]),
                Some("default.audio.source") => input_name = metadata_name(&item["value"]),
                _ => (),
            }
        }
    }
    let mut snapshot = Snapshot::default();
    for node in &nodes {
        if node["type"] != "PipeWire:Interface:Node" {
            continue;
        }
        let props = &node["info"]["props"];
        let target = match props["media.class"].as_str() {
            Some("Audio/Sink") => Target::Output,
            Some("Audio/Source" | "Audio/Source/Virtual") => Target::Input,
            _ => continue,
        };
        let Some(id) = property_u64(&node["id"]).and_then(|n| u32::try_from(n).ok()) else {
            continue;
        };
        let Some(serial) = property_u64(&props["object.serial"]) else {
            continue;
        };
        let Some(name) = props["node.name"].as_str() else {
            continue;
        };
        let label = ["node.description", "node.nick", "node.name"]
            .into_iter()
            .find_map(|key| props[key].as_str().filter(|s| !s.is_empty()))
            .unwrap_or(name)
            .chars()
            .take(160)
            .collect();
        let selected = Some(name)
            == match target {
                Target::Output => output_name.as_deref(),
                Target::Input => input_name.as_deref(),
            };
        let device = Device {
            id,
            serial,
            name: name.into(),
            label,
            selected,
            target,
        };
        match target {
            Target::Output => snapshot.outputs.push(device),
            Target::Input => snapshot.inputs.push(device),
        }
    }
    snapshot
        .outputs
        .sort_by(|a, b| a.label.cmp(&b.label).then(a.id.cmp(&b.id)));
    snapshot
        .inputs
        .sort_by(|a, b| a.label.cmp(&b.label).then(a.id.cmp(&b.id)));
    Ok(snapshot)
}
pub fn parse_level(bytes: &[u8]) -> Result<Level, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid audio volume".to_string())?;
    let number = text
        .strip_prefix("Volume:")
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v >= 0. && *v <= 100.);
    number
        .map(|volume| Level {
            volume,
            muted: text.split_whitespace().any(|s| s == "[MUTED]"),
        })
        .ok_or_else(|| "Invalid audio volume".into())
}
fn query() -> Result<Snapshot, String> {
    let mut snapshot = parse_devices(&run("pw-dump", &[])?)?;
    let mut errors = vec![];
    for target in [Target::Output, Target::Input] {
        let devices = if target == Target::Output {
            &snapshot.outputs
        } else {
            &snapshot.inputs
        };
        if devices.iter().any(|d| d.selected) {
            match run("wpctl", &["get-volume", target.alias()]).and_then(|data| parse_level(&data))
            {
                Ok(level) => {
                    if target == Target::Output {
                        snapshot.output = Some(level)
                    } else {
                        snapshot.input = Some(level)
                    }
                }
                Err(error) => errors.push(error),
            }
        }
    }
    snapshot.error = errors.into_iter().next().unwrap_or_default();
    if snapshot.error.is_empty() && snapshot.output.is_none() && snapshot.input.is_none() {
        snapshot.error = "No default audio device. Select an available device below.".into();
    }
    Ok(snapshot)
}
fn apply(request: Request) -> Result<(), String> {
    match request {
        Request::Open(_) => (),
        Request::Volume(target, volume) => {
            if !volume.is_finite() {
                return Err("Invalid audio volume".into());
            }
            run(
                "wpctl",
                &[
                    "set-volume",
                    "--limit",
                    "1.0",
                    target.alias(),
                    &format!("{:.3}", volume.clamp(0., 1.)),
                ],
            )?;
        }
        Request::Step(target, step) => {
            if step != 0 {
                run(
                    "wpctl",
                    &[
                        "set-volume",
                        "--limit",
                        "1.0",
                        target.alias(),
                        &format!(
                            "{}%{}",
                            step.unsigned_abs().min(100),
                            if step > 0 { "+" } else { "-" }
                        ),
                    ],
                )?;
            }
        }
        Request::Mute(target) => {
            run("wpctl", &["set-mute", target.alias(), "toggle"])?;
        }
        Request::Default { target, id, serial } => {
            let current = parse_devices(&run("pw-dump", &[])?)?;
            if !current
                .outputs
                .iter()
                .chain(&current.inputs)
                .any(|d| d.id == id && d.serial == serial && d.target == target)
            {
                return Err("That audio device was disconnected. Select another device.".into());
            }
            run("wpctl", &["set-default", &id.to_string()])?;
        }
    }
    Ok(())
}
fn flush_volumes(volumes: &Mutex<[Option<f32>; 2]>) -> Result<(), String> {
    let values = std::mem::replace(&mut *volumes.lock().unwrap(), [None, None]);
    for (target, value) in [Target::Output, Target::Input].into_iter().zip(values) {
        if let Some(value) = value {
            apply(Request::Volume(target, value))?;
        }
    }
    Ok(())
}
fn worker(
    state: Arc<Mutex<Snapshot>>,
    receive: mpsc::Receiver<Request>,
    volumes: Arc<Mutex<[Option<f32>; 2]>>,
) {
    let mut open = false;
    let mut failure: Option<(Instant, String)> = None;
    loop {
        if let Err(error) = flush_volumes(&volumes) {
            failure = Some((Instant::now(), error));
        }
        let mut next = query().unwrap_or_else(|error| Snapshot {
            error,
            ..Default::default()
        });
        if let Some((time, error)) = &failure {
            if time.elapsed() < Duration::from_secs(4) {
                next.error = error.clone();
            } else {
                failure = None;
            }
        }
        {
            let mut old = state.lock().unwrap();
            next.revision = old.revision;
            if next != *old {
                next.revision = old.revision.wrapping_add(1);
                *old = next;
            }
        }
        match receive.recv_timeout(if open {
            Duration::from_millis(350)
        } else {
            Duration::from_secs(2)
        }) {
            Ok(request) => {
                if let Request::Open(value) = request {
                    open = value;
                }
                if !matches!(request, Request::Volume(..)) {
                    failure = flush_volumes(&volumes)
                        .and_then(|_| apply(request))
                        .err()
                        .map(|e| (Instant::now(), e));
                }
                for _ in 0..7 {
                    let Ok(request) = receive.try_recv() else {
                        break;
                    };
                    if let Request::Open(value) = request {
                        open = value;
                    }
                    if !matches!(request, Request::Volume(..)) {
                        failure = flush_volumes(&volumes)
                            .and_then(|_| apply(request))
                            .err()
                            .map(|e| (Instant::now(), e));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_warnings_do_not_disable_successful_audio_queries() {
        assert!(!command_error(
            "w 08:42:47 mod.rt: rtkit error: org.freedesktop.dbus.error.serviceunknown"
        ));
        assert!(command_error("translate id error: '-1' is not a valid id"));
        assert!(command_error("error: failed to connect"));
    }
    #[test]
    fn default_metadata_and_device_classes_control_selection() {
        let data = serde_json::json!([
            {"id":1,"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},"metadata":[{"subject":0,"key":"default.audio.sink","value":{"name":"speakers"}},{"subject":0,"key":"default.audio.source","value":"{\"name\":\"mic\"}"},{"subject":0,"key":"default.configured.audio.sink","value":{"name":"gone"}}]},
            {"id":5,"type":"PipeWire:Interface:Node","info":{"props":{"media.class":"Audio/Sink","node.name":"speakers","node.description":"Speakers","object.serial":105}}},
            {"id":6,"type":"PipeWire:Interface:Node","info":{"props":{"media.class":"Audio/Sink","node.name":"headset","node.nick":"Headset","object.serial":"106"}}},
            {"id":7,"type":"PipeWire:Interface:Node","info":{"props":{"media.class":"Audio/Source","node.name":"mic","object.serial":107}}},
            {"id":8,"type":"PipeWire:Interface:Node","info":{"props":{"media.class":"Stream/Output/Audio","node.name":"player","object.serial":108}}}
        ]);
        let s = parse_devices(&serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(s.outputs.len(), 2);
        assert_eq!(s.inputs.len(), 1);
        assert!(s.outputs.iter().find(|d| d.id == 5).unwrap().selected);
        assert!(s.inputs[0].selected);
        assert_eq!(s.outputs[0].serial, 106);
        let mut changed = data;
        changed[1]["info"]["props"]["object.serial"] = serde_json::json!(205);
        let s = parse_devices(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert_eq!(s.outputs[1].serial, 205);
    }
    #[test]
    fn levels_preserve_mute_and_amplification_and_reject_invalid_data() {
        assert_eq!(
            parse_level(b"Volume: 0.42 [MUTED]\n").unwrap(),
            Level {
                volume: 0.42,
                muted: true
            }
        );
        assert_eq!(parse_level(b"Volume: 1.50\n").unwrap().volume, 1.5);
        for data in [
            b"Volume: NaN".as_slice(),
            b"Volume: -1",
            b"unavailable",
            b"Volume: inf",
        ] {
            assert!(parse_level(data).is_err());
        }
        assert!(parse_devices(b"not JSON").is_err());
    }
}
