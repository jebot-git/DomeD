//! Persistent WirePlumber Notification category, including between short sounds.
use crate::audio::{run, Level};
use serde_json::{json, Value};

const KEY: &str = "restore.stream.Output/Audio.media.role:Notification";

fn settings(nodes: &[Value]) -> Option<Value> {
    let metadata = nodes.iter().find(|n| {
        n["type"] == "PipeWire:Interface:Metadata"
            && n["props"]["metadata.name"] == "route-settings"
    })?;
    let value = metadata["metadata"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["subject"] == 0 && m["key"] == KEY)
        .map(|m| m["value"].clone())
        .unwrap_or_else(
            || json!({"volume":1.,"volumes":[1.,1.],"channels":["FL","FR"],"mute":false}),
        );
    if value.is_string() {
        serde_json::from_str(value.as_str()?).ok()
    } else {
        value.is_object().then_some(value)
    }
}

pub fn level(nodes: &[Value]) -> Option<Level> {
    let value = settings(nodes)?;
    let master = value["volume"].as_f64().unwrap_or(1.);
    let peak = value["volumes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .reduce(f64::max)
        .unwrap_or(1.);
    let volume = (master * peak).max(0.).cbrt() as f32;
    volume.is_finite().then_some(Level {
        volume,
        muted: value["mute"].as_bool().unwrap_or(false),
    })
}

fn update(mut value: Value, volume: Option<f32>, muted: Option<bool>) -> Result<Value, String> {
    if let Some(volume) = volume {
        if !volume.is_finite() {
            return Err("Invalid notification volume".into());
        }
        let channels = value["volumes"]
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![json!(1.), json!(1.)]);
        let peak = channels
            .iter()
            .filter_map(Value::as_f64)
            .reduce(f64::max)
            .unwrap_or(1.);
        let amplitude = f64::from(volume.clamp(0., 1.)).powi(3);
        value["volume"] = json!(1.);
        value["volumes"] = Value::Array(
            channels
                .iter()
                .map(|c| {
                    json!(if peak > 0. {
                        c.as_f64().unwrap_or(0.).max(0.) / peak * amplitude
                    } else {
                        amplitude
                    })
                })
                .collect(),
        );
    }
    if let Some(muted) = muted {
        value["mute"] = json!(muted);
    }
    Ok(value)
}

pub fn apply(volume: Option<f32>, muted: Option<bool>) -> Result<(), String> {
    let nodes: Vec<Value> = serde_json::from_slice(&run("pw-dump", &[])?)
        .map_err(|_| "Invalid PipeWire notification settings")?;
    let current =
        settings(&nodes).ok_or("Notification controls require WirePlumber route settings")?;
    let value = update(current, volume, muted)?.to_string();
    run(
        "pw-metadata",
        &["-n", "route-settings", "0", KEY, &value, "Spa:String:JSON"],
    )?;
    // WirePlumber saves this category for future streams. Apply the same
    // change to events that are already playing; metadata alone leaves them
    // unchanged. Revalidate the server serial before addressing a node ID.
    let current: Vec<Value> = serde_json::from_slice(&run("pw-dump", &[])?)
        .map_err(|_| "Invalid PipeWire notification streams")?;
    for node in current.iter().filter(|n| {
        n["type"] == "PipeWire:Interface:Node"
            && n["info"]["props"]["media.class"] == "Stream/Output/Audio"
            && n["info"]["props"]["media.role"] == "Notification"
    }) {
        let Some(id) = node["id"].as_u64() else {
            continue;
        };
        let Some(serial) = node["info"]["props"]["object.serial"].as_u64() else {
            continue;
        };
        let fresh: Vec<Value> = serde_json::from_slice(&run("pw-dump", &[])?)
            .map_err(|_| "Invalid PipeWire notification streams")?;
        if !fresh
            .iter()
            .any(|n| n["id"] == id && n["info"]["props"]["object.serial"] == serial)
        {
            continue;
        }
        let live_result = (|| -> Result<(), String> {
            if let Some(volume) = volume {
                run(
                    "wpctl",
                    &[
                        "set-volume",
                        "--limit",
                        "1.0",
                        &id.to_string(),
                        &format!("{:.3}", volume.clamp(0., 1.)),
                    ],
                )?;
            }
            if let Some(muted) = muted {
                run(
                    "wpctl",
                    &["set-mute", &id.to_string(), if muted { "1" } else { "0" }],
                )?;
            }
            Ok(())
        })();
        if let Err(error) = live_result {
            let fresh: Vec<Value> = serde_json::from_slice(&run("pw-dump", &[])?)
                .map_err(|_| "Invalid PipeWire notification streams")?;
            // Short event streams normally disappear while controls are open.
            if fresh
                .iter()
                .any(|n| n["id"] == id && n["info"]["props"]["object.serial"] == serial)
            {
                return Err(error);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn category_exists_without_a_playing_stream_and_preserves_channel_balance() {
        let original = json!({"volume":1.,"volumes":[0.125,1.],"channels":["FL","FR"],"mute":true,"target":"keep"});
        let changed = update(original, Some(0.5), Some(false)).unwrap();
        assert_eq!(changed["volumes"], json!([0.015625, 0.125]));
        assert_eq!(changed["target"], "keep");
        let nodes = vec![
            json!({"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"route-settings"},
            "metadata":[{"subject":0,"key":KEY,"value":changed}]}),
        ];
        assert_eq!(
            level(&nodes),
            Some(Level {
                volume: 0.5,
                muted: false
            })
        );
        assert!(update(json!({}), Some(f32::NAN), None).is_err());
        assert!(level(&[]).is_none());
    }
}
