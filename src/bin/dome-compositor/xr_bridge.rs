use crate::Dome;
use dome::xr_input::{Packet, SeatEvent};
use smithay::{
    backend::input::ButtonState,
    backend::{
        allocator::Fourcc,
        renderer::{
            damage::OutputDamageTracker,
            element::{
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                Kind,
            },
            gles::{GlesRenderer, GlesTexture},
            Bind, ExportMem, Offscreen,
        },
    },
    input::pointer::{AxisFrame, ButtonEvent, MotionEvent},
    utils::{Rectangle, Transform, SERIAL_COUNTER},
};
impl Dome {
    pub fn export_xr_shell(&self, pixels: &[u8], width: i32, height: i32) {
        use dome::xr_transport::{ShellLayout, Surface, NOTIFICATIONS, TASK_BAR, TOP_BAR, TOP_PANEL, BOTTOM_PANEL};
        let mut top = 36;
        let mut bottom = 60;
        if let Some(menu) = self.menu {
            if !self.panel_menu(menu) {
                top = (44 + self.menu_viewport(self.menu_items(menu).len(), 0, self.menu_hover, height).height.ceil() as i32).min(height);
            }
        }
        for panel in self.appmenu_panels(width, height) {
            top = top.max((panel.y + panel.viewport.height.ceil() as i32 + 8).min(height));
        }
        let tray = self.tray.state();
        if tray.menu.is_some() {
            top = top.max((44 + self.tray_menu_viewport(height).height.ceil() as i32).min(height));
        }
        if self.launcher {
            top = 456.min(height);
        }
        if self.shell_modal_open() {
            top = height;
        }
        if self.desktop_manager {
            bottom = 390.min(height);
        }
        if let Some(index) = self.task_popup {
            bottom = bottom.max((height as f64 - self.popup_rect(index, width, height).1) as i32);
        }
        let toasts = if self.shell_modal_open() {
            vec![]
        } else {
            self.notifications.layout(width, height)
        };
        let notification_rect = toasts.first().map(|first| {
            [
                first.x as u32,
                first.y as u32,
                first.w as u32,
                (toasts.last().unwrap().y + toasts.last().unwrap().h - first.y) as u32,
            ]
        });
        let notification_pixels = crate::qt::paint(
            serde_json::json!({"kind":"notifications","width":width,"height":height,"appearance":self.appearance,"notifications":toasts}),
        );
        let mut b = self.xr_bridge.lock().unwrap();
        if notification_rect.is_none() { b.frames.remove(&NOTIFICATIONS); }
        if let Some([x, y, w, h]) = notification_rect {
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for row in y..y + h {
                for pixel in notification_pixels[((row * width as u32 + x) * 4) as usize
                    ..((row * width as u32 + x + w) * 4) as usize]
                    .chunks_exact(4)
                {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
            }
            b.update_surface(
                NOTIFICATIONS,
                Surface {
                    width: w,
                    height: h,
                    rgba: std::sync::Arc::new(rgba),
                    scale: 1.,
                    stamp: std::time::Instant::now(),
                },
            );
        }
        let mut pullout = if top > 36 || bottom > 60 {
            b.shell.pullout.clone().or_else(|| Some(dome::xr_transport::ShellPullout {
                top_attached: false,
                bottom_attached: false,
                anchor: b.session.head.upright(),
                viewport: b.session.mouse_viewport.unwrap_or([-0.65, 0.65, -0.6, 0.6]),
            }))
        } else { None };
        if let Some(p) = &mut pullout {
            // StatusNotifier and input-method menus originate in the upper tray.
            // Permanent audio/history widgets keep their existing placement.
            p.top_attached=(self.menu.is_some() && self.menu_has_ui_origin) || tray.menu.is_some();
            p.bottom_attached=self.task_popup.is_some();
        }
        b.shell = ShellLayout {
            width: width as u32,
            height: height as u32,
            top: top as u32,
            bottom: bottom as u32,
            notifications: notification_rect,
            pullout,
        };
        for id in [TOP_BAR, TASK_BAR, TOP_PANEL, BOTTOM_PANEL] {
            let Some([_, start, _, rows]) = b.shell.surface_rect(id) else {
                b.frames.remove(&id);
                continue;
            };
            let (start, rows) = (start as i32, rows as i32);
            let mut rgba = Vec::with_capacity((width * rows * 4) as usize);
            for pixel in pixels[(start * width * 4) as usize..((start + rows) * width * 4) as usize]
                .chunks_exact(4)
            {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
            b.update_surface(
                id,
                Surface {
                    width: width as u32,
                    height: rows as u32,
                    rgba: std::sync::Arc::new(rgba),
                    scale: 1.,
                    stamp: std::time::Instant::now(),
                },
            );
        }
    }
    pub fn xr_input(&mut self) {
        self.clipboard.poll(&self.seat);
        if !self.native_session_active {
            self.xr_task_leave.update(None, false, self.start_time.elapsed());
            self.xr_bridge.lock().unwrap().packets.clear();
            return;
        }
        let presentation = self.scene.lock().unwrap().presentation;
        self.xr_animation = self
            .xr_animator
            .update(self.start_time.elapsed().as_secs_f64(), presentation);
        if presentation != self.xr_presentation {
            self.cancel_super_mmb();
            self.xr_presentation = presentation;
            if self.input_xr_active {
                self.ignored_buttons.extend(self.physical_buttons.drain());
            }
            {
                let mut b = self.xr_bridge.lock().unwrap();
                b.packets.clear();
                b.cursor = None;
                b.mouse_buttons.clear();
            }
            self.dispatch_xr(Packet {
                events: vec![SeatEvent::Reset],
                actions: vec![],
            });
            self.menu = None;
            self.cancel_quick_drag();
            self.launcher = false;
            self.task_popup = None;
            self.desktop_manager = false;
            self.tray.close();
        }
        let visible = self.xr_bridge.lock().unwrap().visible();
        if visible {
            let head = self.xr_bridge.lock().unwrap().session.head;
            self.scene.lock().unwrap().view_pose = head;
        }
        if self.xr_was_visible && !visible {
            let geo = self.primary_output_geometry();
            let promoted: Vec<_> = self
                .scene
                .lock()
                .unwrap()
                .windows
                .iter()
                .filter(|w| !w.desktop_visible)
                .map(|w| w.id)
                .collect();
            let mut scene = self.scene.lock().unwrap();
            scene.desktop_fallback(geo.size.w as f32, geo.size.h as f32);
            for w in &mut scene.windows {
                if promoted.contains(&w.id) {
                    w.desktop.x += geo.loc.x as f32;
                    w.desktop.y += geo.loc.y as f32;
                }
            }
            drop(scene);
            {
                let mut b = self.xr_bridge.lock().unwrap();
                b.mouse_buttons.clear();
                b.last_mouse = None;
                b.mouse_angles = None;
                b.cursor = None;
                b.packets.clear();
            }
            self.mouse_angles = None;
            self.input_realm = dome::Realm::Desktop;
            self.dispatch_xr(Packet {
                events: vec![SeatEvent::Reset],
                actions: vec![],
            });
        }
        self.xr_was_visible = visible;
        self.sync_input_route();
        let packets = {
            let mut b = self.xr_bridge.lock().unwrap();
            if !b.mouse_owned()
                && b.last_input
                    .is_some_and(|t| t.elapsed().as_secs_f32() > 0.25)
            {
                b.last_input = None;
                b.packets.clear();
                b.packets.push_back(Packet {
                    events: vec![SeatEvent::Reset],
                    actions: vec![],
                });
            }
            b.packets.drain(..).collect::<Vec<_>>()
        };
        for p in packets {
            self.dispatch_xr(p);
        }
        // Head motion changes the projected target even without a new mouse
        // delta. Re-hit without extending the physical mouse ownership lease.
        let refresh_mouse = {
            let b = self.xr_bridge.lock().unwrap();
            visible && self.input_xr_active && b.mouse_owned() && b.mouse_angles.is_some()
        };
        if refresh_mouse {
            self.xr_mouse_motion(0., 0.);
        }
        self.xr_task_popup_tick();
    }
    pub fn dispatch_xr(&mut self, mut p: Packet) {
        if self.session.locked() { return; }
        // Late renderer packets must not move or refocus the desktop-owned seat.
        let desktop_owned =
            !self.input_xr_active && self.xr_bridge.lock().unwrap().last_session.is_some();
        if desktop_owned {
            p.actions.clear();
            p.events.retain(|event| matches!(event, SeatEvent::Reset));
            if p.events.is_empty() {
                return;
            }
        }
        let realm = self.input_realm;
        let mut shell_interaction = false;
        self.input_realm = dome::Realm::Xr;
        for a in p.actions {
            // Renderer packets may still reference a window just sent to the desktop.
            if let dome::Action::Focus { id } = &a {
                if !self.scene.lock().unwrap().surface_visible_in(*id, dome::Realm::Xr) {
                    continue;
                }
            }
            let presentation = self.scene.lock().unwrap().presentation;
            if presentation.desktop && self.xr_animation.opacity >= 1. && !self.shell_modal_open() {
                self.action(a);
            }
        }
        self.sync_scene();
        for e in p.events {
            let e = if self.shell_modal_open()
                && matches!(&e, SeatEvent::Motion { id, .. } if !dome::xr_transport::is_shell(*id))
            {
                SeatEvent::Leave
            } else { e };
            let pointer = self.seat.get_pointer().unwrap();
            let time = self.start_time.elapsed().as_millis() as u32;
            match e {
                SeatEvent::Motion { id, uv } => {
                    if !dome::xr_transport::is_shell(id) {
                        self.menu_edge_hover = false;
                        self.menu_pointer = None;
                    }
                    if !dome::xr_transport::is_shell(id)
                        && !self.scene.lock().unwrap().surface_visible_in(id, dome::Realm::Xr)
                    {
                        self.xr_target = None;
                        self.xr_bridge.lock().unwrap().cursor = None;
                        continue;
                    }
                    self.context_scroll_hover = None;
                    if !self
                        .xr_animation
                        .interactive(self.scene.lock().unwrap().presentation, id)
                    {
                        continue;
                    }
                    self.xr_bridge.lock().unwrap().cursor =
                        Some(dome::xr_transport::Cursor { id, uv });
                    if dome::xr_transport::is_shell(id) {
                        shell_interaction = true;
                        let layout = self.xr_bridge.lock().unwrap().shell.clone();
                        let Some([rx, ry, rw, rh]) = layout.surface_rect(id) else { continue; };
                        let x = rx as f64 + uv[0] as f64 * rw as f64;
                        let y = ry as f64 + uv[1] as f64 * rh as f64;
                        self.xr_target = Some(id);
                        self.xr_shell_pos = Some([x, y]);
                        self.launcher_motion(x,y);
                        self.audio_motion(x, layout.width as i32, layout.height as i32);
                        self.xr_task_motion(x, y, layout.width as i32, layout.height as i32);
                        self.appmenu_motion(x, y, layout.width as i32, layout.height as i32);

                        pointer.motion(
                            self,
                            None,
                            &MotionEvent {
                                location: (x, y).into(),
                                serial: SERIAL_COUNTER.next_serial(),
                                time,
                            },
                        );
                        pointer.frame(self);
                        continue;
                    }
                    self.xr_shell_pos = None;
                    self.context_scroll_hover =
                        self.context_control_at(id, uv).map(|dir| (id, dir));
                    if self.context_scroll_hover.is_some() {
                        self.xr_target = Some(id);
                        continue;
                    }

                    let active = {
                        let scene = self.scene.lock().unwrap();
                        scene.surface_visible_in(id, dome::Realm::Xr)
                    };
                    if active {
                        if let Some((surface, geo)) = self.popup_surface(id) {
                            let scene = self.scene.lock().unwrap();
                            let pos = self
                                .desktop_popup_rects(&scene)
                                .iter()
                                .find(|(i, _)| *i == id)
                                .map(|(_, r)| (r.x as f64, r.y as f64))
                                .or_else(|| {
                                    scene
                                        .window(id)
                                        .map(|w| (w.desktop.x as f64, w.desktop.y as f64))
                                })
                                .unwrap_or((0., 0.));
                            drop(scene);
                            let origin =
                                (pos.0 - geo.loc.x as f64, pos.1 - geo.loc.y as f64).into();
                            let y = self
                                .scene
                                .lock()
                                .unwrap()
                                .popup(id)
                                .and_then(|p| p.viewport)
                                .map_or(uv[1] * geo.size.h as f32, |v| {
                                    v.content_y(uv[1] * v.height)
                                });
                            let location =
                                (pos.0 + uv[0] as f64 * geo.size.w as f64, pos.1 + y as f64).into();
                            self.xr_target = Some(id);
                            pointer.motion(
                                self,
                                Some((surface, origin)),
                                &MotionEvent {
                                    location,
                                    serial: SERIAL_COUNTER.next_serial(),
                                    time,
                                },
                            );
                        }
                    }
                }
                SeatEvent::Button { button, pressed } => {
                    if !pressed && button==0x110 && self.launch_navigation.quick_drag.is_some(){
                        let [x,y]=self.xr_shell_pos.unwrap_or([f64::NAN,f64::NAN]);
                        self.launcher_button(x,y,button,false);
                    }

                    if pressed && !self.xr_target.is_some_and(|id|self.selection_tooltips.contains_key(&id)) {
                        self.dismiss_selection_tooltips();
                    }
                    if !pressed && button == 0x110 {
                        self.audio_drag = None;
                    }
                    if !pressed && self.xr_shell_buttons.remove(&button) {
                        self.xr_buttons.remove(&button);
                        continue;
                    }
                    if pressed && self.shell_modal_open() && self.xr_shell_pos.is_none() {
                        self.xr_shell_buttons.insert(button);
                        self.xr_buttons.insert(button);
                        continue;
                    }
                    if !pressed && self.context_scroll_buttons.remove(&button) {
                        continue;
                    }
                    if pressed && button == 0x110 {
                        if let Some((id, dir)) = self.context_scroll_hover {
                            self.context_scroll_buttons.insert(button);
                            self.scroll_context(id, dir as f32 * 96.);
                            continue;
                        }
                    }
                    if pressed && self.xr_target.is_none() {
                        // FLTK popups do not request an xdg_popup grab. Empty-space
                        // clicks must still dismiss them; ray leave alone must not.
                        if self.spatial_popups.iter().any(|(_, popup)|
                            matches!(popup, smithay::desktop::PopupKind::Xdg(_))) {
                            self.dismiss_wayland_popups();
                        }
                        if !pointer.is_grabbed() { continue; }
                    }
                    if pressed
                        && !pointer.is_grabbed()
                        && self.xr_target.is_none_or(|id| {
                            !self
                                .xr_animation
                                .interactive(self.scene.lock().unwrap().presentation, id)
                        })
                    {
                        continue;
                    }
                    if let Some([x, y]) = self.xr_shell_pos.filter(|_| pressed) {
                        if pressed {
                            self.xr_buttons.insert(button);
                            self.xr_shell_buttons.insert(button);
                            if self.xr_target == Some(dome::xr_transport::NOTIFICATIONS)
                                && button != 0x110
                            {
                                continue;
                            }
                            let layout = self.xr_bridge.lock().unwrap().shell.clone();
                            let tray = self.visible_tray(layout.width as i32);
                            let tx = layout.width as f64 - self.clock_reserve() as f64 - tray.items.len() as f64 * 28.;
                            if self.launcher {
                                self.launcher_button(x,y,button,true);
                            } else if self.shell_modal_open() {
                                self.shell_click(x, y);
                            } else if y < 36.
                                && x >= tx
                                && x < layout.width as f64 - self.clock_reserve() as f64
                                && tray.menu.is_none()
                            {
                                self.activate_tray(
                                    ((x - tx) / 28.) as usize,
                                    x as i32,
                                    y as i32,
                                    button == 0x111,
                                );
                            } else if y >= (layout.height as f64 - 60.)
                                && x >= 110.
                                && x < layout.width as f64 - 250.
                            {
                                self.activate_task(((x - 110.) / 130.) as usize, button);
                            } else {
                                self.shell_click(x, y);
                            }
                        }
                        continue;
                    }

                    if !pressed && !self.xr_buttons.contains(&button) { continue; }

                    if pressed {
                        self.xr_buttons.insert(button);
                    } else {
                        self.xr_buttons.remove(&button);
                    }
                    self.context_button_origin(button, pressed);
                    pointer.button(
                        self,
                        &ButtonEvent {
                            button,
                            state: if pressed {
                                ButtonState::Pressed
                            } else {
                                ButtonState::Released
                            },
                            serial: SERIAL_COUNTER.next_serial(),
                            time,
                        },
                    );
                }
                SeatEvent::Scroll {
                    horizontal,
                    vertical,
                } => {
                    if let Some([x, y]) = self.xr_shell_pos {
                        let size = self.xr_bridge.lock().unwrap().shell.clone();
                        if self.menu_scroll(x, y, vertical * 3., size.width as i32, size.height as i32) { continue; }
                        if self.audio_scroll(x, y, vertical, size.width as i32, size.height as i32)
                        {
                            continue;
                        }
                    }
                    if let Some([x,y])=self.xr_shell_pos {if self.launcher_scroll(x,y,vertical){continue;}}
                    if self.xr_shell_pos.is_some() && self.history_scroll(vertical) {
                        continue;
                    }
                    if self.xr_shell_pos.is_some() && self.xr_settings_scroll(vertical) {
                        continue;
                    }
                    if self.shell_modal_open() { continue; }
                    if let Some(id) = self.xr_target {
                        if self.scroll_context(id, vertical as f32 * 3.) {
                            continue;
                        }
                    }
                    if self.xr_target.is_none_or(|id| {
                        !self
                            .xr_animation
                            .interactive(self.scene.lock().unwrap().presentation, id)
                    }) {
                        continue;
                    }
                    if !self.task_scroll(vertical)
                        && self.xr_shell_pos.is_none()
                        && self.xr_target.is_some()
                    {
                        pointer.axis(
                            self,
                            AxisFrame::new(time)
                                .source(smithay::backend::input::AxisSource::Wheel)
                                .value(smithay::backend::input::Axis::Horizontal, horizontal)
                                .value(smithay::backend::input::Axis::Vertical, vertical),
                        );
                    }
                }
                SeatEvent::Leave => {
                    self.cancel_quick_drag();
                    self.audio_drag = None;
                    self.context_scroll_hover = None;
                    self.menu_edge_hover = false;
                    self.menu_pointer = None;
                    self.xr_target = None;
                    self.xr_shell_pos = None;
                    self.xr_bridge.lock().unwrap().cursor = None;
                    pointer.motion(
                        self,
                        None,
                        &MotionEvent {
                            location: pointer.current_location(),
                            serial: SERIAL_COUNTER.next_serial(),
                            time,
                        },
                    );
                }
                SeatEvent::Reset => {
                    self.cancel_quick_drag();
                    self.audio_drag = None;
                    self.context_scroll_hover = None;
                    self.menu_edge_hover = false;
                    self.menu_pointer = None;
                    self.xr_shell_pos = None;
                    for button in self.xr_buttons.drain().collect::<Vec<_>>() {
                        if self.xr_shell_buttons.remove(&button) { continue; }
                        pointer.button(
                            self,
                            &ButtonEvent {
                                button,
                                state: ButtonState::Released,
                                serial: SERIAL_COUNTER.next_serial(),
                                time,
                            },
                        );
                    }
                    self.xr_shell_buttons.clear();
                    self.xr_target = None;
                }
            }
            pointer.frame(self);
        }
        self.input_realm = if shell_interaction
            && (self.launcher || self.menu.is_some() || self.desktop_manager)
        {
            dome::Realm::Xr
        } else {
            realm
        };
    }
    /// Physical devices retain ownership until input leaves the XR realm.
    pub fn mouse_takeover(&mut self) -> bool {
        let (visible, owned) = {
            let b = self.xr_bridge.lock().unwrap();
            (b.visible(), b.mouse_owned())
        };
        if !visible || !self.xr_owns_input() {
            self.input_realm = dome::Realm::Desktop;
            return false;
        }
        if !owned {
            self.dispatch_xr(Packet {
                events: vec![SeatEvent::Reset],
                actions: vec![],
            });
        }
        let mut b = self.xr_bridge.lock().unwrap();
        b.packets.clear();
        b.last_mouse = Some(std::time::Instant::now());
        self.input_realm = dome::Realm::Xr;
        true
    }
    pub fn xr_mouse_motion(&mut self, dx: f64, dy: f64) {
        if self.super_mmb_motion(dx) {return;}
        let (session, layout, cursor, captured) = {
            let b = self.xr_bridge.lock().unwrap();
            (
                b.session.clone(),
                b.shell.clone(),
                if self.mouse_angles.is_some() { self.mouse_cursor.clone() } else { b.cursor.clone() },
                !b.mouse_buttons.is_empty(),
            )
        };
        let scene = self.scene.lock().unwrap().clone();
        let mut surfaces = dome::xr_input::quads_in_fov(&scene, session.head, session.mouse_viewport);
        if let Some(viewport) = session.mouse_viewport {
            surfaces.extend(dome::xr_input::shell_quads_in_fov(&scene, session.head, &layout, viewport));
        }
        surfaces.retain(|q| self.xr_animation.interactive(scene.presentation, q.id));
        let background = dome::xr_input::mouse_dome(&scene, session.head);
        let head = glam::Vec3::from_array(session.head.position_m);
        // Rebase from the last surface UV as the head or carousel moves. A
        // stationary mouse stays on its panel instead of drifting with head movement.
        let previous = self.mouse_angles.unwrap_or([0., 0.]);
        let mut angles = {
            cursor
                .as_ref()
                .and_then(|c| {
                    surfaces
                        .iter()
                        .chain(std::iter::once(&background))
                        .find(|q| q.id == c.id)
                        .map(|q| q.point(c.uv))
                })
                .map(|p| dome::xr_input::mouse_angles_at(session.head, p))
                .unwrap_or(previous)
        };
        let viewport = session.mouse_viewport.unwrap_or_else(|| {
            let horizontal = session.horizontal_fov_deg.max(30.).to_radians() / 2.;
            let vertical = scene.config.vertical_fov_deg.clamp(30., 160.).to_radians() / 2.;
            [-horizontal, horizontal, -vertical, vertical]
        });
        dome::xr_input::confine_mouse(&mut angles, viewport);
        dome::xr_input::move_mouse(&mut angles, dx, dy);
        dome::xr_input::confine_mouse(&mut angles, viewport);
        self.mouse_angles = Some(angles);
        let ray = dome::xr_input::mouse_ray(session.head, angles);
        self.xr_bridge.lock().unwrap().mouse_angles = Some(angles);
        let target = dome::xr_input::mouse_target_with_ui(
            &scene, &surfaces, head, ray,
            if captured { cursor.as_ref().map(|c| c.id) } else { None },
            self.shell_modal_open(),
        );
        self.mouse_cursor = target.map(|(id, uv, _)| dome::xr_transport::Cursor { id, uv })
            .or_else(|| background.hit(head, ray, true)
                .map(|uv| dome::xr_transport::Cursor { id: background.id, uv }));
        if let Some((id, uv, _)) = target {
            self.dispatch_xr(Packet {
                events: vec![SeatEvent::Motion { id, uv }],
                actions: vec![],
            });
        } else if !captured {
            self.dispatch_xr(Packet {
                events: vec![SeatEvent::Leave],
                actions: vec![],
            });
        }
        self.xr_bridge.lock().unwrap().cursor = self.mouse_cursor.clone();
    }
    pub fn xr_mouse_button(&mut self, button: u32, pressed: bool) {
        self.xr_mouse_motion(0., 0.);
        let mut actions = vec![];
        if pressed && !self.shell_modal_open() {
            if let Some(id) = self
                .xr_target
                .filter(|id| !dome::xr_transport::is_shell(*id))
            {
                let scene = self.scene.lock().unwrap();
                let id = if scene.window(id).is_some() {
                    id
                } else {
                    scene.popup_owner(id)
                };
                actions.push(dome::Action::Focus { id });
            }
        }
        self.dispatch_xr(Packet {
            events: vec![SeatEvent::Button { button, pressed }],
            actions,
        });
        let mut b = self.xr_bridge.lock().unwrap();
        if pressed {
            b.mouse_buttons.insert(button);
        } else {
            b.mouse_buttons.remove(&button);
        }
    }
    pub fn export_xr_surface(&mut self, renderer: &mut GlesRenderer) {
        if self.session.locked() {
            let mut bridge = self.xr_bridge.lock().unwrap();
            bridge.frames.clear(); bridge.gpu_frames.clear();
            return;
        }
        self.update_xr_density();
        self.update_task_previews(renderer);
        let mut live: Vec<_> = self.registry.iter().map(|(id,_)| *id)
            .chain(self.spatial_popups.iter().map(|(id,_)| *id)).collect();
        let shells: Vec<_> = {
            let b = self.xr_bridge.lock().unwrap();
            b.frames.iter().filter(|(id, _)| dome::xr_transport::is_shell(**id))
                .map(|(id, source)| (*id, source.clone(), b.gpu_requested.contains_key(id)))
                .collect()
        };
        live.extend(shells.iter().map(|(id, _, _)| *id));
        self.xr_gpu_capture.publish(&self.xr_bridge, &live);
        for (id, source, requested) in shells {
            if requested {
                if let Err(error) = self.xr_gpu_capture.render_shell(renderer, id, &source) {
                    self.xr_gpu_capture.disable(&error);
                    let mut bridge = self.xr_bridge.lock().unwrap();
                    bridge.gpu_socket = None;
                    bridge.gpu_frames.clear();
                    break;
                }
            }
        }
        // Inspect every requested surface. An unchanged surface keeps its
        // publication stamp, so choosing only the oldest publication starves
        // every other window once that surface becomes idle. Capture's damage
        // signatures and pending fences already skip unnecessary GPU work.
        let gpu_requests: Vec<_> = {
            let b = self.xr_bridge.lock().unwrap();
            b.gpu_requested.keys()
                .filter(|id| live.contains(id) && !dome::xr_transport::is_shell(**id))
                .copied().collect()
        };
        for id in gpu_requests {
            if let Some((surface, geo)) = self.popup_surface(id) {
                if geo.size.w > 0 && geo.size.h > 0 && geo.size.w <= 4096 && geo.size.h <= 4096 {
                    let scene = self.scene.lock().unwrap();
                    let scale = scene.config.xr_render_scale;
                    let viewport = scene.popup(id).and_then(|p| p.viewport).filter(|v| v.band() > 0.);
                    drop(scene);
                    if let Err(e) = self.xr_gpu_capture.render(renderer, id, &surface, geo, scale, viewport) {
                        self.xr_gpu_capture.disable(&e);
                        let mut bridge = self.xr_bridge.lock().unwrap();
                        bridge.gpu_socket = None;
                        bridge.gpu_frames.clear();
                    }
                }
            }
        }
        let request = {
            let mut b = self.xr_bridge.lock().unwrap();
            b.requested.retain(|_, t| t.elapsed().as_secs_f32() < 0.5);
            b.frames.retain(|id, _| {
                dome::xr_transport::is_shell(*id)
                    || self.registry.iter().any(|(i, _)| i == id)
                    || self.spatial_popups.iter().any(|(i, _)| i == id)
            });
            b.requested
                .keys()
                .filter(|id| !dome::xr_transport::is_shell(**id))
                .filter_map(|id| {
                    let age = b
                        .frames
                        .get(id)
                        .map(|f| f.stamp.elapsed().as_secs_f32())
                        .unwrap_or(100.);
                    if age >= 0.05 {
                        Some((*id, age))
                    } else {
                        None
                    }
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|v| v.0)
        };
        let Some(id) = request else {
            return;
        };
        let Some((surface, geo)) = self.popup_surface(id) else {
            return;
        };
        let size = geo.size;
        if size.w < 1 || size.h < 1 || size.w > 4096 || size.h > 4096 {
            return;
        }
        let scale_setting = self.scene.lock().unwrap().config.xr_render_scale;
        let (width, height, scale) = dome::xr_gpu::capture_size(size.w as u32, size.h as u32, scale_setting);
        let size = smithay::utils::Size::<i32, smithay::utils::Physical>::from((width as i32, height as i32));
        let capture = (|| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            let mut texture: GlesTexture =
                renderer.create_buffer(Fourcc::Argb8888, (size.w, size.h).into())?;
            let mut target = renderer.bind(&mut texture)?;
            let elements = render_elements_from_surface_tree::<
                GlesRenderer,
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(
                renderer,
                &surface,
                ((-geo.loc.x as f64 * scale).round() as i32, (-geo.loc.y as f64 * scale).round() as i32),
                scale,
                1.,
                Kind::Unspecified,
            );
            let mut tracker = OutputDamageTracker::new((size.w, size.h), scale, Transform::Normal);
            tracker.render_output(renderer, &mut target, 0, &elements, [0., 0., 0., 0.])?;
            let mapping = renderer.copy_framebuffer(
                &target,
                Rectangle::from_size((size.w, size.h).into()),
                Fourcc::Abgr8888,
            )?; // Normal offscreen rendering and glReadPixels already share the surface row order.
                // GlesMapping::flipped describes re-import into GL, not the raw HTTP RGBA contract.
            let rgba = renderer.map_texture(&mapping)?.to_vec();
            Ok(rgba)
        })();
        match capture {
            Ok(rgba) => {
                let viewport = self
                    .scene
                    .lock()
                    .unwrap()
                    .popup(id)
                    .and_then(|p| p.viewport)
                    .filter(|v| v.band() > 0.);
                let rgba = if let Some(v) = viewport {
                    dome::context_menu::texture_scaled(&rgba, size.w as u32, v, scale as f32)
                } else {
                    rgba
                };
                let height = viewport.map_or(size.h as u32, |v| (v.height * scale as f32).round() as u32);
                self.xr_bridge.lock().unwrap().frames.insert(
                    id,
                    dome::xr_transport::Surface {
                        width: size.w as u32,
                        height,
                        rgba: std::sync::Arc::new(rgba),
                        scale,
                        stamp: std::time::Instant::now(),
                    },
                );
            }
            Err(e) => {
                self.shell_error = format!("XR window readback: {e}");
            }
        }
    }
}
