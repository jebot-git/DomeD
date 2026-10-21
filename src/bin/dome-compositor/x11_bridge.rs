//! The session-owned X bridge is a desktop client, independent of XR focus.
use crate::{state::ClientState, Dome};
use smithay::reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource};

impl Dome {
    pub fn is_x11_bridge(&self, surface: &WlSurface) -> bool {
        surface.client().is_some_and(|client| {
            client.get_data::<ClientState>().is_some_and(|state| state.x11_bridge)
        })
    }
}
