//! Runtime-paced XR capture and frame callbacks, independent of monitor scanout.
use crate::{desktop::DesktopRenderer, Dome};
use smithay::{backend::renderer::gles::GlesRenderer, output::{Mode, Output, PhysicalProperties, Subpixel},
    utils::Transform, wayland::compositor::{with_surface_tree_downward, TraversalAction}};
use std::time::{Duration, Instant};
#[derive(Default)]
pub struct Refresh {
    pub active: bool,
    output: Option<Output>,
    deadline: Option<Instant>,
    renderer: DesktopRenderer,
}
impl Dome {
    pub fn xr_tick_interval(&self) -> Duration {
        self.xr_bridge.lock().unwrap().frame_interval()
            .unwrap_or(Duration::from_millis(16))
    }
    pub fn tick_xr(&mut self, renderer: &mut GlesRenderer) {
        let interval = self.xr_bridge.lock().unwrap().frame_interval();
        let mut refresh = std::mem::take(&mut self.xr_refresh);
        refresh.active = interval.is_some();
        let now = Instant::now();
        if let Some(interval) = interval {
            if refresh.deadline.is_some_and(|deadline| now + Duration::from_micros(500) < deadline) {
                self.xr_refresh = refresh;
                return;
            }
            refresh.deadline = Some(dome::xr_transport::next_frame_deadline(
                refresh.deadline.unwrap_or(now), now, interval));
            let output = refresh.output.get_or_insert_with(|| {
                let output = Output::new("Dome-XR".into(), PhysicalProperties {
                    size: (0, 0).into(), subpixel: Subpixel::Unknown,
                    make: "DomeD".into(), model: "XR desktop".into(),
                });
                output.create_global::<Dome>(&self.display_handle);
                output
            });
            let mode = Mode { size: (1024, 768).into(),
                refresh: dome::xr_transport::refresh_millihz(interval.as_nanos() as u64).unwrap() };
            if output.current_mode() != Some(mode) {
                let old = output.current_mode();
                output.set_preferred(mode);
                output.change_current_state(Some(mode), Some(Transform::Normal), None, Some((0, 0).into()));
                if let Some(old) = old { output.delete_mode(old); }
            }
            let size = self.primary_output_geometry().size;
            refresh.renderer.prepare(self, (size.w, size.h).into());
            self.export_notification_windows(renderer);
            self.export_xr_surface(renderer);
        } else {
            refresh.deadline = None;
        }
        if let Some(output) = refresh.output.as_ref() {
            let scene = self.scene.lock().unwrap().clone();
            for (id, window) in &self.registry {
                let visible = refresh.active && scene.window(*id).is_some_and(|w| w.xr_visible && !w.minimized);
                if let Some(top) = window.toplevel() {
                    let mut surfaces = Vec::new();
                    with_surface_tree_downward(top.wl_surface(), (), |_, _, _| TraversalAction::DoChildren(()),
                        |s, _, _| surfaces.push(s.clone()), |_, _, _| true);
                    for surface in surfaces {
                        if visible { output.enter(&surface); } else { output.leave(&surface); }
                    }
                }
                if visible {
                    window.send_frame(output, self.start_time.elapsed(), Some(Duration::ZERO), |_, _| Some(output.clone()));
                }
            }
            for (id, popup) in &self.spatial_popups {
                let visible = refresh.active && scene.popup(*id).is_some_and(|p| scene.popup_visible(p, false));
                if visible {
                    output.enter(popup.wl_surface());
                    smithay::desktop::utils::send_frames_surface_tree(popup.wl_surface(), output,
                        self.start_time.elapsed(), Some(Duration::ZERO), |_, _| Some(output.clone()));
                } else { output.leave(popup.wl_surface()); }
            }
            let _ = self.display_handle.flush_clients();
        }
        self.xr_refresh = refresh;
    }
}
