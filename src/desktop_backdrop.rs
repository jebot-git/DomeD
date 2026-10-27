//! Original scalable LinuXR desktop artwork, selected by the shell colour scheme.
pub const SVG: &str = include_str!("../assets/backgrounds/linuxr.svg");
pub const LIGHT_SVG: &str = include_str!("../assets/backgrounds/linuxr-light.svg");
pub fn svg(light: bool) -> &'static str {
    if light {
        LIGHT_SVG
    } else {
        SVG
    }
}
pub fn background(light: bool) -> [u8; 3] {
    if light {
        [216, 221, 224]
    } else {
        [43, 49, 56]
    }
}
