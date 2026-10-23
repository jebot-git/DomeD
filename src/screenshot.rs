//! PNG capture storage and stereo projection, shared by desktop and XR.
use std::{
    io::Write,
    path::{Path, PathBuf},
};
pub const MAX_PIXELS: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Pixels {
    pub fn opaque_black(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || width as u64 * height as u64 > MAX_PIXELS as u64 {
            return Err("Desktop capture is too large".into());
        }
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for pixel in rgba.chunks_exact_mut(4) { pixel[3] = 255; }
        Ok(Self { width, height, rgba })
    }
    /// Copy one normalized output into the desktop canvas. Uncovered gaps stay black.
    pub fn blit(&mut self, source: &Self, x: u32, y: u32) -> Result<(), String> {
        if !self.valid() || !source.valid() ||
            x as u64 + source.width as u64 > self.width as u64 ||
            y as u64 + source.height as u64 > self.height as u64 {
            return Err("Output does not fit desktop capture".into());
        }
        for row in 0..source.height as usize {
            let start = ((y as usize + row) * self.width as usize + x as usize) * 4;
            let src = row * source.width as usize * 4;
            self.rgba[start..start + source.width as usize * 4]
                .copy_from_slice(&source.rgba[src..src + source.width as usize * 4]);
        }
        Ok(())
    }

    pub fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width as usize * self.height as usize <= MAX_PIXELS
            && self.rgba.len() == self.width as usize * self.height as usize * 4
    }
    pub fn crop(&self, a: [i32; 2], b: [i32; 2]) -> Option<Self> {
        if !self.valid() {
            return None;
        }
        let x = a[0].min(b[0]).clamp(0, self.width as i32);
        let y = a[1].min(b[1]).clamp(0, self.height as i32);
        let w = a[0].max(b[0]).clamp(0, self.width as i32) - x;
        let h = a[1].max(b[1]).clamp(0, self.height as i32) - y;
        if w == 0 || h == 0 {
            return None;
        }
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for row in y..y + h {
            let start = (row as usize * self.width as usize + x as usize) * 4;
            rgba.extend_from_slice(&self.rgba[start..start + w as usize * 4]);
        }
        Some(Self {
            width: w as u32,
            height: h as u32,
            rgba,
        })
    }
    pub fn png(&self) -> Result<Vec<u8>, String> {
        use image::ImageEncoder;
        if !self.valid() {
            return Err("Invalid screenshot dimensions".into());
        }
        let mut png = vec![];
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &self.rgba,
                self.width,
                self.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        Ok(png)
    }
}
/// Parse XDG user-dirs values without executing shell text.
pub fn pictures_dir(home: &Path, config: &str) -> PathBuf {
    let value = config
        .lines()
        .filter_map(|line| line.trim().strip_prefix("XDG_PICTURES_DIR="))
        .last()
        .unwrap_or("\"$HOME/Pictures\"")
        .trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    let value = value.replace("\\\"", "\"").replace("\\\\", "\\");
    let path = if value == "$HOME" || value == "${HOME}" {
        home.to_owned()
    } else if let Some(suffix) = value
        .strip_prefix("$HOME/")
        .or_else(|| value.strip_prefix("${HOME}/"))
    {
        home.join(suffix)
    } else {
        PathBuf::from(value)
    };
    if path.is_absolute() {
        path
    } else {
        home.join("Pictures")
    }
}
pub fn save_png(png: &[u8], stereo: bool) -> Result<PathBuf, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?);
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let contents = std::fs::read_to_string(config.join("user-dirs.dirs")).unwrap_or_default();
    let folder = pictures_dir(&home, &contents).join("Screenshots");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S_%f");
    let name = format!("Screenshot_{stamp}{}.png", if stereo { "_SBS" } else { "" });
    let path = folder.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    if let Err(e) = file.write_all(png).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(e.to_string());
    }
    Ok(path)
}
#[derive(Clone)]
pub struct Layer {
    pub quad: crate::xr_input::Quad,
    pub pixels: Pixels,
    pub stereo: bool,
    pub opacity: f32,
}
#[derive(Clone, Copy)]
pub struct Eye {
    pub pose: crate::Pose,
    pub fov: [f32; 4],
} // left,right,down,up, radians
/// Paint layers in the same order as OpenXR composition, using independent eye
/// positions/FOVs. Cylinder intersections reuse the compositor's spatial policy.
pub fn sbs(layers: &[Layer], eyes: [Eye; 2], width: u32, height: u32) -> Result<Pixels, String> {
    use glam::{Quat, Vec3};
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || width as usize * height as usize * 2 > MAX_PIXELS
        || layers.iter().any(|l| !l.pixels.valid())
    {
        return Err("Invalid stereo capture".into());
    }
    let mut result = Pixels {
        width: width * 2,
        height,
        rgba: vec![0; width as usize * height as usize * 8],
    };
    for (eye_index, eye) in eyes.iter().enumerate() {
        eye.pose.validate()?;
        let [left, right, down, up] = eye.fov.map(f32::tan);
        if ![left, right, down, up].iter().all(|f| f.is_finite()) || left >= right || down >= up {
            return Err("Invalid eye FOV".into());
        }
        let origin = Vec3::from_array(eye.pose.position_m);
        let rotation = Quat::from_array(eye.pose.orientation_xyzw);
        for y in 0..height {
            for x in 0..width {
                let ray = rotation
                    * Vec3::new(
                        left + (right - left) * (x as f32 + 0.5) / width as f32,
                        up - (up - down) * (y as f32 + 0.5) / height as f32,
                        -1.,
                    );
                let at = ((y * result.width + x + eye_index as u32 * width) * 4) as usize;
                // Opaque black outside DomeD layers; runtime passthrough/foreign
                // OpenXR overlays are not exposed to this application's renderer.
                result.rgba[at + 3] = 255;
                for layer in layers {
                    let Some([u, v]) = layer.quad.hit(origin, ray, false) else {
                        continue;
                    };
                    let p = &layer.pixels;
                    let stereo = layer.stereo && p.width >= 2;
                    let region_width = if stereo { p.width / 2 } else { p.width };
                    if region_width == 0 {
                        continue;
                    }
                    let sx = ((u * region_width as f32) as u32).min(region_width - 1)
                        + if stereo && eye_index == 1 {
                            p.width - region_width
                        } else {
                            0
                        };
                    let sy = ((v * p.height as f32) as u32).min(p.height - 1);
                    let pixel = &p.rgba[((sy * p.width + sx) * 4) as usize..][..4];
                    let opacity = layer.opacity.clamp(0., 1.);
                    let alpha = pixel[3] as f32 / 255. * opacity;
                    // DomeD surfaces use premultiplied alpha, like submitted layers.
                    for c in 0..3 {
                        result.rgba[at + c] = (pixel[c] as f32 * opacity
                            + result.rgba[at + c] as f32 * (1. - alpha))
                            .round()
                            .clamp(0., 255.) as u8;
                    }
                }
            }
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pose(x: f32, z: f32) -> crate::Pose {
        crate::Pose {
            position_m: [x, 0., z],
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
    fn eye(x: f32) -> Eye {
        Eye {
            pose: pose(x, 0.),
            fov: [-0.7853982, 0.7853982, -0.7853982, 0.7853982],
        }
    }
    fn layer(pixels: Pixels, stereo: bool) -> Layer {
        Layer {
            quad: crate::xr_input::Quad {
                id: 1,
                pose: pose(0., -2.),
                size: [4., 4.],
                dome: None,
            },
            pixels,
            stereo,
            opacity: 1.,
        }
    }
    #[test]
    fn xdg_pictures_expands_only_home_and_keeps_localized_paths() {
        let home = Path::new("/home/test");
        assert_eq!(
            pictures_dir(home, "XDG_PICTURES_DIR=\"$HOME/Obrázky\""),
            home.join("Obrázky")
        );
        assert_eq!(
            pictures_dir(home, "XDG_PICTURES_DIR=\"/mnt/images\""),
            PathBuf::from("/mnt/images")
        );
        assert_eq!(pictures_dir(home, "XDG_PICTURES_DIR=\"${HOME}\""), home);
        assert_eq!(
            pictures_dir(home, "XDG_PICTURES_DIR=\"$(touch /tmp/never)\""),
            home.join("Pictures")
        );
        assert_eq!(pictures_dir(home, ""), home.join("Pictures"));
    }
    #[test]
    fn reversed_clipped_region_keeps_top_down_rows_and_png() {
        let p = Pixels {
            width: 2,
            height: 2,
            rgba: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        };
        let crop = p.crop([2, 2], [-10, 0]).unwrap();
        assert_eq!(crop.rgba, p.rgba);
        assert_eq!(
            p.crop([0, 0], [1, 2]).unwrap().rgba,
            vec![255, 0, 0, 255, 0, 0, 255, 255]
        );
        assert!(p.crop([1, 1], [1, 1]).is_none());
        let decoded = image::load_from_memory(&crop.png().unwrap())
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded.as_raw(), &p.rgba);
    }
    #[test]
    fn stereo_sources_use_left_then_right_halves() {
        let pixels = Pixels {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let p = sbs(&[layer(pixels, true)], [eye(-0.032), eye(0.032)], 1, 1).unwrap();
        assert_eq!(p.width, 2);
        assert_eq!(p.rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
    }
    #[test]
    fn independent_eye_positions_produce_real_parallax() {
        let pixels = Pixels {
            width: 100,
            height: 1,
            rgba: (0..100).flat_map(|x| [x as u8, 0, 0, 255]).collect(),
        };
        let p = sbs(&[layer(pixels, false)], [eye(-0.1), eye(0.1)], 1, 1).unwrap();
        assert!(p.rgba[0] < p.rgba[4]);
    }
    #[test]
    fn premultiplied_layers_and_opacity_follow_composition_order() {
        let back = layer(
            Pixels {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 255, 255],
            },
            false,
        );
        let mut front = layer(
            Pixels {
                width: 1,
                height: 1,
                rgba: vec![128, 0, 0, 128],
            },
            false,
        );
        front.opacity = 0.5;
        let p = sbs(&[back, front], [eye(0.), eye(0.)], 1, 1).unwrap();
        assert_eq!(&p.rgba[..4], &[64, 0, 191, 255]);
    }
    #[test]
    fn rejects_invalid_images_and_fovs() {
        assert!(!Pixels {
            width: u32::MAX,
            height: u32::MAX,
            rgba: vec![]
        }
        .valid());
        let mut bad = eye(0.);
        bad.fov = [f32::NAN; 4];
        assert!(sbs(&[], [bad, eye(0.)], 1, 1).is_err());
        assert!(sbs(&[], [eye(0.); 2], 8192, 8192).is_err());
    }
    #[test]
    fn odd_stereo_width_matches_openxr_subimage_regions() {
        let pixels = Pixels {
            width: 5,
            height: 1,
            rgba: [10, 20, 30, 40, 50]
                .into_iter()
                .flat_map(|x| [x, 0, 0, 255])
                .collect(),
        };
        let p = sbs(&[layer(pixels, true)], [eye(0.); 2], 1, 1).unwrap();
        assert_eq!(p.rgba[0], 20);
        assert_eq!(p.rgba[4], 50);
    }
    #[test]
    fn curved_surfaces_use_the_same_headset_ray_intersections() {
        let mut curved = layer(
            Pixels {
                width: 1,
                height: 1,
                rgba: vec![10, 20, 30, 255],
            },
            false,
        );
        curved.quad.dome = Some(crate::xr_input::DomePatch {
            frame: pose(0., 0.),
            radius: 2.,
            horizontal: 1.5,
            top: 0.5,
            bottom: -0.5,
        });
        let p = sbs(&[curved], [eye(-0.032), eye(0.032)], 1, 1).unwrap();
        assert_eq!(p.rgba, vec![10, 20, 30, 255, 10, 20, 30, 255]);
    }
}
