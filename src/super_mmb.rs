//! Reserved compositor mouse gesture; ordinary MMB remains application input.
use crate::shortcuts::Mods;
pub const MIDDLE: u32 = 0x112;
pub fn starts(button:u32, mods:Mods)->bool {
    button==MIDDLE && mods.logo && !mods.ctrl && !mods.alt && !mods.shift
}
/// Same horizontal angular sensitivity as the XR mouse pointer. Positive drag
/// moves the lane content right, keeping relative lane offsets unchanged.
pub fn lane_delta(dx:f64)->Option<f32> {
    let degrees=(dx*0.0025).to_degrees();
    degrees.is_finite().then(|| degrees.rem_euclid(360.) as f32)
}
