//! Passive cursor and focus feedback. These overlays never participate in hit testing.
use crate::Realm;
use std::time::{Duration, Instant};

pub const FINDER_SIZE: u32 = 128;
pub const FINDER_DURATION: f32 = 0.66;
pub const ACCENT: [u8; 3] = [107, 171, 255];

#[derive(Default)]
pub struct CursorFinder {
    last: Option<(Instant, Realm, [f64; 2])>,
    down: bool,
    animation: Option<(Instant, Realm)>,
}
impl CursorFinder {
    pub fn button(&mut self, button: u32, pressed: bool, realm: Realm, point: [f64; 2], now: Instant) {
        if button != 0x112 {
            if pressed { self.last = None; }
            return;
        }
        if !pressed { self.down = false; return; }
        if self.down { return; }
        self.down = true;
        let double = self.last.is_some_and(|(time, view, p)| {
            view == realm && now.saturating_duration_since(time) <= Duration::from_millis(400)
                && (point[0] - p[0]).hypot(point[1] - p[1]) <= 8.
        });
        if double {
            self.animation = Some((now, realm));
            self.last = None;
        } else {
            self.last = Some((now, realm, point));
        }
    }
    pub fn started(&self, realm: Realm) -> Option<Instant> {
        self.animation.filter(|(_, view)| *view == realm).map(|(time, _)| time)
    }
    pub fn elapsed(&self, realm: Realm, now: Instant) -> Option<f32> {
        self.started(realm).map(|time| now.saturating_duration_since(time).as_secs_f32())
            .filter(|age| *age < FINDER_DURATION)
    }
}

pub fn accent(value: &str) -> [u8; 3] {
    if value.len() != 7 || !value.starts_with('#') || !value[1..].is_ascii() { return ACCENT; }
    let colors: Option<Vec<_>> = [1, 3, 5].into_iter()
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).ok()).collect();
    colors.map(|c| [c[0], c[1], c[2]]).unwrap_or(ACCENT)
}

/// Three short, staggered rings contract towards the live pointer hotspot.
pub fn finder_pixels(seconds: f32, color: [u8; 3]) -> Vec<u8> {
    let n = FINDER_SIZE as usize;
    let mut pixels = vec![0; n * n * 4];
    if !seconds.is_finite() || !(0. ..FINDER_DURATION).contains(&seconds) { return pixels; }
    for pulse in 0..3 {
        let phase = (seconds - pulse as f32 * 0.18) / 0.30;
        if !(0. ..1.).contains(&phase) { continue; }
        let radius = 6. + 50. * (1. - phase).powi(2);
        let opacity = (std::f32::consts::PI * phase).sin() * 0.78;
        for y in 0..n {
            for x in 0..n {
                let distance = ((x as f32 - 63.5).hypot(y as f32 - 63.5) - radius).abs();
                let (rgb, a) = if distance < 1.5 { (color, opacity) }
                    else if distance < 3. { ([12, 18, 26], opacity * 0.45) } else { continue; };
                let at = (y * n + x) * 4;
                if (a * 255.) as u8 > pixels[at + 3] {
                    for c in 0..3 { pixels[at + c] = (rgb[c] as f32 * a).round() as u8; }
                    pixels[at + 3] = (a * 255.) as u8;
                }
            }
        }
    }
    pixels
}

/// Blend a one-pixel accent edge and a faint second pixel inside the client.
pub fn focus_border(pixels: &mut [u8], width: u32, height: u32, color: [u8; 3], stereo: bool) {
    focus_border_scaled(pixels, width, height, color, stereo, 1.);
}
pub fn focus_border_scaled(pixels: &mut [u8], width: u32, height: u32, color: [u8; 3], stereo: bool, scale: f32) {
    let inset = (2. * scale).ceil() as u32;
    if width == 0 || height == 0 || pixels.len() != width as usize * height as usize * 4 { return; }
    let eyes = if stereo { 2 } else { 1 };
    for eye in 0..eyes {
        let start = eye * width / eyes;
        let end = (eye + 1) * width / eyes;
        let eye_width = end - start;
        if eye_width == 0 { continue; }
        for y in 0..height {
            let ranges = if y < inset || y >= height.saturating_sub(inset) || eye_width <= 2 * inset {
                [start..end, end..end]
            } else { [start..start + inset, end - inset..end] };
            for range in ranges {
                for x in range {
                    let local = x - start;
                    let edge = local.min(eye_width - 1 - local).min(y.min(height - 1 - y));
                    let alpha = if (edge as f32) < scale { 0.60 } else if (edge as f32) < 2. * scale { 0.16 } else { continue };
                    let at = ((y * width + x) * 4) as usize;
                    let opacity = pixels[at + 3] as f32 / 255.;
                    for c in 0..3 { pixels[at + c] = (pixels[at + c] as f32 * (1. - alpha) + color[c] as f32 * alpha * opacity).round() as u8; }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finder_requires_released_nearby_middle_clicks_in_one_realm() {
        let now = Instant::now();
        let mut f = CursorFinder::default();
        f.button(0x112, true, Realm::Desktop, [10., 10.], now);
        f.button(0x112, true, Realm::Desktop, [10., 10.], now);
        assert_eq!(f.elapsed(Realm::Desktop, now), None);
        f.button(0x112, false, Realm::Desktop, [10., 10.], now);
        f.button(0x112, true, Realm::Desktop, [13., 12.], now + Duration::from_millis(200));
        assert_eq!(f.elapsed(Realm::Desktop, now + Duration::from_millis(200)), Some(0.));
        assert_eq!(f.elapsed(Realm::Xr, now + Duration::from_millis(200)), None);
        assert_eq!(f.elapsed(Realm::Desktop, now + Duration::from_secs(1)), None);
        for (view, p, delay) in [(Realm::Xr, [10., 10.], 100), (Realm::Desktop, [40., 10.], 100), (Realm::Desktop, [10., 10.], 401)] {
            let mut f = CursorFinder::default();
            f.button(0x112, true, Realm::Desktop, [10., 10.], now);
            f.button(0x112, false, Realm::Desktop, [10., 10.], now);
            f.button(0x112, true, view, p, now + Duration::from_millis(delay));
            assert_eq!(f.elapsed(view, now + Duration::from_millis(delay)), None);
        }
    }
    #[test]
    fn rings_contract_three_times_and_leave_the_hotspot_clear() {
        for time in [0.10, 0.34, 0.58] {
            let image = finder_pixels(time, ACCENT);
            assert!(image.chunks_exact(4).any(|p| p[3] > 0));
            assert_eq!(image[(64 * 128 + 64) * 4 + 3], 0);
        }
        assert!(finder_pixels(FINDER_DURATION, ACCENT).iter().all(|b| *b == 0));
    }
    #[test]
    fn focus_stays_inside_two_pixels_and_both_stereo_eyes() {
        for stereo in [false, true] {
            let mut pixels = vec![40; 20 * 12 * 4];
            focus_border(&mut pixels, 20, 12, ACCENT, stereo);
            assert_eq!(&pixels[(5 * 20 + 5) * 4..(5 * 20 + 5) * 4 + 4], &[40; 4]);
            assert_ne!(pixels[0], 40);
            assert_eq!(pixels[3], 40);
            if stereo { assert_ne!(pixels[(5 * 20 + 10) * 4], 40); }
        }
    }
}
