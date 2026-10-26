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

/// Native VR games keep their ordinary mirror on the desktop, just like X11.
pub fn is_vr_mirror(app_id: &str) -> bool {
    app_id.eq_ignore_ascii_case("vrchat.exe")
}

#[cfg(test)]
mod tests {
    #[test]
    fn vrchat_mirror_is_desktop_only_without_matching_its_launcher() {
        assert!(super::is_vr_mirror("vrchat.exe"));
        assert!(super::is_vr_mirror("VRChat.exe"));
        assert!(!super::is_vr_mirror("vrchat-launcher"));
        assert!(!super::is_vr_mirror("org.domed.FLTerm"));
    }
}
