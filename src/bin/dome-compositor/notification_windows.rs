//! Steam draws desktop toasts itself. Import only its exact toast window role;
//! retain the client surface for rendering and input without making it an app.
use smithay::{
    backend::{
        input::ButtonState,
        renderer::{
            damage::OutputDamageTracker,
            element::{
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                Kind,
            },
            gles::{GlesRenderer, GlesTexture},
            utils::with_renderer_surface_state,
            Bind, ExportMem, Offscreen,
        },
    },
    input::pointer::{ButtonEvent, MotionEvent},
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Rectangle, Transform, SERIAL_COUNTER},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};
use std::collections::HashMap;
#[derive(Default)]
pub struct Windows {
    click: Option<PendingClick>,
    pub entries: HashMap<u64, bool>,
    pub previous_focus: HashMap<u64, Option<u64>>,
}
struct PendingClick {
    source: u64,
    phase: u8,
    ready: std::time::Instant,
}
fn steam_toast(app: &str, title: &str) -> bool {
    matches!(
        app.to_ascii_lowercase().as_str(),
        "steam" | "steamwebhelper"
    ) && title
        .strip_prefix("notificationtoasts_")
        .and_then(|s| s.strip_suffix("_desktop"))
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|c| c.is_ascii_digit()))
}
impl crate::Dome {
    pub fn capture_notification_window(&mut self, surface: &WlSurface) -> bool {
        let Some(id) = self.id_for(surface) else {
            return false;
        };
        if self.notification_windows.entries.contains_key(&id) {
            return true;
        }
        let is_notice = with_states(surface, |s| {
            let data = s
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .unwrap()
                .lock()
                .unwrap();
            steam_toast(
                data.app_id.as_deref().unwrap_or(""),
                data.title.as_deref().unwrap_or(""),
            )
        });
        if !is_notice {
            return false;
        }
        // Bound concurrent live imports even if a client never destroys its toasts.
        if self.notification_windows.entries.len() >= 32 {
            return false;
        }
        self.notification_windows.entries.insert(id, true);
        if let Some((_, window)) = self.registry.iter().find(|(i, _)| *i == id) {
            self.space.unmap_elem(window);
            let toplevel = window.toplevel().unwrap();
            toplevel.with_pending_state(|s| {
                s.size = None;
                s.states.unset(smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Activated);
            });
            toplevel.send_pending_configure();
        }
        let mut scene = self.scene.lock().unwrap();
        let focused = scene.focused == Some(id);
        let _ = scene.apply(dome::Action::Close { id });
        if focused {
            if let Some(previous) = self
                .notification_windows
                .previous_focus
                .get(&id)
                .copied()
                .flatten()
                .filter(|i| scene.window(*i).is_some())
            {
                let _ = scene.apply(dome::Action::Focus { id: previous });
            }
        }
        if self.pending_raise == Some(id) {
            self.pending_raise = None;
        }
        self.applied_revision = u64::MAX;
        eprintln!("Dome captured Steam notification surface {id}");
        true
    }
    pub fn export_notification_windows(&mut self, renderer: &mut GlesRenderer) {
        let ids = self
            .notification_windows
            .entries
            .iter()
            .filter_map(|(id, dirty)| dirty.then_some(*id))
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(dirty) = self.notification_windows.entries.get_mut(&id) {
                *dirty = false;
            }
            let Some((_, window)) = self.registry.iter().find(|(i, _)| *i == id) else {
                continue;
            };
            let surface = window.toplevel().unwrap().wl_surface();
            if !with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false) {
                self.notifications.withdraw_surface(id);
                continue;
            }
            let geo = window.geometry();
            let size = geo.size;
            if size.w < 1 || size.h < 1 || size.w > 1024 || size.h > 512 {
                continue;
            }
            let capture = (|| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
                let mut texture: GlesTexture = renderer.create_buffer(
                    smithay::backend::allocator::Fourcc::Argb8888,
                    (size.w, size.h).into(),
                )?;
                let mut target = renderer.bind(&mut texture)?;
                let elements = render_elements_from_surface_tree::<
                    GlesRenderer,
                    WaylandSurfaceRenderElement<GlesRenderer>,
                >(
                    renderer,
                    surface,
                    (-geo.loc.x, -geo.loc.y),
                    1.,
                    1.,
                    Kind::Unspecified,
                );
                let mut damage = OutputDamageTracker::new((size.w, size.h), 1., Transform::Normal);
                damage
                    .render_output(renderer, &mut target, 0, &elements, [0.; 4])?
                    .sync
                    .wait()?;
                let mapping = renderer.copy_framebuffer(
                    &target,
                    Rectangle::from_size((size.w, size.h).into()),
                    smithay::backend::allocator::Fourcc::Abgr8888,
                )?;
                Ok(renderer.map_texture(&mapping)?.to_vec())
            })();
            match capture {
                Ok(pixels) => self
                    .notifications
                    .surface(id, size.w as u32, size.h as u32, &pixels),
                Err(error) => eprintln!("Steam notification capture: {error}"),
            }
        }
    }
    pub fn notification_surface_clicks(&mut self) {
        // Satellite forwards focus over Wayland and raises the X window over
        // XCB. Allow that cross-connection enter to settle before sending a click.
        if let Some(mut click) = self.notification_windows.click.take() {
            if click.ready > std::time::Instant::now() {
                self.notification_windows.click = Some(click);
                return;
            }
            let pointer = self.seat.get_pointer().unwrap();
            let time = self.start_time.elapsed().as_millis() as u32;
            let same_surface = self
                .registry
                .iter()
                .find(|(id, _)| *id == click.source)
                .is_some_and(|(_, window)| {
                    pointer.current_focus().as_ref()
                        == Some(window.toplevel().unwrap().wl_surface())
                });
            if same_surface
                && self
                    .notification_windows
                    .entries
                    .contains_key(&click.source)
                && click.phase < 2
            {
                pointer.button(
                    self,
                    &ButtonEvent {
                        serial: SERIAL_COUNTER.next_serial(),
                        time,
                        button: 0x110,
                        state: if click.phase == 0 {
                            ButtonState::Pressed
                        } else {
                            ButtonState::Released
                        },
                    },
                );
                pointer.frame(self);
                click.phase += 1;
                click.ready = std::time::Instant::now() + std::time::Duration::from_millis(25);
                self.notification_windows.click = Some(click);
            } else {
                if click.phase == 1 {
                    pointer.button(
                        self,
                        &ButtonEvent {
                            serial: SERIAL_COUNTER.next_serial(),
                            time,
                            button: 0x110,
                            state: ButtonState::Released,
                        },
                    );
                    pointer.frame(self);
                }
                let location = pointer.current_location();
                pointer.motion(
                    self,
                    None,
                    &MotionEvent {
                        location,
                        serial: SERIAL_COUNTER.next_serial(),
                        time,
                    },
                );
                pointer.frame(self);
            }
            let _ = self.display_handle.flush_clients();
            return;
        }
        while let Some((id, uv)) = self.notifications.next_surface_click() {
            let Some((_, window)) = self.registry.iter().find(|(i, _)| *i == id) else {
                continue;
            };
            if !self.notification_windows.entries.contains_key(&id) {
                continue;
            }
            let geo = window.geometry();
            let surface = window.toplevel().unwrap().wl_surface().clone();
            let pointer = self.seat.get_pointer().unwrap();
            let location = pointer.current_location();
            let origin = (
                location.x - geo.loc.x as f64 - uv[0] * geo.size.w as f64,
                location.y - geo.loc.y as f64 - uv[1] * geo.size.h as f64,
            )
                .into();
            let time = self.start_time.elapsed().as_millis() as u32;
            pointer.motion(
                self,
                Some((surface, origin)),
                &MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
            let _ = self.display_handle.flush_clients();
            self.notification_windows.click = Some(PendingClick {
                source: id,
                phase: 0,
                ready: std::time::Instant::now() + std::time::Duration::from_millis(25),
            });
            break;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_steam_desktop_toast_role_is_captured() {
        assert!(steam_toast("steam", "notificationtoasts_123_desktop"));
        assert!(steam_toast(
            "steamwebhelper",
            "notificationtoasts_1_desktop"
        ));
        for (app, title) in [
            ("other", "notificationtoasts_1_desktop"),
            ("steam", "Steam"),
            ("steam", "Friends List"),
            ("steam", "notificationtoasts__desktop"),
            ("steam", "notificationtoasts_1_ingame"),
        ] {
            assert!(!steam_toast(app, title));
        }
    }
}
