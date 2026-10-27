//! Local, room-anchored XR environments. No application or input surface is added.
use crate::{Config, Pose, Scene};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backdrop {
    None,
    #[default]
    Precipice,
}
impl Backdrop {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Precipice => "L.A.2097 – Precipice",
        }
    }
    pub fn toggle(self) -> Self {
        if self == Self::None {
            Self::Precipice
        } else {
            Self::None
        }
    }
}

/// Runtime main-session visibility covers independent OpenXR applications;
/// native handoffs and WebXR also have explicit compositor-owned state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Presentation {
    pub runtime_visible: bool,
    pub external_primary: bool,
    pub native_active: bool,
    pub webxr_active: bool,
}
pub fn visible(scene: &Scene, state: Presentation) -> bool {
    scene.config.xr_backdrop != Backdrop::None
        && !scene.locked
        && state.runtime_visible
        && !state.external_primary
        && !state.native_active
        && !state.webxr_active
        && scene.xr_fullscreen().is_none()
}

/// Always room-anchored, even if the desktop is configured to follow the head.
/// Recentring the dome deliberately moves both the desktop and its environment.
pub fn anchor(config: &Config) -> Mat4 {
    Mat4::from_rotation_translation(
        Quat::from_rotation_y((-config.yaw_deg + config.xr_backdrop_yaw_deg).to_radians()),
        Vec3::from_array(config.center_m),
    )
}
pub fn view_projection(eye: Pose, fov: [f32; 4], config: &Config) -> Mat4 {
    let [left, right, down, up] = fov.map(f32::tan);
    let near = 0.03;
    let far = 12000.;
    let projection = Mat4::from_cols_array(&[
        2. / (right - left),
        0.,
        0.,
        0.,
        0.,
        -2. / (up - down),
        0.,
        0.,
        (right + left) / (right - left),
        -(up + down) / (up - down),
        far / (near - far),
        -1.,
        0.,
        0.,
        near * far / (near - far),
        0.,
    ]);
    let view = Mat4::from_rotation_translation(
        Quat::from_array(eye.orientation_xyzw),
        Vec3::from_array(eye.position_m),
    )
    .inverse();
    projection * view * anchor(config)
}
pub fn local_eye(eye: Pose, config: &Config) -> Vec3 {
    anchor(config)
        .inverse()
        .transform_point3(Vec3::from_array(eye.position_m))
}
pub fn directory() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DOME_XR_BACKDROP_PATH") {
        return Some(path.into());
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
    data.into_iter()
        .chain(
            std::env::var_os("XDG_DATA_DIRS")
                .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]),
        )
        .map(|p| p.join("domed/environments/la2097-precipice"))
        .find(|p| p.join("scene.json").is_file())
}

