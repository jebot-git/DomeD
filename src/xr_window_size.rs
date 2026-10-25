//! Initial XR content resolution. UI/buffer scale and spatial transforms are separate.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    #[serde(rename = "720p")]
    Hd,
    #[serde(rename = "900p")]
    HdPlus,
    #[default]
    #[serde(rename = "1080p")]
    FullHd,
    #[serde(rename = "1440p")]
    Qhd,
}
impl Resolution {
    pub const ALL: [Self;4] = [Self::Hd, Self::HdPlus, Self::FullHd, Self::Qhd];
    pub fn pixels(self) -> [i32;2] {
        match self { Self::Hd => [1280,720], Self::HdPlus => [1600,900],
            Self::FullHd => [1920,1080], Self::Qhd => [2560,1440] }
    }
    pub fn label(self) -> &'static str {
        match self { Self::Hd => "720p (1280 × 720)", Self::HdPlus => "900p (1600 × 900)",
            Self::FullHd => "1080p (1920 × 1080)", Self::Qhd => "QHD (2560 × 1440)" }
    }
    pub fn step(self, direction: f32) -> Self {
        let index = Self::ALL.iter().position(|r| *r == self).unwrap() as i32;
        Self::ALL[(index + direction.signum() as i32).clamp(0,3) as usize]
    }
    /// Explicit client limits take precedence over the ordinary launch default.
    pub fn initial_size(self, min: [i32;2], max: [i32;2]) -> [i32;2] {
        let requested = self.pixels();
        std::array::from_fn(|i| {
            let lo = min[i].max(1);
            let hi = if max[i] > 0 { max[i].max(lo) } else { i32::MAX };
            requested[i].clamp(lo,hi)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_cover_720p_to_qhd_without_scaling() {
        assert_eq!(Resolution::default().pixels(),[1920,1080]);
        assert_eq!(Resolution::Hd.step(-1.),Resolution::Hd);
        assert_eq!(Resolution::Qhd.step(1.),Resolution::Qhd);
        for pair in Resolution::ALL.windows(2) {
            assert_eq!(pair[0].step(1.),pair[1]);
            assert_eq!(pair[1].step(-1.),pair[0]);
        }
        for resolution in Resolution::ALL {
            assert_eq!(resolution.initial_size([0,0],[0,0]),resolution.pixels());
        }
    }
    #[test]
    fn explicit_client_limits_remain_authoritative() {
        assert_eq!(Resolution::Qhd.initial_size([640,480],[640,480]),[640,480]);
        assert_eq!(Resolution::Hd.initial_size([1600,900],[0,0]),[1600,900]);
        assert_eq!(Resolution::Qhd.initial_size([0,0],[1920,1080]),[1920,1080]);
    }
}
