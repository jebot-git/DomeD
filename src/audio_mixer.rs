//! General mixer over the local PipeWire PulseAudio compatibility service.
//! Every mutation revalidates the server object serial, including its routing target.
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Output,
    Input,
    Playback,
    Recording,
    Card,
}
impl Kind {
    fn command(self) -> &'static str {
        match self {
            Self::Output => "sink",
            Self::Input => "source",
            Self::Playback => "sink-input",
            Self::Recording => "source-output",
            Self::Card => "card",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Output => "Output",
            Self::Input => "Input",
            Self::Playback => "Playback",
            Self::Recording => "Recording",
            Self::Card => "Device profile",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub serial: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Channel {
    pub name: String,
    pub volume: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Item {
    pub kind: Kind,
    pub id: u32,
    pub serial: u64,
    pub label: String,
    pub muted: bool,
    pub channels: Vec<Channel>,
    pub choices: Vec<Choice>,
    pub current: String,
}
impl Item {
    pub fn volume(&self, channel: Option<usize>) -> Option<f32> {
        channel
            .and_then(|n| self.channels.get(n).map(|v| v.volume))
            .or_else(|| self.channels.iter().map(|v| v.volume).reduce(f32::max))
    }
}
#[derive(Clone, Debug)]
pub enum Action {
    Volume { channel: Option<usize>, value: f32 },
    Step { channel: Option<usize>, delta: i8 },
    Mute,
    Select(Choice),
}
#[derive(Clone, Debug)]
pub struct Request {
    pub kind: Kind,
    pub id: u32,
    pub serial: u64,
    pub action: Action,
}
fn numeric(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}
fn string(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|n| n.to_string()))
        .unwrap_or_default()
}
fn label(v: &Value, fallback: &str) -> String {
    [
        v["properties"]["application.name"].as_str(),
        v["properties"]["node.description"].as_str(),
        v["description"].as_str(),
        v["properties"]["device.description"].as_str(),
        v["name"].as_str(),
    ]
    .into_iter()
    .flatten()
    .find(|s| !s.is_empty())
    .unwrap_or(fallback)
    .chars()
    .take(160)
    .map(|c| if c.is_control() { ' ' } else { c })
    .collect()
}
pub fn parse(bytes: &[u8]) -> Result<Vec<Item>, String> {
    let data: Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid mixer information".to_owned())?;
    if !data.is_object() {
        return Err("Invalid mixer information".into());
    }
    let mut items = Vec::new();
    for (key, kind) in [
        ("sinks", Kind::Output),
        ("sources", Kind::Input),
        ("sink_inputs", Kind::Playback),
        ("source_outputs", Kind::Recording),
        ("cards", Kind::Card),
    ] {
        for node in data[key].as_array().into_iter().flatten().take(256) {
            let (Some(id), Some(serial)) = (
                numeric(&node["index"]).and_then(|n| u32::try_from(n).ok()),
                numeric(&node["properties"]["object.serial"]),
            ) else {
                continue;
            };
            let mut channels = Vec::new();
            let order = node["channel_map"]
                .as_str()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            if let Some(volumes) = node["volume"].as_object() {
                let names = if order.is_empty() {
                    volumes.keys().map(String::as_str).collect()
                } else {
                    order
                };
                for name in names.into_iter().take(64) {
                    if let Some(value) = numeric(&node["volume"][name]["value"]) {
                        channels.push(Channel {
                            name: name.to_owned(),
                            volume: (value as f32 / 65536.).min(100.),
                        });
                    }
                }
            }
            let mut item = Item {
                kind,
                id,
                serial,
                label: label(node, kind.label()),
                muted: node["mute"].as_bool().unwrap_or(false),
                channels,
                choices: Vec::new(),
                current: String::new(),
            };
            match kind {
                Kind::Playback | Kind::Recording => {
                    let (field, devices) = if kind == Kind::Playback {
                        ("sink", "sinks")
                    } else {
                        ("source", "sources")
                    };
                    item.current = string(&node[field]);
                    for device in data[devices].as_array().into_iter().flatten().take(256) {
                        if let (Some(index), Some(serial)) = (
                            numeric(&device["index"]),
                            numeric(&device["properties"]["object.serial"]),
                        ) {
                            item.choices.push(Choice {
                                value: index.to_string(),
                                label: label(device, "Device"),
                                serial: Some(serial),
                            });
                        }
                    }
                }
                Kind::Card => {
                    item.current = string(&node["active_profile"]);
                    if item.current.is_empty() {
                        item.current = string(&node["active_profile"]["name"]);
                    }
                    if let Some(profiles) = node["profiles"].as_object() {
                        for (name, profile) in profiles.iter().take(256) {
                            if profile["available"] != "no" && profile["available"] != false {
                                item.choices.push(Choice {
                                    value: name.clone(),
                                    label: profile["description"]
                                        .as_str()
                                        .unwrap_or(name)
                                        .chars()
                                        .take(160)
                                        .collect(),
                                    serial: None,
                                });
                            }
                        }
                    }
                }
                Kind::Output | Kind::Input => {
                    item.current = string(&node["active_port"]);
                    if item.current.is_empty() {
                        item.current = string(&node["active_port"]["name"]);
                    }
                    if let Some(ports) = node["ports"].as_array() {
                        for port in ports.iter().take(256) {
                            if port["availability"] != "not available" && port["available"] != "no"
                            {
                                if let Some(name) = port["name"].as_str() {
                                    item.choices.push(Choice {
                                        value: name.to_owned(),
                                        label: port["description"]
                                            .as_str()
                                            .unwrap_or(name)
                                            .chars()
                                            .take(160)
                                            .collect(),
                                        serial: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            items.push(item);
        }
    }
    Ok(items)
}
pub fn query() -> Result<Vec<Item>, String> {
    parse(&crate::audio::run("pactl", &["--format=json", "list"])?)
}
pub fn arguments(request: &Request, items: &[Item]) -> Result<Vec<String>, String> {
    let item = items
        .iter()
        .find(|item| {
            item.kind == request.kind && item.id == request.id && item.serial == request.serial
        })
        .ok_or("That mixer stream or device was disconnected")?;
    let id = item.id.to_string();
    let base = item.kind.command();
    match &request.action {
        Action::Mute if !item.channels.is_empty() => {
            Ok(vec![format!("set-{base}-mute"), id, "toggle".into()])
        }
        Action::Volume { channel, value } => volume_arguments(item, *channel, *value),
        Action::Step { channel, delta } => volume_arguments(
            item,
            *channel,
            item.volume(*channel).ok_or("No volume control")? + *delta as f32 / 100.,
        ),
        Action::Select(selection) => {
            let choice = item
                .choices
                .iter()
                .find(|c| c.value == selection.value && c.serial == selection.serial)
                .ok_or("That mixer choice is no longer available")?;
            if let Some(serial) = choice.serial {
                let kind = if item.kind == Kind::Playback {
                    Kind::Output
                } else {
                    Kind::Input
                };
                if !items.iter().any(|i| {
                    i.kind == kind && i.id.to_string() == choice.value && i.serial == serial
                }) {
                    return Err("The destination audio device was disconnected".into());
                }
            }
            let command = match item.kind {
                Kind::Playback => "move-sink-input",
                Kind::Recording => "move-source-output",
                Kind::Card => "set-card-profile",
                Kind::Output => "set-sink-port",
                Kind::Input => "set-source-port",
            };
            Ok(vec![command.into(), id, choice.value.clone()])
        }
        _ => Err("That mixer item has no volume control".into()),
    }
}
fn volume_arguments(
    item: &Item,
    channel: Option<usize>,
    value: f32,
) -> Result<Vec<String>, String> {
    if !value.is_finite()
        || item.channels.is_empty()
        || channel.is_some_and(|n| n >= item.channels.len())
    {
        return Err("Invalid mixer channel or volume".into());
    }
    let value = value.clamp(0., 1.);
    let peak = item.volume(None).unwrap_or(0.);
    let mut args = vec![
        format!("set-{}-volume", item.kind.command()),
        item.id.to_string(),
    ];
    for (index, old) in item.channels.iter().enumerate() {
        let next = match channel {
            Some(n) if index == n => value,
            Some(_) => old.volume,
            None if peak > 0. => old.volume * value / peak,
            None => value,
        };
        args.push(format!("{:.2}%", next.clamp(0., 1.) * 100.));
    }
    Ok(args)
}
pub fn apply(request: Request) -> Result<(), String> {
    let current = query()?;
    let args = arguments(&request, &current)?;
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    crate::audio::run("pactl", &refs)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<Item> {
        parse(br#"{"sinks":[{"index":5,"name":"speakers","properties":{"object.serial":"105"},"channel_map":"front-left,front-right","volume":{"front-right":{"value":32768},"front-left":{"value":16384}}}],"sink_inputs":[{"index":7,"sink":5,"properties":{"object.serial":107,"application.name":"Player"},"channel_map":"front-left,front-right","volume":{"front-left":{"value":16384},"front-right":{"value":32768}}}],"cards":[{"index":9,"properties":{"object.serial":109},"active_profile":"stereo","profiles":{"stereo":{"description":"Stereo","available":"yes"},"off":{"description":"Off"}}}]}"#).unwrap()
    }
    #[test]
    fn channels_keep_server_order_and_balance() {
        let items = fixture();
        assert_eq!(items[0].channels[0].name, "front-left");
        let r = Request {
            kind: Kind::Playback,
            id: 7,
            serial: 107,
            action: Action::Volume {
                channel: None,
                value: 0.8,
            },
        };
        assert_eq!(
            arguments(&r, &items).unwrap(),
            ["set-sink-input-volume", "7", "40.00%", "80.00%"]
        );
        let r = Request {
            action: Action::Volume {
                channel: Some(0),
                value: 0.7,
            },
            ..r
        };
        assert_eq!(arguments(&r, &items).unwrap()[2..], ["70.00%", "50.00%"]);
    }
    #[test]
    fn stale_ids_and_stale_routing_targets_are_rejected() {
        let mut items = fixture();
        let r = Request {
            kind: Kind::Playback,
            id: 7,
            serial: 999,
            action: Action::Mute,
        };
        assert!(arguments(&r, &items).is_err());
        let r = Request {
            serial: 107,
            action: Action::Select(items[1].choices[0].clone()),
            ..r
        };
        assert_eq!(
            arguments(&r, &items).unwrap(),
            ["move-sink-input", "7", "5"]
        );
        items[0].serial += 1;
        assert!(arguments(&r, &items).is_err());
        // A fresh server snapshot may reuse the old numeric destination ID.
        items[1].choices[0].serial = Some(items[0].serial);
        assert!(arguments(&r, &items).is_err());
    }
    #[test]
    fn profiles_and_invalid_data() {
        let items = fixture();
        let r = Request {
            kind: Kind::Card,
            id: 9,
            serial: 109,
            action: Action::Select(Choice {
                value: "off".into(),
                label: "Off".into(),
                serial: None,
            }),
        };
        assert_eq!(
            arguments(&r, &items).unwrap(),
            ["set-card-profile", "9", "off"]
        );
        assert!(parse(b"[]").is_err());
        let r = Request {
            kind: Kind::Playback,
            id: 7,
            serial: 107,
            action: Action::Volume {
                channel: None,
                value: f32::NAN,
            },
        };
        assert!(arguments(&r, &items).is_err());
    }
    #[test]
    fn server_ports_and_boolean_profile_availability() {
        let items = parse(br#"{"sinks":[{"index":2,"properties":{"object.serial":"102"},"active_port":"analog","ports":[{"name":"analog","description":"Speakers","availability":"available"},{"name":"hdmi","description":"HDMI","availability":"not available"}]}],"cards":[{"index":3,"properties":{"object.serial":"103","device.description":"Sound card"},"active_profile":"stereo","profiles":{"stereo":{"description":"Stereo","available":true},"hdmi":{"available":false},"surround":{"available":"no"}}}]}"#).unwrap();
        assert_eq!(items[0].choices.len(), 1);
        assert_eq!(items[0].choices[0].label, "Speakers");
        assert_eq!(items[1].label, "Sound card");
        assert_eq!(items[1].choices.len(), 1);
        assert_eq!(items[1].choices[0].value, "stereo");
    }
}
