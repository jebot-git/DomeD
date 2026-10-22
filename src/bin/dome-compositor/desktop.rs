//! Shared desktop/XR rendering used by nested and direct DRM backends.
use crate::Dome;
use smithay::{
    backend::renderer::{
        damage::OutputDamageTracker,
        element::{
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            solid::{SolidColorBuffer, SolidColorRenderElement},
            surface::WaylandSurfaceRenderElement,
            Kind,
        },
        gles::{GlesRenderer, GlesTarget},
        sync::SyncPoint,
    },
    input::pointer::{CursorImageStatus, CursorImageSurfaceData},
    output::Output,
    utils::{Physical, Size, Transform},
};
smithay::backend::renderer::element::render_elements! {
    DesktopElement<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Preview=SolidColorRenderElement,
    Chrome=MemoryRenderBufferRenderElement<GlesRenderer>,
}
#[derive(Default)]
pub struct DesktopRenderer {
    decoration_buffers: std::collections::HashMap<u64, (String, MemoryRenderBuffer)>,
    shell_buffer: Option<(String, MemoryRenderBuffer)>,
    cursor: Option<MemoryRenderBuffer>,
    snap_buffer: Option<SolidColorBuffer>,
    focus_buffers: std::collections::HashMap<u64, Vec<SolidColorBuffer>>,
}
impl DesktopRenderer {
    pub fn prepare(&mut self, state: &mut Dome, size: Size<i32, Physical>) {
        state.notification_surface_clicks();
        state.update_keyboard();
        state.sync_spatial_popups();
        state.xr_input();
        state.sync_scene();
        state.update_appmenu();

        let key = format!(
            "{:?}:{}:{}:{}:{:?}:{:?}:{}:{}",
            (
                (state.applied_revision,format!("{:?}",state.clock)),
                (
                    (state.appearance_open,state.default_apps_open,state.default_apps_row,state.default_apps_choice,state.default_apps.revision()),
                    state.xr_settings_open,
                    state.xr_settings_row,
                    (
                        state.geometry_error.clone(),
                        format!(
                            "{:?}:{:?}:{}:{:?}",
                            state.keyboard_config,
                            state.keyboard_draft,
                            state.keyboard_field,
                            (state.keyboard_error.clone(), state.ime.snapshot())
                        )
                    )
                ),
                (state.session_confirm, state.shell_selection),
                format!("{:?}:{:?}", state.appearance, state.session.state()),
                state.about,
                state.task_popup,
                (
                    state.task_context,
                    state.task_context_offset,
                    state.task_context_selected
                ),
                state.pins.clone(),
                (
                    (
                        state.tray.state().revision,
                        state.appmenus.state().revision,
                        state.appmenu_path.clone()
                    ),
                    state.notifications.revision(),
                    state.clipboard.revision,
                    state.history_panel,
                    state.history_selected,
                    (state.launch_index, state.launch_navigation.offset, state.launch_navigation.error.clone()),
                    state.audio.state().revision,
                    state.audio_open,
                    state.audio_selected
                ),
                state.task_previews.revision
            ),
            state.launcher,
            state.desktop_manager,
            state.launch_query,
            size,
            state.menu,
            state.menu_hover,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                / if state.clock.settings.seconds { 1 } else { 60 }
        );
        if self
            .shell_buffer
            .as_ref()
            .is_none_or(|(old, _)| old != &key)
        {
            let realm = state.input_realm;
            state.input_realm = dome::Realm::Xr;
            let xr_pixels = state.shell_pixels(1024, 768);
            state.export_xr_shell(&xr_pixels, 1024, 768);
            state.input_realm = dome::Realm::Desktop;
            let pixels = state.shell_pixels(size.w, size.h);
            state.input_realm = realm;
            self.shell_buffer = Some((
                key,
                MemoryRenderBuffer::from_slice(
                    &pixels,
                    smithay::backend::allocator::Fourcc::Argb8888,
                    (size.w, size.h),
                    1,
                    Transform::Normal,
                    None,
                ),
            ));
        }
        {
            let mut bridge = state.xr_bridge.lock().unwrap();
            bridge.accent = dome::visual_feedback::accent(&state.appearance.accent);
            for (id, f) in &mut bridge.frames {
                if dome::xr_transport::is_shell(*id) {
                    f.stamp = std::time::Instant::now();
                }
            }
        }
    }
    pub fn render(
        &mut self,
        state: &mut Dome,
        renderer: &mut GlesRenderer,
        framebuffer: &mut GlesTarget<'_>,
        damage_tracker: &mut OutputDamageTracker,
        age: usize,
        software_cursor: bool,
    ) -> Result<SyncPoint, Box<dyn std::error::Error>> {
        self.render_at(
            state,
            renderer,
            framebuffer,
            damage_tracker,
            age,
            software_cursor,
            (0, 0).into(),
        )
    }
    pub fn render_at(
        &mut self,
        state: &mut Dome,
        renderer: &mut GlesRenderer,
        framebuffer: &mut GlesTarget<'_>,
        damage_tracker: &mut OutputDamageTracker,
        age: usize,
        software_cursor: bool,
        origin: smithay::utils::Point<i32, smithay::utils::Logical>,
    ) -> Result<SyncPoint, Box<dyn std::error::Error>> {
        state.export_notification_windows(renderer);
        state.export_xr_surface(renderer);
        let shell = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (origin.x as f64, origin.y as f64),
            &self.shell_buffer.as_ref().unwrap().1,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .unwrap();
        let scene = state.scene.lock().unwrap().clone();
        self.focus_buffers.retain(|id, _| scene.focused == Some(*id) && scene.window(*id).is_some());
        self.decoration_buffers
            .retain(|id, _| scene.window(*id).is_some());
        let mut elements: Vec<DesktopElement> = vec![shell.into()];
        if state.seat.get_keyboard().unwrap().modifier_state().shift {
            if let Some((_, r)) = state.snap_preview {
                let size = (r.w.round() as i32, r.h.round() as i32);
                let color = [0.08, 0.22, 0.4, 0.45];
                let buffer = self
                    .snap_buffer
                    .get_or_insert_with(|| SolidColorBuffer::new(size, color));
                buffer.update(size, color);
                elements.push(
                    SolidColorRenderElement::from_buffer(
                        buffer,
                        (r.x.round() as i32, r.y.round() as i32),
                        1.,
                        1.,
                        Kind::Unspecified,
                    )
                    .into(),
                );
            }
        }
        for (id, rect) in state.desktop_popup_rects(&scene).into_iter().rev() {
            if scene
                .popup(id)
                .is_some_and(|p| p.viewport.is_some_and(|v| v.band() > 0.))
            {
                if let Some(frame) = state.xr_bridge.lock().unwrap().frames.get(&id).cloned() {
                    let buffer = MemoryRenderBuffer::from_slice(
                        &frame.rgba,
                        smithay::backend::allocator::Fourcc::Abgr8888,
                        (frame.width as i32, frame.height as i32),
                        1,
                        Transform::Normal,
                        None,
                    );
                    elements.push(
                        MemoryRenderBufferRenderElement::from_buffer(
                            renderer,
                            (rect.x as f64, rect.y as f64),
                            &buffer,
                            None,
                            None,
                            None,
                            Kind::Unspecified,
                        )
                        .unwrap()
                        .into(),
                    );
                }
                continue;
            }
            if let Some((surface, geo)) = state.popup_surface(id) {
                elements.extend(smithay::backend::renderer::element::surface::render_elements_from_surface_tree::<GlesRenderer, DesktopElement>(
                                    renderer, &surface, (rect.x.round() as i32 - geo.loc.x, rect.y.round() as i32 - geo.loc.y), 1.0, 1., Kind::Unspecified));
            }
        }
        for window in state.space.elements().rev() {
            let Some(id) = state.id_for(window.toplevel().unwrap().wl_surface()) else {
                continue;
            };
            let Some(w) = scene.window(id) else {
                continue;
            };
            if scene.popup(id).is_some() {
                continue;
            }
            let Some(pos) = state.space.element_location(window) else {
                continue;
            };
            let geo = window.geometry();
            if scene.focused == Some(id) && geo.size.w >= 4 && geo.size.h >= 4 {
                let color = dome::visual_feedback::accent(&state.appearance.accent);
                let buffers = self.focus_buffers.entry(id).or_default();
                for (inset, alpha) in [(0, 0.60f32), (1, 0.16)] {
                    let w = geo.size.w - 2 * inset;
                    let h = geo.size.h - 2 * inset;
                    for (edge, (x, y, width, height)) in [(inset,inset,w,1),(inset,inset+h-1,w,1),(inset,inset+1,1,h-2),(inset+w-1,inset+1,1,h-2)].into_iter().enumerate() {
                        if width <= 0 || height <= 0 { continue; }
                        let index = inset as usize * 4 + edge;
                        let rgba = [color[0] as f32 / 255. * alpha, color[1] as f32 / 255. * alpha, color[2] as f32 / 255. * alpha, alpha];
                        while buffers.len() <= index { buffers.push(SolidColorBuffer::new((1,1), rgba)); }
                        buffers[index].update((width,height), rgba);
                        elements.push(SolidColorRenderElement::from_buffer(&buffers[index], (pos.x+x,pos.y+y), 1., 1., Kind::Unspecified).into());
                    }
                }
            }
            elements.extend(
                smithay::backend::renderer::element::surface::render_elements_from_surface_tree::<
                    GlesRenderer,
                    DesktopElement,
                >(
                    renderer,
                    window.toplevel().unwrap().wl_surface(),
                    (pos.x - geo.loc.x, pos.y - geo.loc.y),
                    1.0,
                    1.,
                    Kind::Unspecified,
                ),
            );
            if w.maximized {
                continue;
            }
            let dw = geo.size.w + 8;
            let dh = geo.size.h + 34;
            if dw < 8 || dh < 34 || dw > 8192 || dh > 8192 {
                continue;
            }
            let zoomed = state.workbench_zoom.get(&id).is_some_and(|s| s.zoomed);
            let pressed = state
                .chrome_button
                .filter(|(owner, _, inside)| *owner == id && *inside)
                .map_or(0, |(_, hit, _)| hit);
            let key = format!(
                "{dw}:{dh}:{}:{}:{}:{}:{}:{}:{}:{zoomed}:{pressed}",
                w.title,
                scene.focused == Some(id),
                state.appearance.light,
                state.appearance.accent,
                state.appearance.opacity,
                state.appearance.font,
                state.appearance.font_size
            );
            if self
                .decoration_buffers
                .get(&id)
                .is_none_or(|(old, _)| old != &key)
            {
                let bytes = crate::qt::paint(
                    serde_json::json!({"kind":"frame","width":dw,"height":dh,"title":w.title,"active":scene.focused==Some(id),"appearance":state.appearance,"zoomed":zoomed,"pressed":pressed}),
                );
                self.decoration_buffers.insert(
                    id,
                    (
                        key,
                        MemoryRenderBuffer::from_slice(
                            &bytes,
                            smithay::backend::allocator::Fourcc::Argb8888,
                            (dw, dh),
                            1,
                            Transform::Normal,
                            None,
                        ),
                    ),
                );
            }
            let deco = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                ((pos.x - 4) as f64, (pos.y - 30) as f64),
                &self.decoration_buffers[&id].1,
                None,
                None,
                None,
                Kind::Unspecified,
            )
            .unwrap();
            elements.push(deco.into());
        }
        let capture_output = state.space.outputs().find(|o|
            state.space.output_geometry(o).is_some_and(|g| g.loc == origin)).cloned();
        let background = [state.appearance.background[0], state.appearance.background[1], state.appearance.background[2], 1.];
        if let Some(output) = &capture_output {
            crate::screencast::render(renderer, output, origin, &elements, 1, background);
        }
        if !state.input_xr_active {
            if let Some(age) = state.cursor_finder.elapsed(dome::Realm::Desktop, std::time::Instant::now()) {
                let rgba = dome::visual_feedback::finder_pixels(age, dome::visual_feedback::accent(&state.appearance.accent));
                let buffer = MemoryRenderBuffer::from_slice(&rgba, smithay::backend::allocator::Fourcc::Abgr8888, (128,128), 1, Transform::Normal, None);
                let p = state.seat.get_pointer().unwrap().current_location();
                if let Ok(effect) = MemoryRenderBufferRenderElement::from_buffer(renderer, (p.x-64.,p.y-64.), &buffer, None, None, None, Kind::Unspecified) {
                    elements.insert(0, effect.into());
                }
            }
        }
        let mut cursors = Vec::new();
        if !state.input_xr_active { self.cursor_elements(state, renderer, &mut cursors); }
        let cursor_count = cursors.len();
        cursors.extend(elements);
        elements = cursors;
        if let Some(output) = &capture_output {
            crate::screencast::render(renderer, output, origin, &elements, 2, background);
        }
        if !software_cursor { elements.drain(..cursor_count); }
        let elements: Vec<_> = elements
            .into_iter()
            .map(|e| {
                smithay::backend::renderer::element::utils::RelocateRenderElement::from_element(
                    e,
                    (-origin.x, -origin.y),
                    smithay::backend::renderer::element::utils::Relocate::Relative,
                )
            })
            .collect();
        damage_tracker
            .render_output(
                renderer,
                framebuffer,
                age,
                &elements,
                [
                    state.appearance.background[0],
                    state.appearance.background[1],
                    state.appearance.background[2],
                    1.,
                ],
            )
            .map(|r| r.sync)
            .map_err(|e| format!("Desktop rendering: {e}").into())
    }
    fn cursor_elements(
        &mut self,
        state: &Dome,
        renderer: &mut GlesRenderer,
        elements: &mut Vec<DesktopElement>,
    ) {
        let location = state.seat.get_pointer().unwrap().current_location();
        match &state.cursor_status {
            CursorImageStatus::Hidden => (),
            CursorImageStatus::Surface(surface) => {
                let hotspot = smithay::wayland::compositor::with_states(surface, |states| {
                    states
                        .data_map
                        .get::<CursorImageSurfaceData>()
                        .map(|attrs| attrs.lock().unwrap().hotspot)
                        .unwrap_or_default()
                });
                elements.extend(smithay::backend::renderer::element::surface::render_elements_from_surface_tree::<GlesRenderer,DesktopElement>(
                    renderer,surface,(location.x.round() as i32-hotspot.x,location.y.round() as i32-hotspot.y),1.,1.,Kind::Cursor));
            }
            CursorImageStatus::Named(_) => {
                let buffer = self.cursor.get_or_insert_with(|| {
                    let mut pixels = vec![0u8; 24 * 32 * 4];
                    for y in 0..27usize {
                        for x in 0..18usize {
                            if x <= y * 2 / 3 && (x < 5 || y < 19) {
                                let edge = x == 0 || x == y * 2 / 3 || y == 26;
                                let color = if edge {
                                    [20, 20, 20, 255]
                                } else {
                                    [245, 245, 245, 255]
                                };
                                pixels[(y * 24 + x) * 4..(y * 24 + x) * 4 + 4]
                                    .copy_from_slice(&color);
                            }
                        }
                    }
                    MemoryRenderBuffer::from_slice(
                        &pixels,
                        smithay::backend::allocator::Fourcc::Argb8888,
                        (24, 32),
                        1,
                        Transform::Normal,
                        None,
                    )
                });
                if let Ok(cursor) = MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    (location.x, location.y),
                    buffer,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    elements.push(cursor.into());
                }
            }
        }
    }
}
pub fn finish_frame(state: &mut Dome, output: &Output) {
    state
        .registry
        .iter()
        .map(|(_, w)| w)
        .filter(|w| {
            let outputs = state.space.outputs_for_element(w);
            outputs.contains(output)
                || (outputs.is_empty() && state.space.outputs().next() == Some(output))
        })
        .for_each(|window| {
            window.send_frame(
                output,
                state.start_time.elapsed(),
                Some(std::time::Duration::ZERO),
                |_, _| Some(output.clone()),
            )
        });
    // Closing removes a window from the scene/registry before its client has
    // dispatched xdg_toplevel.close. An in-flight EGL swap can still commit a
    // fresh frame callback afterward. Keep completing those callbacks until the
    // client destroys the toplevel, or it can never dispatch close/hide-to-tray.
    if state.space.outputs().next() == Some(output) {
        for toplevel in state.xdg_shell_state.toplevel_surfaces() {
            if !state.registry.iter().any(|(_, window)| {
                window.toplevel().is_some_and(|known| known == toplevel)
            }) {
                smithay::desktop::utils::send_frames_surface_tree(
                    toplevel.wl_surface(),
                    output,
                    state.start_time.elapsed(),
                    Some(std::time::Duration::ZERO),
                    |_, _| Some(output.clone()),
                );
            }
        }
    }
    for popup in &state.ime_popups {
        smithay::desktop::utils::send_frames_surface_tree(
            popup.wl_surface(),
            output,
            state.start_time.elapsed(),
            Some(std::time::Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }
    state.space.refresh();
    state.popups.cleanup();
    let _ = state.display_handle.flush_clients();
}
