//! Native Wayland application bells use the same session mute control.
use crate::Dome;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
impl smithay::wayland::xdg_system_bell::XdgSystemBellHandler for Dome {
    fn ring(&mut self, _surface: Option<WlSurface>) {
        if !self.session.locked() {
            self.sounds.play(dome::system_sounds::Event::Bell);
        }
    }
}
smithay::delegate_xdg_system_bell!(Dome);
