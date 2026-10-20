//! cmuxr spectrum bridge. Frames expire after 500 ms; no synthetic animation.
use serde::Deserialize;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
struct State {
    version: u32,
    timestamp: f64,
    enabled: bool,
    playing: bool,
    bands: Vec<f32>,
}
impl State {
    fn valid(&self, now: f64) -> bool {
        self.version == 1 && self.enabled && self.playing
            && self.timestamp.is_finite() && (0.0..=0.5).contains(&(now-self.timestamp))
            && self.bands.len() == 16
            && self.bands.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x))
    }
}

pub struct Frame {
    pub rgba: Vec<u8>,
    pub serial: u64,
    pub size: [f32; 2],
}

pub struct MusicFloor {
    last_read: Instant,
    state: Option<State>,
    serial: u64,
}
impl Default for MusicFloor {
    fn default() -> Self {
        Self { last_read: Instant::now() - Duration::from_secs(1), state: None, serial: 0 }
    }
}
impl MusicFloor {
    /// Only a valid STAGE rectangle is accepted: LOCAL is never assumed to be a floor.
    pub fn frame(&mut self, bounds: Option<[f32; 2]>) -> Option<Frame> {
        let size = bounds?;
        if size.iter().any(|v| !v.is_finite() || *v <= 0. || *v > 100.) { return None; }
        if self.last_read.elapsed() >= Duration::from_millis(50) {
            self.last_read = Instant::now();
            self.serial += 1;
            self.state = std::env::var_os("XDG_RUNTIME_DIR")
                .and_then(|root| std::fs::read(std::path::PathBuf::from(root).join("cmuxr-floor.json")).ok())
                .filter(|bytes| bytes.len() < 4096)
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
        let state = self.state.as_ref().filter(|s| s.valid(now))?;
        Some(Frame { rgba: tiles(&state.bands), serial: self.serial, size })
    }
}

/// Sixteen spectrum columns, sixteen tiles deep, with transparent grout.
fn tiles(bands: &[f32]) -> Vec<u8> {
    let mut rgba = vec![0; 128*128*4];
    for y in 0..128 {
        for x in 0..128 {
            let band = x/8;
            let level = 15-y/8;
            if x%8 == 0 || y%8 == 0 || bands[band] <= level as f32/16. { continue; }
            let colours = [[55,130,255], [45,235,170], [240,200,55], [245,65,155]];
            let colour = colours[level/4];
            let i = (y*128+x)*4;
            rgba[i..i+4].copy_from_slice(&[colour[0],colour[1],colour[2],190]);
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_rejects_stale_disabled_paused_and_invalid_audio() {
        let mut s = State {version:1,timestamp:100.,enabled:true,playing:true,bands:vec![0.5;16]};
        assert!(s.valid(100.2));
        assert!(!s.valid(100.6)); assert!(!s.valid(99.));
        s.enabled=false; assert!(!s.valid(100.)); s.enabled=true;
        s.playing=false; assert!(!s.valid(100.)); s.playing=true;
        s.bands[0]=f32::NAN; assert!(!s.valid(100.));
    }
    #[test]
    fn silence_is_transparent_and_columns_follow_audio() {
        assert!(tiles(&[0.;16]).iter().all(|x| *x==0));
        let mut bands=[0.;16]; bands[0]=1.;
        let pixels=tiles(&bands);
        assert_eq!(pixels[(1*128+1)*4+3],190);
        assert_eq!(pixels[(1*128+9)*4+3],0);
        assert_eq!(pixels[(1*128)*4+3],0);
    }
    #[test]
    fn no_floor_without_stage_bounds() {
        let mut floor=MusicFloor::default();
        assert!(floor.frame(None).is_none());
        assert!(floor.frame(Some([f32::NAN,2.])).is_none());
        assert!(floor.frame(Some([0.,2.])).is_none());
    }
}

/// An explicitly chosen extent also applies to the integrated DomeD renderer.
pub fn configured_bounds() -> Option<[f32; 2]> {
    let root = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("cmuxr/settings.json")).ok()?).ok()?;
    let dimensions = value.get("floor_size_m")?.as_array()?;
    if dimensions.len() != 2 { return None; }
    let size = [dimensions[0].as_f64()? as f32, dimensions[1].as_f64()? as f32];
    size.iter().all(|v| v.is_finite() && *v > 0. && *v <= 100.).then_some(size)
}
