use smithay::{
    delegate_xdg_shell,
    desktop::{find_popup_root_surface, PopupKind, PopupManager, Window},
    input::{
        pointer::{Focus, GrabStartData as PointerGrabStartData},
        Seat,
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            protocol::{wl_output, wl_seat, wl_surface::WlSurface},
            Resource,
        },
    },
    utils::{Rectangle, Serial},
    wayland::{
        compositor::with_states,
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            XdgToplevelSurfaceData,
        },
    },
};

use crate::{
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab},
    Dome,
};

// Remember only compositor-assigned launch geometry. Some toolkits announce
// their transient parent after the bufferless commit that triggers configure.
// A later parent association may correct that suggestion once, but must not
// overwrite restored geometry or a user's subsequent resize.
#[derive(Default)]
struct XrLaunchSize(std::sync::Mutex<Option<[i32; 2]>>);

impl XdgShellHandler for Dome {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        self.register(window.clone());
        self.sync_scene();
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        self.metadata(surface.wl_surface());
    }
    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        self.metadata(surface.wl_surface());
    }
    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.id_for(surface.wl_surface()) {
            self.notification_windows.previous_focus.remove(&id);
            self.selection_tooltips.remove(&id);
            if self.notification_windows.entries.remove(&id).is_some() {
                self.notifications.withdraw_surface(id);
            } else { self.action(dome::Action::Close { id }); }
        }
        self.registry.retain(|(_, w)| {
            let keep = w.toplevel().unwrap() != &surface;
            if !keep {
                self.space.unmap_elem(w);
            }
            keep
        });
    }
    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.id_for(surface.wl_surface()) {
            let maximized = self
                .scene
                .lock()
                .unwrap()
                .window(id)
                .is_some_and(|w| w.maximized);
            if !maximized {
                self.action(dome::Action::Maximize { id });
            }
        }
    }
    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.id_for(surface.wl_surface()) {
            let maximized = self
                .scene
                .lock()
                .unwrap()
                .window(id)
                .is_some_and(|w| w.maximized);
            if maximized {
                self.action(dome::Action::Maximize { id });
            }
        }
    }
    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<wl_output::WlOutput>) {
        let media = self.id_for(surface.wl_surface()).is_some_and(|id|
            self.scene.lock().unwrap().window(id).is_some_and(|w| w.xr_media() && w.xr_visible));
        if media { self.maximize_request(surface); } else { surface.send_configure(); }
    }
    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        let media = self.id_for(surface.wl_surface()).is_some_and(|id|
            self.scene.lock().unwrap().window(id).is_some_and(|w| w.xr_media() && w.xr_visible));
        if media { self.unmaximize_request(surface); }
    }
    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.id_for(surface.wl_surface()) {
            self.action(dome::Action::Minimize { id });
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.associate_context_menu(&surface);
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let seat = Seat::from_resource(&seat).unwrap();

        let wl_surface = surface.wl_surface();

        if let Some(start_data) = check_grab(&seat, wl_surface, serial) {
            let pointer = seat.get_pointer().unwrap();

            let Some(window) = self
                .space
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == wl_surface)
                .cloned()
            else {
                return;
            };
            let initial_window_location = self.space.element_location(&window).unwrap();

            let grab = MoveSurfaceGrab {
                start_data,
                window,
                initial_window_location,
            };

            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let seat = Seat::from_resource(&seat).unwrap();

        let wl_surface = surface.wl_surface();

        if let Some(start_data) = check_grab(&seat, wl_surface, serial) {
            let pointer = seat.get_pointer().unwrap();

            let Some(window) = self
                .space
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == wl_surface)
                .cloned()
            else {
                return;
            };
            let initial_window_location = self.space.element_location(&window).unwrap();
            let initial_window_size = window.geometry().size;

            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Resizing);
            });

            surface.send_pending_configure();

            let grab = ResizeSurfaceGrab::start(
                start_data,
                window,
                edges.into(),
                Rectangle::new(initial_window_location, initial_window_size),
            );

            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        use smithay::desktop::{PopupKeyboardGrab, PopupPointerGrab};
        let Some(seat) = Seat::<Dome>::from_resource(&seat) else {
            return;
        };
        let popup = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&popup) else {
            return;
        };
        let Some(pointer) = seat.get_pointer() else {
            return;
        };
        let Some(keyboard) = seat.get_keyboard() else {
            return;
        };
        // Initial activation and nested grabs must come from this client's seat.
        let authorized = keyboard
            .current_focus()
            .is_some_and(|surface| surface.id().same_client_as(&root.id()));
        if !authorized {
            return;
        }
        if let Ok(grab) = self.popups.grab_popup(root, popup, &seat, serial) {
            let previous = grab.previous_serial().unwrap_or(serial);
            if keyboard.is_grabbed() && !keyboard.has_grab(previous) {
                return;
            }
            keyboard.set_focus(self, grab.current_grab(), serial);
            keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
            pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
            self.popup_grab = Some(grab);
        }
    }
}

