mod compositor;
mod decoration;
mod xdg_shell;

use crate::Dome;

//
// Wl Seat
//

use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
    ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::{delegate_data_device, delegate_output, delegate_seat};

impl SeatHandler for Dome {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Dome> {
        &mut self.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        image: smithay::input::pointer::CursorImageStatus,
    ) {
        self.cursor_status = image;
    }

    fn led_state_changed(&mut self, _seat: &Seat<Self>, leds: smithay::input::keyboard::LedState) {
        #[cfg(feature = "standalone")]
        self.update_keyboard_leds(leds);
        #[cfg(not(feature = "standalone"))]
        let _ = leds;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client.clone());
        smithay::wayland::selection::primary_selection::set_primary_focus(dh, seat, client);
    }
}

delegate_seat!(Dome);

//
// Wl Data Device
//

impl SelectionHandler for Dome {
    type SelectionUserData = std::sync::Arc<crate::clipboard::Entry>;
    fn new_selection(
        &mut self,
        ty: smithay::wayland::selection::SelectionTarget,
        source: Option<smithay::wayland::selection::SelectionSource>,
        _seat: Seat<Self>,
    ) {
        if ty == smithay::wayland::selection::SelectionTarget::Clipboard {
            self.clipboard.changed(source);
        }
    }
    fn send_selection(
        &mut self,
        _ty: smithay::wayland::selection::SelectionTarget,
        mime: String,
        fd: std::os::fd::OwnedFd,
        _seat: Seat<Self>,
        data: &Self::SelectionUserData,
    ) {
        if let Some((_, bytes)) = data.formats.iter().find(|(m, _)| m == &mime) {
            crate::clipboard::send_data(fd, bytes.clone());
        }
    }
}

impl DataDeviceHandler for Dome {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Dome {}
impl ServerDndGrabHandler for Dome {}

delegate_data_device!(Dome);

//
// Wl Output & Xdg Output
//

impl OutputHandler for Dome {}
delegate_output!(Dome);

impl smithay::wayland::dmabuf::DmabufHandler for Dome {
    fn dmabuf_state(&mut self) -> &mut smithay::wayland::dmabuf::DmabufState {
        &mut self.dmabuf_state
    }
    fn dmabuf_imported(
        &mut self,
        _global: &smithay::wayland::dmabuf::DmabufGlobal,
        dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: smithay::wayland::dmabuf::ImportNotifier,
    ) {
        if self
            .dmabuf_importer
            .as_mut()
            .is_some_and(|import| import(&dmabuf))
        {
            let _ = notifier.successful::<Self>();
        } else {
            notifier.failed();
        }
    }
}
smithay::delegate_dmabuf!(Dome);

impl smithay::wayland::selection::primary_selection::PrimarySelectionHandler for Dome {
    fn primary_selection_state(
        &self,
    ) -> &smithay::wayland::selection::primary_selection::PrimarySelectionState {
        &self.primary_selection_state
    }
}
impl smithay::wayland::selection::wlr_data_control::DataControlHandler for Dome {
    fn data_control_state(
        &self,
    ) -> &smithay::wayland::selection::wlr_data_control::DataControlState {
        &self.data_control_state
    }
}
impl smithay::wayland::selection::ext_data_control::DataControlHandler for Dome {
    fn data_control_state(
        &self,
    ) -> &smithay::wayland::selection::ext_data_control::DataControlState {
        &self.ext_data_control_state
    }
}
smithay::delegate_primary_selection!(Dome);
smithay::delegate_data_control!(Dome);
smithay::delegate_ext_data_control!(Dome);

impl smithay::wayland::fractional_scale::FractionalScaleHandler for Dome {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        self.update_surface_density(&surface);
    }
}
smithay::delegate_fractional_scale!(Dome);
impl Dome {
    pub fn update_surface_density(&self, surface: &WlSurface) {
        use smithay::wayland::{compositor::{with_states, get_parent}, fractional_scale::with_fractional_scale};
        let mut root = surface.clone();
        while let Some(p) = get_parent(&root) { root = p; }
        let origin = surface.client().and_then(|c| c.get_data::<crate::state::ClientState>().map(|s| s.origin));
        let scene = self.scene.lock().unwrap();
        let in_xr = self.id_for(&root).and_then(|id| scene.window(id)).is_some_and(|w| w.xr_visible);
        let scale = if origin == Some(dome::Realm::Xr) || (in_xr && self.xr_bridge.lock().unwrap().visible()) {
            scene.config.xr_render_scale as f64
        } else { 1. };
        with_states(surface, |s| {
            with_fractional_scale(s, |f| f.set_preferred_scale(scale));
            smithay::wayland::compositor::send_surface_state(surface, s, scale.ceil() as i32, smithay::utils::Transform::Normal);
        });
    }
    pub fn update_xr_density(&self) {
        use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};
        let mut surfaces = Vec::new();
        for surface in self.registry.iter().map(|(_,w)| w.toplevel().unwrap().wl_surface())
            .chain(self.spatial_popups.iter().map(|(_,p)| p.wl_surface())) {
            with_surface_tree_downward(surface, (), |_,_,_| TraversalAction::DoChildren(()),
                |s,_,_| surfaces.push(s.clone()), |_,_,_| true);
        }
        // Tree traversal holds Smithay's surface lock. Resolve parentage and
        // update state only after leaving it.
        for surface in surfaces { self.update_surface_density(&surface); }
    }
}
