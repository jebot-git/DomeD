//! Reserved compositor mouse gesture; ordinary MMB remains application input.
use crate::shortcuts::Mods;
pub const MIDDLE: u32 = 0x112;
/// Shift selects all lanes in XR; desktop keeps its exact Super binding.
pub fn starts(button:u32, mods:Mods, xr:bool)->bool {
    button==MIDDLE && mods.logo && !mods.ctrl && !mods.alt && (!mods.shift || xr)
}
/// Capture the owning lane once, before the gesture clears pointer focus.
/// Gaps, shell surfaces and detached panels have no draggable lane target.
pub fn xr_lane(scene:&crate::Scene, target:Option<u64>)->Option<usize> {
    let id=target.filter(|id| !crate::xr_transport::is_shell(*id))?;
    if !scene.surface_visible_in(id,crate::Realm::Xr) {return None;}
    let window=scene.window(scene.popup_owner(id))?;
    (window.pullout_slot.is_none() && window.lane<3).then_some(window.lane)
}
/// Same horizontal angular sensitivity as the XR mouse pointer. Positive drag
/// moves the selected lane content right.
pub fn lane_delta(dx:f64)->Option<f32> {
    let degrees=(dx*0.0025).to_degrees();
    degrees.is_finite().then(|| degrees.rem_euclid(360.) as f32)
}