// Xdg Shell
delegate_xdg_shell!(Dome);

fn check_grab(
    seat: &Seat<Dome>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<PointerGrabStartData<Dome>> {
    let pointer = seat.get_pointer()?;

    // Check that this surface has a click grab.
    if !pointer.has_grab(serial) {
        return None;
    }

    let start_data = pointer.grab_start_data()?;

    let (focus, _) = start_data.focus.as_ref()?;
    // If the focus was for a different surface, ignore the request.
    if !focus.id().same_client_as(&surface.id()) {
        return None;
    }

    Some(start_data)
}

impl Dome {
    /// Suggest launch geometry, correcting a delayed transient-parent hint once.
    pub fn configure_initial_xr_size(&mut self, surface: &WlSurface) {
        let Some(id) = self.id_for(surface) else { return; };
        if self.notification_windows.entries.contains_key(&id)
            || self.selection_tooltips.contains_key(&id) || self.is_x11_bridge(surface) { return; }
        let Some(window) = self.registry.iter().find(|(i,_)| *i == id).map(|(_,w)| w) else { return; };
        let (initial, transient) = with_states(surface, |states| {
            let data = states.data_map.get::<XdgToplevelSurfaceData>().unwrap().lock().unwrap();
            (!data.initial_configure_sent, data.parent.is_some())
        });
        if !initial && !transient { return; }
        let launch_size = with_states(surface, |states| {
            states.data_map.insert_if_missing(XrLaunchSize::default);
            *states.data_map.get::<XrLaunchSize>().unwrap().0.lock().unwrap()
        });
        let limits = with_states(surface, |states| {
            let mut cached = states.cached_state.get::<smithay::wayland::shell::xdg::SurfaceCachedState>();
            let state = cached.current();
            ([state.min_size.w, state.min_size.h], [state.max_size.w, state.max_size.h])
        });
        let mut scene = self.scene.lock().unwrap();
        let resolution = scene.config.xr_window_resolution;
        let size = if transient { resolution.initial_popup_size(limits.0, limits.1) }
            else { resolution.initial_size(limits.0, limits.1) };
        let Some(w) = scene.windows.iter_mut().find(|w| w.id == id) else { return; };
        // Restored/explicit geometry wins over the new-window launch default.
        if w.origin != dome::Realm::Xr { return; }
        if initial {
            if w.desktop.w != 720. || w.desktop.h != 460. { return; }
        } else if launch_size != Some([w.desktop.w as i32,w.desktop.h as i32]) {
            return;
        }
        with_states(surface, |states| {
            *states.data_map.get::<XrLaunchSize>().unwrap().0.lock().unwrap() =
                if transient { None } else { Some(size) };
        });
        w.desktop.w = size[0] as f32;
        w.desktop.h = size[1] as f32;
        scene.revision += 1;
        let toplevel = window.toplevel().unwrap();
        toplevel.with_pending_state(|state| state.size = Some((size[0],size[1]).into()));
        if !initial { toplevel.send_pending_configure(); }
    }
}

/// Should be called on `WlSurface::commit`
pub fn handle_commit(popups: &mut PopupManager, registry: &[(u64, Window)], surface: &WlSurface) {
    // Handle toplevel commits.
    if let Some(window) = registry
        .iter()
        .map(|(_, w)| w)
        .find(|w| w.toplevel().unwrap().wl_surface() == surface)
        .cloned()
    {
        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .unwrap()
                .lock()
                .unwrap()
                .initial_configure_sent
        });

        if !initial_configure_sent {
            window.toplevel().unwrap().send_configure();
        }
    }

    // Handle popup commits.
    popups.commit(surface);
    if let Some(popup) = popups.find_popup(surface) {
        match popup {
            PopupKind::Xdg(ref xdg) => {
                if !xdg.is_initial_configure_sent() {
                    // NOTE: This should never fail as the initial configure is always
                    // allowed.
                    xdg.send_configure().expect("initial configure failed");
                }
            }
            PopupKind::InputMethod(ref _input_method) => {}
        }
    }
}

impl Dome {
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self
            .space
            .elements()
            .find(|w| w.toplevel().unwrap().wl_surface() == &root)
        else {
            return;
        };

        let window_geo = self.space.element_geometry(window).unwrap();
        let output_geo = self.output_geometry_at(
            (
                window_geo.loc.x as f64 + window_geo.size.w as f64 / 2.,
                window_geo.loc.y as f64 + window_geo.size.h as f64 / 2.,
            )
                .into(),
        );

