use crate::Dome;
use dome::xr_input::{Packet, SeatEvent};
use smithay::{
    backend::input::ButtonState,
    backend::{
        allocator::Fourcc,
        renderer::{
            damage::OutputDamageTracker,
            element::{surface::WaylandSurfaceRenderElement, AsRenderElements},
            gles::{GlesRenderer, GlesTexture},
            Bind, ExportMem, Offscreen,
        },
    },
    input::pointer::{AxisFrame, ButtonEvent, MotionEvent},
    utils::{Rectangle, Transform, SERIAL_COUNTER},
};
impl Dome {
    pub fn export_xr_shell(&self, pixels: &[u8], width: i32, height: i32) {
        use dome::xr_transport::{ShellLayout, Surface, TASK_BAR, TOP_BAR};
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
        if self.about || self.appearance_open || self.session_confirm.is_some() {
            top = height;
        }
        if self.desktop_manager {
            bottom = 390.min(height);
        }
        if let Some(index) = self.task_popup {
            bottom = bottom.max((height as f64 - self.popup_rect(index, width, height).1) as i32);
        }
        let mut b = self.xr_bridge.lock().unwrap();
        b.shell = ShellLayout {
            width: width as u32,
            height: height as u32,
            top: top as u32,
            bottom: bottom as u32,
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
        if self.xr_was_visible && !visible {
            let size = self
                .space
                .outputs()
                .next()
                .and_then(|o| self.space.output_geometry(o))
                .map(|r| r.size)
                .unwrap_or((1280, 800).into());
            self.scene
                .lock()
                .unwrap()
                .desktop_fallback(size.w as f32, size.h as f32);
            {
                let mut b = self.xr_bridge.lock().unwrap();
                b.mouse_buttons.clear();
                b.last_mouse = None;
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
                        let x = uv[0] as f64 * layout.width as f64;
                        let y = if id == dome::xr_transport::TOP_BAR {
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

                    if let Some((_, w)) = self.registry.iter().find(|(i, _)| *i == id).cloned() {
                        let pos = self.space.element_location(&w).or_else(|| {
                            self.scene
                                .lock()
                                .unwrap()
                                .window(id)
                                .map(|w| (w.desktop.x as i32, w.desktop.y as i32).into())
                        });
                        if let Some(pos) = pos {
                            let geo = w.geometry();
                            let location = pos.to_f64()
                                + smithay::utils::Point::<f64, smithay::utils::Logical>::from((
                                    uv[0] as f64 * geo.size.w as f64,
                                    uv[1] as f64 * geo.size.h as f64,
                                ));
                            self.xr_target = Some(id);
                            pointer.motion(
                                self,
                                Some((w.toplevel().unwrap().wl_surface().clone(), pos.to_f64())),
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
                    if pressed
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
                            let layout = self.xr_bridge.lock().unwrap().shell.clone();
                            let tray = self.visible_tray(layout.width as i32);
                            let tx = layout.width as f64 - 176. - tray.items.len() as f64 * 28.;
                            if y < 36.
                                && x >= tx
                                && x < layout.width as f64 - 176.
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

                    if pressed && self.xr_target.is_none() {
                        continue;
                    }
                    if pressed {
                        self.xr_buttons.insert(button);
                    } else {
                        self.xr_buttons.remove(&button);
                    }
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
                SeatEvent::Reset => {
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
        angles[0] += dx as f32 * 0.0025;
        angles[1] = (angles[1] - dy as f32 * 0.0025).clamp(-1.5, 1.5);
        let [yaw, pitch] = *angles;
        let ray = glam::Vec3::from_array(session.head.rotate([
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            -yaw.cos() * pitch.cos(),
        ]));
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
                events: vec![SeatEvent::Reset],
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
                dome::xr_transport::is_shell(*id) || self.registry.iter().any(|(i, _)| i == id)
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
        let Some((_, w)) = self.registry.iter().find(|(i, _)| *i == id).cloned() else {
            return;
        };
        let geo = w.geometry();
        let size = geo.size;
        if size.w < 1 || size.h < 1 || size.w > 4096 || size.h > 4096 {
            return;
        }
        let capture = (|| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            let mut texture: GlesTexture =
                renderer.create_buffer(Fourcc::Argb8888, (size.w, size.h).into())?;
            let mut target = renderer.bind(&mut texture)?;
            let elements = w.render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                renderer,
                (-geo.loc.x, -geo.loc.y).into(),
                1.0.into(),
                1.,
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
                self.xr_bridge.lock().unwrap().frames.insert(
                    id,
                    dome::xr_transport::Surface {
                        width: size.w as u32,
                        height: size.h as u32,
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
