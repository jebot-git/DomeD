use crate::Dome;
use smithay::{
    delegate_xdg_decoration,
    reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
    wayland::shell::xdg::{decoration::XdgDecorationHandler, ToplevelSurface},
};
impl XdgDecorationHandler for Dome {
    fn new_decoration(&mut self, s: ToplevelSurface) {
        self.request_mode(s, Mode::ServerSide);
    }
    fn request_mode(&mut self, s: ToplevelSurface, _: Mode) {
        s.with_pending_state(|p| p.decoration_mode = Some(Mode::ServerSide));
        // The first configure follows the initial surface commit, after the
        // client has supplied parent/size limits and XR launch sizing is known.
        if s.is_initial_configure_sent() { s.send_pending_configure(); }
    }
    fn unset_mode(&mut self, s: ToplevelSurface) {
        self.request_mode(s, Mode::ServerSide);
    }
}
delegate_xdg_decoration!(Dome);
