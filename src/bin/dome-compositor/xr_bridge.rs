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
        use dome::xr_transport::{ShellLayout, Surface, NOTIFICATIONS, TASK_BAR, TOP_BAR};
        let mut top = 36;
        let mut bottom = 60;
        if let Some(menu) = self.menu {
            top = (40 + self.menu_items(menu).len() as i32 * 28 + 4).min(height);
        }
        let tray = self.tray.state();
        if tray.menu.is_some() {
            top = top.max((40 + tray.rows.len() as i32 * 28 + 4).min(height));
        }
        if self.launcher {
            top = 456.min(height);
        }
        if self.about
            || self.appearance_open
            || self.xr_settings_open
            || self.session_confirm.is_some()
        {
            top = height;
        }
        if self.desktop_manager {
            bottom = 390.min(height);
        }
        if let Some(index) = self.task_popup {
            bottom = bottom.max((height as f64 - self.popup_rect(index, width, height).1) as i32);
        }
        let toasts = self.notifications.layout(width, height);
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
        b.frames.remove(&NOTIFICATIONS);
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
            b.frames.insert(
                NOTIFICATIONS,
                Surface {
                    width: w,
                    height: h,
                    rgba: std::sync::Arc::new(rgba),
                    stamp: std::time::Instant::now(),
                },
            );
        }
        b.shell = ShellLayout {
            width: width as u32,
            height: height as u32,
            top: top as u32,
            bottom: bottom as u32,
            notifications: notification_rect,
        };
        for (id, start, rows) in [(TOP_BAR, 0, top), (TASK_BAR, height - bottom, bottom)] {
            let mut rgba = Vec::with_capacity((width * rows * 4) as usize);
            for pixel in pixels[(start * width * 4) as usize..((start + rows) * width * 4) as usize]
                .chunks_exact(4)
            {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
            b.frames.insert(
                id,
                Surface {
                    width: width as u32,
                    height: rows as u32,
                    rgba: std::sync::Arc::new(rgba),
                    stamp: std::time::Instant::now(),
                },
            );
        }
    }
    pub fn xr_input(&mut self) {
        if !self.native_session_active {
            self.xr_bridge.lock().unwrap().packets.clear();
            return;
        }
        let presentation = self.scene.lock().unwrap().presentation;
        self.xr_animation = self
            .xr_animator
            .update(self.start_time.elapsed().as_secs_f64(), presentation);
        if presentation != self.xr_presentation {
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
    }
    pub fn dispatch_xr(&mut self, mut p: Packet) {
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
            let presentation = self.scene.lock().unwrap().presentation;
            if presentation.desktop && self.xr_animation.opacity >= 1. {
                self.action(a);
            }
        }
        self.sync_scene();
        for e in p.events {
            let pointer = self.seat.get_pointer().unwrap();
            let time = self.start_time.elapsed().as_millis() as u32;
            match e {
                SeatEvent::Motion { id, uv } => {
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
                        let x = if id == dome::xr_transport::NOTIFICATIONS {
                            let r = layout.notifications.unwrap_or([0; 4]);
                            r[0] as f64 + uv[0] as f64 * r[2] as f64
                        } else {
                            uv[0] as f64 * layout.width as f64
                        };
                        let y = if id == dome::xr_transport::NOTIFICATIONS {
                            let r = layout.notifications.unwrap_or([0; 4]);
                            r[1] as f64 + uv[1] as f64 * r[3] as f64
                        } else if id == dome::xr_transport::TOP_BAR {
                            uv[1] as f64 * layout.top as f64
                        } else {
                            layout.height as f64 - layout.bottom as f64
                                + uv[1] as f64 * layout.bottom as f64
                        };
                        self.xr_target = Some(id);
                        self.xr_shell_pos = Some([x, y]);
                        self.task_motion(x, y, layout.width as i32, layout.height as i32);
                        if let Some(menu) = self.menu {
                            let x0 = [0., 46., 230., 280., 332., 390., 470.][menu];
                            self.menu_hover = if x >= x0 && x < x0 + 380. && y >= 40. {
                                ((y - 40.) / 28.) as i32
                            } else {
                                -1
                            };
                        }
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
                        scene.popup(id).map_or_else(
                            || scene.window(id).is_some(),
                            |p| scene.popup_visible(p, false),
                        )
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
                    if let Some([x, y]) = self.xr_shell_pos {
                        if pressed {
                            self.xr_buttons.insert(button);
                            if self.xr_target == Some(dome::xr_transport::NOTIFICATIONS)
                                && button != 0x110
                            {
                                continue;
                            }
                            let layout = self.xr_bridge.lock().unwrap().shell.clone();
                            let tray = self.visible_tray(layout.width as i32);
                            let tx = layout.width as f64 - 220. - tray.items.len() as f64 * 28.;
                            if y < 36.
                                && x >= tx
                                && x < layout.width as f64 - 220.
                                && tray.menu.is_none()
                            {
                                self.tray.activate(
                                    ((x - tx) / 28.) as usize,
                                    x as i32,
                                    y as i32,
                                    button == 0x111,
                                );
                            } else if y >= (layout.height as f64 - 60.)
                                && x >= 110.
                                && x < layout.width as f64 - 160.
                            {
                                self.activate_task(((x - 110.) / 130.) as usize, button);
                            } else {
                                self.shell_click(x, y);
                            }
                        } else {
                            self.xr_buttons.remove(&button);
                        }
                        continue;
                    }

                    if pressed && self.xr_target.is_none() && !pointer.is_grabbed() {
                        continue;
                    }
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
                    if self.xr_shell_pos.is_some() && self.xr_settings_scroll(vertical) {
                        continue;
                    }
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
                    self.context_scroll_hover = None;
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
                    self.context_scroll_hover = None;
                    if self.xr_shell_pos.take().is_some() {
                        self.xr_buttons.clear();
                        self.xr_target = None;
                        continue;
                    }
                    for button in self.xr_buttons.drain().collect::<Vec<_>>() {
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
    /// Physical devices take exclusive ownership until idle and all buttons are released.
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
            self.mouse_angles = None;
        }
        let mut b = self.xr_bridge.lock().unwrap();
        b.packets.clear();
        b.last_mouse = Some(std::time::Instant::now());
        self.input_realm = dome::Realm::Xr;
        true
    }
    pub fn xr_mouse_motion(&mut self, dx: f64, dy: f64) {
        let (session, layout, cursor, captured) = {
            let b = self.xr_bridge.lock().unwrap();
            (
                b.session.clone(),
                b.shell.clone(),
                b.cursor.clone(),
                !b.mouse_buttons.is_empty(),
            )
        };
        let mut scene = self.scene.lock().unwrap().clone();
        scene.config.upper_bar_max_width_deg = scene
            .config
            .upper_bar_max_width_deg
            .min(session.horizontal_fov_deg.max(30.));
        scene.config.taskbar_max_width_deg = scene
            .config
            .taskbar_max_width_deg
            .min(session.horizontal_fov_deg.max(30.));
        let mut surfaces = dome::xr_input::quads(&scene, session.head);
        surfaces.extend(dome::xr_input::shell_quads(&scene, session.head, &layout));
        surfaces.retain(|q| self.xr_animation.interactive(scene.presentation, q.id));
        let head = glam::Vec3::from_array(session.head.position_m);
        let angles = self.mouse_angles.get_or_insert_with(|| {
            cursor
                .as_ref()
                .and_then(|c| {
                    surfaces
                        .iter()
                        .find(|q| q.id == c.id)
                        .map(|q| q.point(c.uv))
                })
                .map(|p| {
                    let d =
                        glam::Vec3::from_array(session.head.inverse_rotate((p - head).to_array()))
                            .normalize_or_zero();
                    [d.x.atan2(-d.z), d.y.clamp(-1., 1.).asin()]
                })
                .unwrap_or([0., 0.])
        });
        dome::xr_input::move_mouse(angles, dx, dy);
        let ray = dome::xr_input::mouse_ray(session.head, *angles);
        self.xr_bridge.lock().unwrap().mouse_angles = Some(*angles);
        let target = surfaces
            .iter()
            .filter(|q| !captured || cursor.as_ref().is_some_and(|c| c.id == q.id))
            .filter_map(|q| {
                q.hit(head, ray, captured)
                    .map(|uv| (q.id, uv, q.point(uv).distance(head)))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
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
            self.xr_bridge.lock().unwrap().cursor = None;
        }
    }
    pub fn xr_mouse_button(&mut self, button: u32, pressed: bool) {
        self.xr_mouse_motion(0., 0.);
        let mut actions = vec![];
        if pressed {
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
                (-geo.loc.x, -geo.loc.y),
                1.0,
                1.,
                Kind::Unspecified,
            );
            let mut tracker = OutputDamageTracker::new((size.w, size.h), 1., Transform::Normal);
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
                    dome::context_menu::texture(&rgba, size.w as u32, v)
                } else {
                    rgba
                };
                let height = viewport.map_or(size.h as u32, |v| v.height.round() as u32);
                self.xr_bridge.lock().unwrap().frames.insert(
                    id,
                    dome::xr_transport::Surface {
                        width: size.w as u32,
                        height,
                        rgba: std::sync::Arc::new(rgba),
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
