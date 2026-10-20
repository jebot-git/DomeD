use crate::{grabs::resize_grab, state::ClientState, Dome};
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_shm,
    reexports::wayland_server::{
        protocol::{wl_buffer, wl_surface::WlSurface},
        Client,
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            get_parent, is_sync_subsurface, CompositorClientState, CompositorHandler,
            CompositorState,
        },
        shm::{ShmHandler, ShmState},
    },
};

use super::xdg_shell;

impl CompositorHandler for Dome {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self
                .registry
                .iter()
                .map(|(_, w)| w)
                .find(|w| w.toplevel().unwrap().wl_surface() == &root)
            {
                window.on_commit();
                // SDL and other fixed-size clients legitimately keep their
                // committed size instead of our initial suggested rectangle.
                // Keep scene/XR coordinates consistent with that actual surface.
                let geometry = window.geometry();
                let fixed = smithay::wayland::compositor::with_states(&root, |s| {
                    let mut cached = s.cached_state.get::<
                        smithay::wayland::shell::xdg::SurfaceCachedState,
                    >();
                    let current = cached.current();
                    (
                        current.min_size.w > 0 && current.min_size.w == current.max_size.w,
                        current.min_size.h > 0 && current.min_size.h == current.max_size.h,
                    )
                });
                if !geometry.is_empty() && (fixed.0 || fixed.1) {
                    if let Some(id) = self.id_for(&root) {
                        let mut scene = self.scene.lock().unwrap();
                        if let Some(w) = scene.windows.iter_mut().find(|w| w.id == id) {
                            if !w.maximized {
                                let before = w.desktop;
                                if fixed.0 { w.desktop.w = geometry.size.w as f32; }
                                if fixed.1 { w.desktop.h = geometry.size.h as f32; }
                                if w.desktop != before { scene.revision += 1; }
                            }
                        }
                    }
                }
            }
        };

        xdg_shell::handle_commit(&mut self.popups, &self.registry, surface);
        resize_grab::handle_commit(&mut self.space, surface);
        self.applied_revision = u64::MAX;
    }
}

impl BufferHandler for Dome {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Dome {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

delegate_compositor!(Dome);
delegate_shm!(Dome);

smithay::delegate_viewporter!(Dome);