/// A single pausable clock drives geometry, particles and the audio mixer.
#[derive(Default, Debug)]
pub struct Clock {
    pub seconds: f64,
    previous: Option<f64>,
    active: bool,
}
impl Clock {
    pub fn advance(&mut self, now: f64, active: bool) -> f64 {
        if self.active && active {
            self.seconds += self
                .previous
                .map_or(0., |last| (now - last).clamp(0., 0.25));
        }
        self.previous = Some(now);
        self.active = active;
        self.seconds
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Motion {
    Traffic {
        origin: [f32; 3],
        velocity: [f32; 3],
        axis: usize,
        start: f32,
        finish: f32,
        reset: [f32; 3],
    },
    Rotation {
        origin: [f32; 3],
        velocity: [f32; 3],
        speed: f32,
    },
    Curve {
        origin: [f32; 3],
        keys: Vec<[f32; 4]>,
    },
}
impl Motion {
    pub fn matrix(&self, seconds: f64) -> Mat4 {
        let t = seconds as f32;
        match self {
            Self::Traffic {
                origin,
                velocity,
                axis,
                finish,
                reset,
                ..
            } => {
                let origin = Vec3::from_array(*origin);
                let velocity = Vec3::from_array(*velocity);
                let first = ((*finish - origin[*axis]) / velocity[*axis]).max(0.);
                let target = if t <= first {
                    origin + velocity * t
                } else {
                    let duration = ((*finish - reset[*axis]) / velocity[*axis])
                        .abs()
                        .max(0.001);
                    Vec3::from_array(*reset) + velocity * (t - first).rem_euclid(duration)
                };
                Mat4::from_translation(target - origin)
            }
            Self::Rotation {
                origin,
                velocity,
                speed,
            } => {
                let p = Vec3::from_array(*origin);
                Mat4::from_translation(p)
                    * Mat4::from_quat(Quat::from_axis_angle(
                        Vec3::from_array(*velocity).normalize_or_zero(),
                        (t * speed).to_radians(),
                    ))
                    * Mat4::from_translation(-p)
            }
            Self::Curve { keys, .. } => {
                let upper = keys.partition_point(|key| key[0] < t);
                let b = keys[upper.min(keys.len() - 1)];
                let a = keys[upper.saturating_sub(1)];
                let blend = if b[0] > a[0] {
                    ((t - a[0]) / (b[0] - a[0])).clamp(0., 1.)
                } else {
                    0.
                };
                Mat4::from_translation(
                    Vec3::new(a[1], a[2], a[3]).lerp(Vec3::new(b[1], b[2], b[3]), blend),
                )
            }
        }
    }
}
#[derive(Debug, Deserialize)]
pub struct Draw {
    pub texture: String,
    pub mode: u32,
    pub cutoff: f32,
    pub first: u32,
    pub count: u32,
    #[serde(default)]
    pub motions: Vec<usize>,
    #[serde(default)]
    pub uv_scroll: [f32; 2],
}
#[derive(Debug, Deserialize)]
pub struct Particles {
    pub texture: String,
    pub mode: u32,
    pub first: u32,
    pub count: u32,
    pub color: [f32; 4],
    pub steam: bool,
    #[serde(default)]
    pub motions: Vec<usize>,
    #[serde(default)]
    pub pulse: Option<[f32; 3]>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Sound {
    pub file: String,
    pub gain: f32,
    pub looping: bool,
    pub position: [f32; 3],
    pub attached: bool,
    pub min_distance: f32,
    pub max_distance: f32,
    pub spatial: f32,
    #[serde(default)]
    pub motions: Vec<usize>,
}
#[derive(Debug, Deserialize)]
pub struct Asset {
    pub version: u32,
    pub vertex_stride: u32,
    pub vertices: String,
    pub draws: Vec<Draw>,
    pub fog: [f32; 4],
    pub sky: [f32; 3],
    #[serde(default)]
    pub indices: String,
    #[serde(default)]
    pub motions: Vec<Motion>,
    #[serde(default)]
    pub particles: Vec<Particles>,
    #[serde(default)]
    pub particle_file: String,
    #[serde(default)]
    pub sounds: Vec<Sound>,
}
impl Asset {
    pub fn motion(&self, ids: &[usize], seconds: f64) -> Mat4 {
        ids.iter().fold(Mat4::IDENTITY, |matrix, id| {
            matrix * self.motions[*id].matrix(seconds)
        })
    }
    pub fn load(directory: &Path) -> Result<(Self, Vec<u8>), Box<dyn std::error::Error>> {
        let manifest = directory.join("scene.json");
        if manifest.metadata()?.len() > 2_097_152 {
            return Err("Backdrop manifest too large".into());
        }
        let asset: Self = serde_json::from_slice(&std::fs::read(manifest)?)?;
        if !matches!((asset.version, asset.vertex_stride), (1, 48) | (2, 32))
            || asset.draws.len() > 1024
            || asset.motions.len() > 128
            || asset.particles.len() > 64
            || asset.sounds.len() > 16
        {
            return Err("Unsupported backdrop scene".into());
        }
        let data = asset.read(directory, &asset.vertices, 512 * 1024 * 1024)?;
        let indices = asset.load_indices(directory, data.len())?;
        asset.validate(data.len(), indices.as_ref().map(Vec::len))?;
        for draw in &asset.draws {
            asset.file(directory, &draw.texture)?;
        }
        for p in &asset.particles {
            asset.file(directory, &p.texture)?;
        }
        for sound in &asset.sounds {
            asset.file(directory, &sound.file)?;
        }
        for row in data.chunks_exact(asset.vertex_stride as usize) {
            let full = if asset.version == 1 { row } else { &row[..12] };
            if full
                .chunks_exact(4)
                .any(|c| !f32::from_le_bytes(c.try_into().unwrap()).is_finite())
                || asset.version == 2
                    && row[12..]
                        .chunks_exact(2)
                        .any(|c| u16::from_le_bytes(c.try_into().unwrap()) & 0x7c00 == 0x7c00)
            {
                return Err("Nonfinite backdrop vertex".into());
            }
        }
        Ok((asset, data))
    }
    pub fn load_indices(
        &self,
        directory: &Path,
        vertices: usize,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        if self.version == 1 {
            return Ok(None);
        }
        let data = self.read(directory, &self.indices, 256 * 1024 * 1024)?;
        if data.is_empty()
            || data.len() % 12 != 0
            || data.chunks_exact(4).any(|c| {
                u32::from_le_bytes(c.try_into().unwrap()) as usize
                    >= vertices / self.vertex_stride as usize
            })
        {
            return Err("Invalid backdrop mesh indices".into());
        }
        Ok(Some(data))
    }
    pub fn particle_data(&self, directory: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if self.particles.is_empty() {
            return Ok(vec![]);
        }
        let data = self.read(directory, &self.particle_file, 16 * 1024 * 1024)?;
        if data.len() % 52 != 0
            || data
                .chunks_exact(4)
                .any(|c| !f32::from_le_bytes(c.try_into().unwrap()).is_finite())
            || self
                .particles
                .iter()
                .any(|p| u64::from(p.first) + u64::from(p.count) > (data.len() / 52) as u64)
        {
            return Err("Invalid backdrop particles".into());
        }
        Ok(data)
    }
    pub fn file(
        &self,
        directory: &Path,
        name: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if Path::new(name).components().count() != 1
            || !matches!(
                Path::new(name).components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err("Backdrop paths must be local filenames".into());
        }
        Ok(directory.join(name))
    }
    fn read(
        &self,
        directory: &Path,
        name: &str,
        max: u64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let path = self.file(directory, name)?;
        if path.metadata()?.len() > max {
            return Err("Backdrop asset too large".into());
        }
        let data = std::fs::read(path)?;
        if name.ends_with(".zst") {
            unsafe {
                let library = libloading::Library::new("libzstd.so.1")?;
                let size = *library
                    .get::<unsafe extern "C" fn(*const std::ffi::c_void, usize) -> u64>(
                        b"ZSTD_getFrameContentSize\0",
                    )?;
                let decompress = *library.get::<unsafe extern "C" fn(
                    *mut std::ffi::c_void,
                    usize,
                    *const std::ffi::c_void,
                    usize,
                ) -> usize>(b"ZSTD_decompress\0")?;
                let bytes = size(data.as_ptr().cast(), data.len());
                if bytes == 0 || bytes > max {
                    return Err("Invalid compressed backdrop buffer size".into());
                }
                let mut decoded = vec![0; bytes as usize];
                let actual = decompress(
                    decoded.as_mut_ptr().cast(),
                    decoded.len(),
                    data.as_ptr().cast(),
                    data.len(),
                );
                if actual != decoded.len() {
                    return Err("Backdrop decompression failed".into());
                }
                Ok(decoded)
            }
        } else {
            Ok(data)
        }
    }
    fn validate(&self, bytes: usize, indices: Option<usize>) -> Result<(), &'static str> {
        if bytes == 0
            || bytes % self.vertex_stride as usize != 0
            || self.draws.is_empty()
            || self
                .fog
                .iter()
                .chain(self.sky.iter())
                .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("Invalid backdrop data");
        }
        let count = indices.map_or(bytes / self.vertex_stride as usize, |size| size / 4) as u64;
        for d in &self.draws {
            if d.mode > 2
                || !d.cutoff.is_finite()
                || !(0. ..=1.).contains(&d.cutoff)
                || d.count == 0
                || d.count % 3 != 0
                || u64::from(d.first) + u64::from(d.count) > count
                || d.motions.iter().any(|id| *id >= self.motions.len())
                || d.uv_scroll.iter().any(|v| !v.is_finite())
            {
                return Err("Invalid backdrop draw range");
            }
        }
        for m in &self.motions {
            let valid = match m {
                Motion::Traffic {
                    origin,
                    velocity,
                    axis,
                    start,
                    finish,
                    reset,
                } => {
                    *axis < 3
                        && origin
                            .iter()
                            .chain(velocity)
                            .chain(reset)
                            .chain([start, finish])
                            .all(|v| v.is_finite())
                        && velocity[*axis].abs() > 0.0001
                }
                Motion::Rotation {
                    origin,
                    velocity,
                    speed,
                } => {
                    origin
                        .iter()
                        .chain(velocity)
                        .chain([speed])
                        .all(|v| v.is_finite())
                        && Vec3::from_array(*velocity).length() > 0.01
                }
                Motion::Curve { origin, keys } => {
                    !keys.is_empty()
                        && keys.len() <= 4096
                        && origin
                            .iter()
                            .chain(keys.iter().flatten())
                            .all(|v| v.is_finite())
                        && keys.windows(2).all(|w| w[0][0] < w[1][0])
                }
            };
            if !valid {
                return Err("Invalid backdrop motion");
            }
        }
        if self.sounds.iter().any(|s| {
            s.motions.iter().any(|id| *id >= self.motions.len())
                || !s.gain.is_finite()
                || !(0. ..=2.).contains(&s.gain)
                || !s.spatial.is_finite()
                || !(0. ..=1.).contains(&s.spatial)
                || s.position.iter().any(|v| !v.is_finite())
                || !s.min_distance.is_finite()
                || !s.max_distance.is_finite()
                || s.min_distance <= 0.
                || s.max_distance < s.min_distance
        }) {
            return Err("Invalid backdrop sound");
        }
        if self.particles.iter().any(|p| {
            p.mode < 1
                || p.mode > 2
                || p.count == 0
                || p.count > 2000
                || p.color.iter().any(|v| !v.is_finite() || *v < 0.)
                || p.motions.iter().any(|id| *id >= self.motions.len())
                || p.pulse.is_some_and(|v| {
                    v.iter().any(|x| !x.is_finite()) || v[0] <= 0. || v[1] < 0. || v[2] < 0.
                })
        }) {
            return Err("Invalid backdrop emitter");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_animation_clocks_pause_without_catchup() {
        let mut clock = Clock::default();
        assert_eq!(clock.advance(0., true), 0.);
        assert!((clock.advance(0.1, true) - 0.1).abs() < 1e-8);
        assert_eq!(clock.advance(5., false), 0.1);
        assert_eq!(clock.advance(9., false), 0.1);
        assert_eq!(clock.advance(12., true), 0.1);
        assert!((clock.advance(12.1, true) - 0.2).abs() < 1e-8);
    }
    #[test]
    fn traffic_wraps_on_authored_routes_and_rotations_keep_the_pivot() {
        let motion = Motion::Traffic {
            origin: [2., 3., 4.],
            velocity: [-10., 0., 0.],
            axis: 0,
            start: 10.,
            finish: -10.,
            reset: [10., 3., 4.],
        };
        let point = Vec3::new(2., 3., 4.);
        assert_eq!(motion.matrix(0.).transform_point3(point), point);
        assert!(
            (motion.matrix(0.5).transform_point3(point) - Vec3::new(-3., 3., 4.)).length() < 1e-5
        );
        assert!(
            (motion.matrix(1.5).transform_point3(point) - Vec3::new(7., 3., 4.)).length() < 1e-4
        );
        let rotate = Motion::Rotation {
            origin: [2., 3., 4.],
            velocity: [0., 1., 0.],
            speed: 90.,
        };
        assert!((rotate.matrix(2.).transform_point3(point) - point).length() < 1e-5);
        assert!((rotate.matrix(1.).transform_point3(point + Vec3::X) - point).length() - 1. < 1e-5);
    }
    #[test]
    fn fullscreen_sources_suspend_and_restore_without_mutating_desktop() {
        let mut scene = Scene::new("backdrop");
        let state = Presentation {
            runtime_visible: true,
            ..Default::default()
        };
        assert!(visible(&scene, state));
        for blocked in [
            Presentation {
                external_primary: true,
                ..state
            },
            Presentation {
                native_active: true,
                ..state
            },
            Presentation {
                webxr_active: true,
                ..state
            },
            Presentation::default(),
        ] {
            assert!(!visible(&scene, blocked));
            assert!(visible(&scene, state));
        }
        let id = scene.add("Film", crate::stereo::MPVR_SBS_APP_ID);
        {
            let w = scene.windows.iter_mut().find(|w| w.id == id).unwrap();
            w.stereo = crate::stereo::Stereo::SideBySide;
            w.maximized = true;
        }
        assert!(!visible(&scene, state));
        scene.apply(crate::Action::Minimize { id }).unwrap();
        assert!(visible(&scene, state));
        scene.locked = true;
        assert!(!visible(&scene, state));
    }
    #[test]
    fn backdrop_rotation_keeps_desktop_geometry_and_centre_fixed() {
        let mut scene = Scene::new("rotation");
        let frame = scene.frame_pose();
        scene.config.xr_backdrop_yaw_deg = 90.;
        assert_eq!(scene.frame_pose(), frame);
        let centre = Vec3::from_array(scene.config.center_m);
        assert!((anchor(&scene.config).transform_point3(Vec3::ZERO) - centre).length() < 1e-6);
        assert!(
            (anchor(&scene.config).transform_point3(-Vec3::Z) - (centre - Vec3::X)).length() < 1e-5
        );
    }
    #[test]
    fn anchoring_and_stereo_preserve_room_space() {
        let mut config = Config::default();
        config.center_m = [3., 2., -1.];
        config.yaw_deg = 30.;
        let original = anchor(&config);
        config.anchor_mode = crate::AnchorMode::Head;
        assert_eq!(original, anchor(&config));
        assert!(
            (original.transform_point3(Vec3::ZERO) - Vec3::from_array(config.center_m)).length()
                < 1e-6
        );
        let eye = Pose {
            position_m: config.center_m,
            orientation_xyzw: [0., 0., 0., 1.],
        };
        assert!(local_eye(eye, &config).length() < 1e-5);
        config.yaw_deg = 0.;
        let fov = [-0.7, 0.8, -0.65, 0.9];
        let eyes = [-0.032, 0.032].map(|x| Pose {
            position_m: [3. + x, 2., -1.],
            ..eye
        });
        let point = Vec3::new(0., 0., -3.);
        let projected = eyes.map(|e| view_projection(e, fov, &config).project_point3(point));
        assert!(projected[0].x > projected[1].x);
        assert!((0. ..1.).contains(&projected[0].z));
    }
    #[test]
    fn invalid_draws_cannot_address_outside_mesh() {
        let mut asset = Asset {
            indices: String::new(),
            motions: vec![],
            particles: vec![],
            particle_file: String::new(),
            sounds: vec![],
            version: 1,
            vertex_stride: 48,
            vertices: "vertices.bin".into(),
            draws: vec![Draw {
                texture: "white.png".into(),
                motions: vec![],
                uv_scroll: [0.; 2],
                mode: 0,
                cutoff: 0.,
                first: 0,
                count: 3,
            }],
            fog: [0.; 4],
            sky: [0.; 3],
        };
        assert!(asset.validate(144, None).is_ok());
        asset.draws[0].first = u32::MAX;
        assert!(asset.validate(144, None).is_err());
        assert!(asset.file(Path::new("/tmp"), "../outside").is_err());
    }
}
