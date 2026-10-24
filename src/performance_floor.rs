//! htop's bitmap-labelled GPU canvas on the same STAGE floor as cmuxr.
//! The producer publishes a matched JSON/RGBA pair once per sample. Nothing
//! is synthesized when collection pauses, exits, or becomes stale.
use serde::Deserialize;
use std::{fs::OpenOptions, io::Read, os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

pub const ID: u64 = u64::MAX - 33;
pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 256;
const PIXELS: usize = WIDTH as usize * HEIGHT as usize * 4;

#[derive(Deserialize)]
struct Meter { label: String, value: f64 }
#[derive(Deserialize)]
struct State {
    version: u32, timestamp: f64, enabled: bool, serial: u64,
    width: u32, height: u32, meters: Vec<Meter>,
}
impl State {
    fn valid(&self, now: f64) -> bool {
        self.version == 1 && self.enabled && self.serial != 0
            && self.timestamp.is_finite() && (0.0..=2.5).contains(&(now-self.timestamp))
            && self.width == WIDTH && self.height == HEIGHT && self.meters.len() == 3
            && self.meters.iter().zip(["CPU", "RAM", "SWAP"]).all(|(m, name)|
                m.label == name && m.value.is_finite() && (0.0..=1.0).contains(&m.value))
    }
}

/// No symlinks, FIFOs, other users' files, unbounded allocation, or partial reads.
fn read_private(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let file = OpenOptions::new().read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK).open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.uid() != nix::unistd::getuid().as_raw()
        || meta.mode() & 0o077 != 0 || meta.len() > limit as u64 { return None; }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}
fn decode_pixels(bytes: &[u8], serial: u64) -> Option<Vec<u8>> {
    if bytes.len() != PIXELS + 12 || bytes.get(..4)? != b"HTPF"
        || u64::from_le_bytes(bytes.get(4..12)?.try_into().ok()?) != serial { return None; }
    Some(bytes[12..].to_vec())
}

pub struct Frame { pub rgba: Vec<u8>, pub serial: u64, pub size: [f32; 2], pub position: [f32; 3] }
/// Fit a readable strip inside the forward edge of the actual play rectangle.
fn placement(bounds: Option<[f32; 2]>) -> Option<([f32; 2], [f32; 3])> {
    let [w, d] = bounds?;
    if [w,d].iter().any(|v| !v.is_finite() || *v <= 0. || *v > 100.) { return None; }
    let width = (w * 0.9).min(2.8).min(d * 0.8 * 4.);
    let depth = width / 4.;
    Some(([width,depth],[0.,0.008,-d/2. + depth/2. + d*0.05]))
}
pub struct PerformanceFloor {
    last_read: Instant, state: Option<State>, rgba: Vec<u8>,
}
impl Default for PerformanceFloor {
    fn default() -> Self {
        Self { last_read: Instant::now()-Duration::from_secs(1), state: None, rgba: Vec::new() }
    }
}
impl PerformanceFloor {
    pub fn frame(&mut self, bounds: Option<[f32; 2]>) -> Option<Frame> {
        let (size, position) = placement(bounds)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
        if self.last_read.elapsed() >= Duration::from_millis(100) {
            self.last_read = Instant::now();
            let state = std::env::var_os("XDG_RUNTIME_DIR").and_then(|root| {
                let root = std::path::PathBuf::from(root);
                let bytes = read_private(&root.join("htop-floor.json"), 4096)?;
                let state: State = serde_json::from_slice(&bytes).ok()?;
                if !state.valid(now) { return None; }
                if self.state.as_ref().is_none_or(|s| s.serial != state.serial || s.timestamp != state.timestamp) {
                    let bytes = read_private(&root.join("htop-floor.rgba"), PIXELS+12)?;
                    self.rgba = decode_pixels(&bytes, state.serial)?;
                }
                Some(state)
            });
            // A mismatched publication is skipped until the next poll; old data
            // never acquires the newer timestamp and cannot live indefinitely.
            self.state = state;
        }
        let state = self.state.as_ref().filter(|s| s.valid(now))?;
        Some(Frame { rgba: self.rgba.clone(), serial: state.serial, size, position })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> State { State {version:1,timestamp:100.,enabled:true,serial:1,width:WIDTH,height:HEIGHT,
        meters:["CPU","RAM","SWAP"].iter().map(|s|Meter{label:s.to_string(),value:0.5}).collect()} }
    #[test]
    fn stale_paused_future_and_bad_meters_are_hidden() {
        let mut s=sample(); assert!(s.valid(102.5)); assert!(!s.valid(102.51)); assert!(!s.valid(99.));
        s.enabled=false; assert!(!s.valid(100.)); s.enabled=true;
        s.meters[0].value=f64::NAN; assert!(!s.valid(100.));
        s.meters[0].value=1.1; assert!(!s.valid(100.));
        s.meters[0].value=0.5; s.meters[0].label="GPU".into(); assert!(!s.valid(100.));
    }
    #[test]
    fn pixels_must_match_serial_and_exact_canvas_size() {
        let mut bytes=b"HTPF".to_vec(); bytes.extend_from_slice(&17u64.to_le_bytes()); bytes.resize(PIXELS+12,42);
        assert_eq!(decode_pixels(&bytes,17).unwrap().len(),PIXELS);
        assert!(decode_pixels(&bytes,18).is_none()); bytes.push(0); assert!(decode_pixels(&bytes,17).is_none());
    }
    #[test]
    fn strip_is_on_stage_inside_bounds_and_preserves_aspect() {
        assert!(placement(None).is_none()); assert!(placement(Some([f32::NAN,2.])).is_none());
        assert!(placement(Some([2.,0.])).is_none());
        for bounds in [[2.,2.],[10.,1.],[0.5,4.]] {
            let (size,p)=placement(Some(bounds)).unwrap();
            assert!(size[0]<=bounds[0] && size[1]<=bounds[1]); assert_eq!(size[0]/size[1],4.);
            assert!(p[2]-size[1]/2. >= -bounds[1]/2.); assert!(p[2]+size[1]/2. <= bounds[1]/2.);
            assert!(p[1]>0.005); // above cmuxr, without coplanar flicker
        }
    }
    #[test]
    fn private_reads_reject_symlinks_permissions_and_oversize() {
        use std::os::unix::fs::{symlink,PermissionsExt};
        let dir=std::env::temp_dir().join(format!("htop-floor-test-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap(); let path=dir.join("state");
        std::fs::write(&path,b"test").unwrap();
        std::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_private(&path,4).unwrap(),b"test"); assert!(read_private(&path,3).is_none());
        let link=dir.join("link"); symlink(&path,&link).unwrap(); assert!(read_private(&link,4).is_none());
        std::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o644)).unwrap(); assert!(read_private(&path,4).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
