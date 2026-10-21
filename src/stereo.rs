//! Opt-in client identity and per-eye texture regions for squeezed SBS surfaces.
use serde::{Deserialize, Serialize};

pub const QIMGVR_SBS_APP_ID: &str = "org.qimgvr.Sbs";
pub const MPVR_SBS_APP_ID: &str = "org.mpvr.Sbs";
pub const GMSH_SBS_APP_ID: &str = "org.gmsh.Sbs";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stereo {
    #[default]
    Mono,
    SideBySide,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eye { Both, Left, Right }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region { pub eye: Eye, pub x: u32, pub width: u32 }

impl Stereo {
    pub fn for_app_id(app_id: &str) -> Self {
        if matches!(app_id, QIMGVR_SBS_APP_ID | MPVR_SBS_APP_ID | GMSH_SBS_APP_ID) { Self::SideBySide } else { Self::Mono }
    }
    pub fn layer_count(self) -> usize { if self == Self::SideBySide { 2 } else { 1 } }
    pub fn regions(self, width: u32) -> Vec<Region> {
        if self == Self::SideBySide && width >= 2 {
            vec![Region { eye: Eye::Left, x: 0, width: width / 2 },
                 Region { eye: Eye::Right, x: width - width / 2, width: width / 2 }]
        } else { vec![Region { eye: Eye::Both, x: 0, width }] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stereo_is_opt_in_and_eye_regions_do_not_overlap() {
        assert_eq!(Stereo::for_app_id("qimgvr"), Stereo::Mono);
        assert_eq!(Stereo::for_app_id(QIMGVR_SBS_APP_ID), Stereo::SideBySide);
        assert_eq!(Stereo::for_app_id(MPVR_SBS_APP_ID), Stereo::SideBySide);
        assert_eq!(Stereo::for_app_id(GMSH_SBS_APP_ID), Stereo::SideBySide);
        assert_eq!(Stereo::for_app_id("gmsh"), Stereo::Mono);
        assert_eq!(Stereo::for_app_id("mpv"), Stereo::Mono);
        let stereo = Stereo::SideBySide;
        assert_eq!(stereo.layer_count(), 2);
        assert_eq!(stereo.regions(1280), vec![Region { eye: Eye::Left, x: 0, width: 640 }, Region { eye: Eye::Right, x: 640, width: 640 }]);
        assert_eq!(stereo.regions(5)[1].x, 3);
        assert_eq!(Stereo::Mono.regions(1280)[0].eye, Eye::Both);
    }
}
