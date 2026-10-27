//! Shared Appearance panel rows and viewport geometry for desktop and XR.
use crate::{xr_backdrop::Backdrop, Config};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackdropRow {
    Desktop,
    Scene,
    Volume,
    Rotation,
}
pub fn row_count(config: &Config) -> usize {
    if config.xr_backdrop == Backdrop::None {
        10
    } else {
        12
    }
}
pub fn backdrop_row(config: &Config, row: usize) -> Option<BackdropRow> {
    match row {
        7 => Some(BackdropRow::Desktop),
        8 => Some(BackdropRow::Scene),
        9 if config.xr_backdrop != Backdrop::None => Some(BackdropRow::Volume),
        10 if config.xr_backdrop != Backdrop::None => Some(BackdropRow::Rotation),
        _ => None,
    }
}
pub fn volume(config: &Config) -> f32 {
    if config.xr_backdrop_sound {
        config.xr_backdrop_volume
    } else {
        0.
    }
}
pub fn adjust(config: &Config, row: BackdropRow, direction: i32) -> Result<Config, String> {
    let mut config = config.clone();
    match row {
        BackdropRow::Desktop => config.desktop_backdrop = !config.desktop_backdrop,
        BackdropRow::Scene => config.xr_backdrop = config.xr_backdrop.toggle(),
        BackdropRow::Volume => {
            config.xr_backdrop_volume =
                (volume(&config) + direction.signum() as f32 * 0.05).clamp(0., 1.);
            config.xr_backdrop_sound = true;
        }
        BackdropRow::Rotation => {
            config.xr_backdrop_yaw_deg =
                (config.xr_backdrop_yaw_deg + direction.signum() as f32 * 5.).rem_euclid(360.)
        }
    }
    config.validate()?;
    Ok(config)
}
pub fn reset(config: &Config) -> Config {
    let mut config = config.clone();
    let defaults = Config::default();
    config.desktop_backdrop = defaults.desktop_backdrop;
    config.xr_backdrop = defaults.xr_backdrop;
    config.xr_backdrop_sound = defaults.xr_backdrop_sound;
    config.xr_backdrop_volume = defaults.xr_backdrop_volume;
    config.xr_backdrop_yaw_deg = defaults.xr_backdrop_yaw_deg;
    config
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Layout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub first: usize,
    pub visible: usize,
}
impl Layout {
    pub fn new(width: f64, height: f64, rows: usize, selected: usize) -> Self {
        let w = 600f64.min((width - 32.).max(160.));
        let h = (560. + rows.saturating_sub(8) as f64 * 48.).min((height - 112.).max(212.));
        let visible = (((h - 176.) / 48.).floor().max(1.) as usize).min(rows);
        let selected = selected.min(rows.saturating_sub(1));
        let first = selected
            .saturating_sub(visible.saturating_sub(1))
            .min(rows.saturating_sub(visible));
        Self {
            x: (width - w) / 2.,
            y: 44. + (height - 112. - h) / 2.,
            width: w,
            height: h,
            first,
            visible,
        }
    }
    pub fn hit(&self, x: f64, y: f64, rows: usize) -> Option<usize> {
        if x < self.x || x > self.x + self.width || y < self.y || y > self.y + self.height {
            return None;
        }
        if y >= self.y + self.height - 48. {
            return Some(rows + usize::from(x >= self.x + self.width / 2.));
        }
        if x < self.x + self.width / 2. || y < self.y + 60. {
            return None;
        }
        let slot = ((y - self.y - 60.) / 48.) as usize;
        (slot < self.visible).then_some(self.first + slot)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditional_rows_and_preferences_preserve_dome_geometry() {
        let mut c = Config::default();
        c.xr_backdrop = Backdrop::None;
        assert_eq!(row_count(&c), 10);
        assert_eq!(backdrop_row(&c, 9), None);
        let c = adjust(&c, BackdropRow::Scene, 1).unwrap();
        assert_eq!(row_count(&c), 12);
        assert_eq!(backdrop_row(&c, 10), Some(BackdropRow::Rotation));
        let v = adjust(&c, BackdropRow::Volume, -1).unwrap();
        assert!((v.xr_backdrop_volume - 0.95).abs() < 1e-6);
        let r = adjust(&v, BackdropRow::Rotation, -1).unwrap();
        assert_eq!(r.xr_backdrop_yaw_deg, 355.);
        assert_eq!(r.yaw_deg, c.yaw_deg);
        assert_eq!(r.center_m, c.center_m);
        let restored: Config = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        assert_eq!(restored, r);
        let reset = reset(&r);
        assert_eq!(reset.center_m, r.center_m);
        assert_eq!(reset.xr_backdrop_yaw_deg, 0.);
    }
    #[test]
    fn compact_panels_keep_selected_rows_and_footer_accessible() {
        for height in [400., 600., 720., 1080.] {
            for selected in 0..12 {
                let frame = Layout::new(800., height, 12, selected);
                assert!(frame.y >= 36. && frame.y + frame.height <= height - 60.);
                assert!(selected >= frame.first && selected < frame.first + frame.visible);
                let y = frame.y + 60. + (selected - frame.first) as f64 * 48. + 20.;
                assert_eq!(
                    frame.hit(frame.x + frame.width - 40., y, 12),
                    Some(selected)
                );
                assert_eq!(
                    frame.hit(frame.x + 20., frame.y + frame.height - 20., 12),
                    Some(12)
                );
            }
        }
    }
}
