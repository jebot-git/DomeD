//! Validated, persistent XR layout and room-anchor preferences.
use crate::{AnchorMode, Config, Pose};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("dome/xr.json")
}
pub fn load(path: &Path) -> Result<Option<Config>, String> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let config: Config = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    config.validate()?;
    Ok(Some(config))
}
pub fn save(path: &Path, config: &Config) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    config.validate()?;
    let parent = path.parent().ok_or("Settings path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(config)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e| e.to_string())
}
pub fn anchor_here(config: &Config, head: Pose) -> Result<Config, String> {
    head.validate()?;
    let forward = head.rotate([0., 0., -1.]);
    let mut config = config.clone();
    config.anchor_mode = AnchorMode::Room;
    config.center_m = head.position_m;
    config.yaw_deg = forward[0].atan2(-forward[2]).to_degrees();
    config.validate()?;
    Ok(config)
}
pub fn rows(c: &Config) -> Vec<(&'static str, String)> {
    vec![
        (
            "Dome anchor",
            if c.anchor_mode == AnchorMode::Room {
                "Fixed in room"
            } else {
                "Follow headset"
            }
            .into(),
        ),
        ("Upper tray height", format!("{:.0}°", c.upper_bar_height_deg)),
        ("Taskbar height", format!("{:.0}°", c.taskbar_height_deg)),
        (
            "XR notification distance",
            format!("{:.2} m", c.bar_distance_m),
        ),
        ("Dome radius", format!("{:.1} m", c.radius_m)),
        (
            "Projection height / vertical span",
            format!(
                "{:.2} m ({:.0}°)",
                c.radius_m * c.dome_vertical_span_deg.to_radians(),
                c.dome_vertical_span_deg
            ),
        ),
        ("Room anchor X", format!("{:.2} m", c.center_m[0])),
        ("Dome height / anchor Y", format!("{:.2} m", c.center_m[1])),
        ("Room anchor Z", format!("{:.2} m", c.center_m[2])),
        ("Room anchor yaw", format!("{:.0}°", c.yaw_deg)),
        ("Lane gap", format!("{:.1}°", c.lane_gap_deg)),
        (
            "Pulled-out panel distance",
            format!("{:.2} m", c.pullout_distance_m),
        ),
        ("Vertical view field", format!("{:.0}°", c.vertical_fov_deg)),
        (
            "Upper dome inward lean",
            format!("{:.0}°", c.dome_upper_tilt_deg),
        ),
    ]
}
pub fn adjust(config: &Config, row: usize, direction: f32) -> Result<Config, String> {
    let mut c = config.clone();
    match row {
        0 => {
            c.anchor_mode = if c.anchor_mode == AnchorMode::Room {
                AnchorMode::Head
            } else {
                AnchorMode::Room
            }
        }
        1 => c.upper_bar_height_deg += direction,
        2 => c.taskbar_height_deg += direction,
        3 => c.bar_distance_m += direction * 0.05,
        4 => c.radius_m += direction * 0.1,
        5 => c.dome_vertical_span_deg += direction * 5.,
        6..=8 => c.center_m[row - 6] += direction * 0.05,
        9 => c.yaw_deg += direction * 5.,
        10 => c.lane_gap_deg += direction * 0.5,
        11 => c.pullout_distance_m += direction * 0.05,
        12 => c.vertical_fov_deg += direction * 5.,
        13 => c.dome_upper_tilt_deg += direction,
        _ => return Err("Unknown XR setting".into()),
    }
    c.validate()?;
    Ok(c)
}