        // Children can be created before their parent's first buffer commit.
        // Use the geometry already configured to the client, including pending state.
        let mut parent_offset = smithay::utils::Point::<i32, smithay::utils::Logical>::from((0, 0));
        let mut parent_size = window_geo.size;
        let mut ancestor = popup.get_parent_surface();
        for depth in 0..16 {
            let Some(surface) = ancestor else {
                break;
            };
            let state = with_states(&surface, |states| {
                let data = states
                    .data_map
                    .get::<smithay::wayland::shell::xdg::XdgPopupSurfaceData>()?
                    .lock()
                    .unwrap();
                let geometry = data
                    .server_pending
                    .as_ref()
                    .unwrap_or_else(|| data.current_server_state())
                    .geometry;
                Some((geometry, data.parent.clone()))
            });
            let Some((geometry, next)) = state else {
                break;
            };
            parent_offset += geometry.loc;
            if depth == 0 {
                parent_size = geometry.size;
            }
            ancestor = next;
        }
        let parent = dome::Rect {
            x: (window_geo.loc.x + parent_offset.x) as f32,
            y: (window_geo.loc.y + parent_offset.y) as f32,
            w: parent_size.w as f32,
            h: parent_size.h as f32,
        };
        let work = dome::Rect {
            x: output_geo.loc.x as f32,
            y: (output_geo.loc.y + 36) as f32,
            w: output_geo.size.w as f32,
            h: (output_geo.size.h - 96).max(1) as f32,
        };
        let context = self
            .context_menus
            .iter()
            .find(|m| m.surface == *popup.wl_surface());
        let nested = popup.get_parent_surface().is_some_and(|surface| surface != root);
        popup.with_pending_state(|state| {
            let size = state.positioner.get_geometry().size;
            let placed = if let Some(context) = context {
                dome::context_menu::rect(
                    parent,
                    context.point,
                    [size.w as f32, (size.h as f32).min(work.h)],
                    work,
                )
            } else if self.input_realm == dome::Realm::Xr {
                let positioner = state.positioner;
                let geometry = positioner.get_unconstrained_geometry(Rectangle::new(
                    ((window_geo.loc.x as f32 - parent.x) as i32,
                     (window_geo.loc.y as f32 - parent.y) as i32).into(),
                    window_geo.size,
                ));
                let control = positioner.anchor_rect;
                let requested = positioner.get_geometry();
                // XR content sizes are independent of the physical monitor.
                // Constrain logical rollouts to their owning window, then apply
                // the actual bar bounds to their shared render/input quad.
                let rollout_work = dome::Rect {
                    x: window_geo.loc.x as f32, y: window_geo.loc.y as f32,
                    w: window_geo.size.w as f32, h: window_geo.size.h as f32 };
                // FLTK's generic menus encode their source through the positioner
                // offset and a 1px anchor rather than a full control rectangle.
                let point_anchor = control.size.w <= 1 && control.size.h <= 1;
                dome::spatial_popup::rollout_rect(parent,
                    dome::Rect {
                        x: if point_anchor { requested.loc.x } else { control.loc.x } as f32,
                        y: if point_anchor { requested.loc.y } else { control.loc.y } as f32,
                        w: if point_anchor { size.w } else { control.size.w } as f32,
                        h: control.size.h as f32 },
                    dome::Rect { x: geometry.loc.x as f32, y: geometry.loc.y as f32,
                        w: size.w as f32, h: size.h as f32 }, rollout_work, nested)
            } else {
                // Respect the client's tree/menu anchor, including edge flips for
                // nested submenus. Only an absent anchor uses the cursor fallback.
                let positioner = state.positioner;
                let has_anchor = positioner.anchor_rect.size.w > 0
                    && positioner.anchor_rect.size.h > 0;
                let geometry = positioner.get_unconstrained_geometry(Rectangle::new(
                    ((work.x - parent.x) as i32, (work.y - parent.y) as i32).into(),
                    (work.w as i32, work.h as i32).into(),
                ));
                let cursor = self.seat.get_pointer().unwrap().current_location();
                dome::spatial_popup::desktop_rect(
                    parent,
                    has_anchor.then_some([geometry.loc.x as f32, geometry.loc.y as f32]),
                    [cursor.x as f32, cursor.y as f32],
                    [size.w as f32, size.h as f32],
                    work,
                )
            };
            // Publish DomeD's placement to the client. Moving only the drawn
            // surface leaves GTK/Chromium's submenu navigation in the wrong space.
            state.geometry = Rectangle::new(
                (
                    (placed.x - parent.x).round() as i32,
                    (placed.y - parent.y).round() as i32,
                )
                    .into(),
                size,
            );
        });
    }
}
