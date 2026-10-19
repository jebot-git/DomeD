use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::FilterResult,
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::SERIAL_COUNTER,
};

use crate::state::Dome;

impl Dome {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => {
                if !self.launcher
                    && self.menu.is_none()
                    && !self.desktop_manager
                    && !self.appearance_open
                    && self.session_confirm.is_none()
                {
                    self.input_realm = if self.xr_bridge.lock().unwrap().visible() {
                        dome::Realm::Xr
                    } else {
                        dome::Realm::Desktop
                    };
                }
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);

                self.seat.get_keyboard().unwrap().input::<(), _>(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |state, mods, key| {
                        let sym = key.modified_sym();
                        let code = event.key_code();
                        let pressed = event.state() == smithay::backend::input::KeyState::Pressed;
                        // Keep ownership until release, including confirmation keys that
                        // close the launcher. Forward releases of keys held before it opened.
                        if !pressed && state.shell_keys.remove(&code) {
                            return FilterResult::Intercept(());
                        }
                        if pressed && state.shell_keys.contains(&code) && !state.launcher {
                            return FilterResult::Intercept(());
                        }
                        // Visibility remains available from launchers and modal shell panels.
                        if pressed {
                            if let Some(command) = dome::shortcuts::resolve(
                                sym.raw(),
                                dome::shortcuts::Mods {
                                    logo: mods.logo,
                                    ctrl: mods.ctrl,
                                    alt: mods.alt,
                                    shift: mods.shift,
                                },
                            )
                            .filter(|c| {
                                matches!(
                                    c,
                                    dome::shortcuts::Command::ToggleXrBars
                                        | dome::shortcuts::Command::ToggleXrTop
                                        | dome::shortcuts::Command::ToggleXrBottom
                                        | dome::shortcuts::Command::ToggleXrDesktop
                                )
                            }) {
                                state.shell_keys.insert(code);
                                state.menu = None;
                                state.command(command);
                                return FilterResult::Intercept(());
                            }
                        }
                        if pressed && sym.raw() == 0xff1b && state.tray.state().menu.is_some() {
                            state.tray.close();
                            state.shell_keys.insert(code);
                            return FilterResult::Intercept(());
                        }
                        if pressed && sym.raw() == 0xff1b && state.task_popup.is_some() {
                            state.task_popup = None;
                            state.task_context = false;
                            state.shell_keys.insert(code);
                            return FilterResult::Intercept(());
                        }
                        if pressed && (state.appearance_open || state.session_confirm.is_some()) {
                            state.shell_keys.insert(code);
                            if sym.raw() == 0xff1b {
                                state.appearance_open = false;
                                state.session_confirm = None;
                            }
                            return FilterResult::Intercept(());
                        }
                        if pressed && state.about {
                            state.shell_keys.insert(code);
                            state.about = false;
                            return FilterResult::Intercept(());
                        }
                        if pressed {
                            if let Some(command) = dome::shortcuts::resolve(
                                if mods.shift && mods.logo {
                                    key.raw_latin_sym_or_raw_current_sym()
                                        .filter(|s| (49..=56).contains(&s.raw()))
                                        .unwrap_or(sym)
                                        .raw()
                                } else {
                                    sym.raw()
                                },
                                dome::shortcuts::Mods {
                                    logo: mods.logo,
                                    ctrl: mods.ctrl,
                                    alt: mods.alt,
                                    shift: mods.shift,
                                },
                            ) {
                                state.shell_keys.insert(code);
                                state.menu = None;
                                state.command(command);
                                return FilterResult::Intercept(());
                            }
                            if let Some(menu) = state.menu {
                                state.shell_keys.insert(code);
                                match sym.raw() {
                                    0xff1b => state.menu = None,
                                    0xff51 => {
                                        state.menu = Some((menu + 6) % 7);
                                        state.menu_hover = 0;
                                    }
                                    0xff53 => {
                                        state.menu = Some((menu + 1) % 7);
                                        state.menu_hover = 0;
                                    }
                                    0xff52 => state.menu_hover = (state.menu_hover - 1).max(0),
                                    0xff54 => {
                                        state.menu_hover = (state.menu_hover + 1)
                                            .min(state.menu_items(menu).len() as i32 - 1)
                                    }
                                    0xff0d => {
                                        let c = state
                                            .menu_items(menu)
                                            .get(state.menu_hover.max(0) as usize)
                                            .map(|i| i.2);
                                        state.menu = None;
                                        if let Some(c) = c {
                                            state.command(c);
                                        }
                                    }
                                    _ => (),
                                };
                                return FilterResult::Intercept(());
                            }
                        }
                        if state.launcher && pressed {
                            state.shell_keys.insert(code);
                            {
                                match sym.raw() {
                                    0xff1b => {
                                        state.launcher = false;
                                    }
                                    0xff08 => {
                                        state.launch_query.pop();
                                    }
                                    0xff0d => state.launch_selected(),
                                    _ => {
                                        if let Some(c) = sym.key_char().filter(|c| !c.is_control())
                                        {
                                            if state.launch_query.len() < 80 {
                                                state.launch_query.push(c);
                                            }
                                        }
                                    }
                                }
                            }
                            return FilterResult::Intercept(());
                        }
                        if pressed && mods.logo && sym.key_char() == Some(' ') {
                            state.shell_keys.insert(code);
                            {
                                state.launcher = true;
                                state.launch_query.clear();
                            }
                            return FilterResult::Intercept(());
                        }
                        if state.xr_bridge.lock().unwrap().visible()
                            && (!state.scene.lock().unwrap().presentation.desktop
                                || state.xr_animation.opacity < 1.)
                        {
                            if pressed {
                                state.shell_keys.insert(code);
                                return FilterResult::Intercept(());
                            }
                            // Balance keys pressed before hiding; new hidden key presses
                            // and their owned releases have already been intercepted.
                        }
                        FilterResult::Forward
                    },
                );
            }
            InputEvent::PointerMotion { event, .. } => {
                let delta = event.delta();
                if (delta.x != 0. || delta.y != 0.) && self.mouse_takeover() {
                    self.xr_mouse_motion(delta.x, delta.y);
                }
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output = self.space.outputs().next().unwrap();

                let output_geo = self.space.output_geometry(output).unwrap();

                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
                let previous = self.mouse_last_absolute.replace([pos.x, pos.y]);
                let delta = previous
                    .map(|p| [pos.x - p[0], pos.y - p[1]])
                    .unwrap_or([0., 0.]);
                if previous.is_none_or(|p| p != [pos.x, pos.y]) && self.mouse_takeover() {
                    self.xr_mouse_motion(delta[0], delta[1]);
                    return;
                }
                if self.xr_bridge.lock().unwrap().visible() {
                    return;
                }
                self.input_realm = dome::Realm::Desktop;

                let serial = SERIAL_COUNTER.next_serial();

                let pointer = self.seat.get_pointer().unwrap();

                self.task_motion(pos.x, pos.y, output_geo.size.w, output_geo.size.h);
                if self.chrome_motion(pos.x, pos.y) {
                    return;
                }
                if let Some(menu) = self.menu {
                    let x0 = [0., 46., 230., 280., 332., 390., 470.][menu];
                    self.menu_hover = if pos.x >= x0 && pos.x < x0 + 380. && pos.y >= 40. {
                        ((pos.y - 40.) / 28.) as i32
                    } else {
                        -1
                    };
                }
                let under = if self.shell_at(pos.x, pos.y) {
                    None
                } else {
                    self.surface_under(pos)
                };

                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerButton { event, .. } => {
                if self.mouse_takeover() {
                    self.xr_mouse_button(
                        event.button_code(),
                        event.state() == ButtonState::Pressed,
                    );
                    return;
                }
                let pointer = self.seat.get_pointer().unwrap();
                let keyboard = self.seat.get_keyboard().unwrap();

                let serial = SERIAL_COUNTER.next_serial();

                let button = event.button_code();

                let button_state = event.state();

                let pos = pointer.current_location();
                if self.chrome_drag.is_some() {
                    if button_state == ButtonState::Released {
                        self.chrome_drag = None;
                    }
                    return;
                }
                if button_state == ButtonState::Pressed
                    && !pointer.is_grabbed()
                    && !self.about
                    && !self.appearance_open
                    && self.session_confirm.is_none()
                    && !self.task_context
                    && self.tray.state().menu.is_none()
                    && self.menu.is_none()
                    && !self.launcher
                    && !self.desktop_manager
                    && self.chrome_press(pos.x, pos.y)
                {
                    return;
                }
                if !pointer.is_grabbed() && self.shell_at(pos.x, pos.y) {
                    if button_state == ButtonState::Pressed {
                        let size = self
                            .space
                            .outputs()
                            .next()
                            .and_then(|o| self.space.output_geometry(o))
                            .map(|r| r.size)
                            .unwrap_or((1280, 800).into());
                        let tray = self.visible_tray(size.w);
                        let tray_start = size.w as f64 - 176. - tray.items.len() as f64 * 28.;
                        if pos.y < 36.
                            && pos.x >= tray_start
                            && pos.x < size.w as f64 - 176.
                            && tray.menu.is_none()
                        {
                            self.tray.activate(
                                ((pos.x - tray_start) / 28.) as usize,
                                pos.x as i32,
                                pos.y as i32,
                                button == 0x111,
                            );
                            return;
                        }
                        if pos.y >= (size.h - 60) as f64
                            && pos.x >= 110.
                            && pos.x < (size.w - 160) as f64
                        {
                            self.activate_task(((pos.x - 110.) / 130.) as usize, button);
                        } else {
                            self.shell_click(pos.x, pos.y);
                        }
                    }
                    return;
                }
                if ButtonState::Pressed == button_state && !pointer.is_grabbed() {
                    if let Some((window, _loc)) = self
                        .space
                        .element_under(pointer.current_location())
                        .map(|(w, l)| (w.clone(), l))
                    {
                        if let Some(id) = self.id_for(window.toplevel().unwrap().wl_surface()) {
                            self.action(dome::Action::Focus { id });
                        }
                        self.space.raise_element(&window, true);
                        keyboard.set_focus(
                            self,
                            Some(window.toplevel().unwrap().wl_surface().clone()),
                            serial,
                        );
                        self.space.elements().for_each(|window| {
                            window.toplevel().unwrap().send_pending_configure();
                        });
                    } else {
                        self.action(dome::Action::Defocus);
                        self.space.elements().for_each(|window| {
                            window.set_activated(false);
                            window.toplevel().unwrap().send_pending_configure();
                        });
                        keyboard.set_focus(self, Option::<WlSurface>::None, serial);
                    }
                };

                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: button_state,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event, .. } => {
                let source = event.source();

                let horizontal_amount = event.amount(Axis::Horizontal).unwrap_or_else(|| {
                    event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.
                });
                let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                    event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.
                });
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                if self.mouse_takeover() {
                    self.xr_mouse_motion(0., 0.);
                    self.dispatch_xr(dome::xr_input::Packet {
                        actions: vec![],
                        events: vec![dome::xr_input::SeatEvent::Scroll {
                            horizontal: horizontal_amount,
                            vertical: vertical_amount,
                        }],
                    });
                    return;
                }
                if self.task_scroll(vertical_amount) {
                    return;
                }
                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                    if let Some(discrete) = horizontal_amount_discrete {
                        frame = frame.v120(Axis::Horizontal, discrete as i32);
                    }
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                    if let Some(discrete) = vertical_amount_discrete {
                        frame = frame.v120(Axis::Vertical, discrete as i32);
                    }
                }

                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                let pointer = self.seat.get_pointer().unwrap();
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }
}
