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
        self.sync_input_route();
        match event {
            InputEvent::Keyboard { event, .. } => {
                if self.native_ignored_keys.remove(&event.key_code())
                    && event.state() == smithay::backend::input::KeyState::Released
                {
                    return;
                }
                if self.history_panel.is_none()
                    && !self.launcher
                    && self.menu.is_none()
                    && !self.desktop_manager
                    && !self.appearance_open
                    && !self.xr_settings_open
                    && self.session_confirm.is_none()
                {
                    self.input_realm = if self.input_xr_active {
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
                        if pressed {
                            state.context_trigger = None;
                        }
                        // Keep ownership until release, including confirmation keys that
                        // close the launcher. Forward releases of keys held before it opened.
                        if !pressed && state.shell_keys.remove(&code) {
                            return FilterResult::Intercept(());
                        }
                        if pressed && state.shell_keys.contains(&code) && !state.launcher {
                            return FilterResult::Intercept(());
                        }
                        if pressed
                            && state.capture_binding(
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
                            )
                        {
                            state.shell_keys.insert(code);
                            return FilterResult::Intercept(());
                        }
                        if pressed
                            && sym.raw() == 0x20
                            && state
                                .keyboard_config
                                .switch_shortcut
                                .matches(mods.logo, mods.ctrl, mods.alt, mods.shift)
                        {
                            state.shell_keys.insert(code);
                            if state.keyboard_config.ime {
                                state.ime.toggle();
                            } else {
                                state.keyboard_switch += 1;
                            }
                            return FilterResult::Intercept(());
                        }
                        if pressed
                            && state.keyboard_settings_key(sym.raw(), sym.key_char(), mods.ctrl)
                        {
                            state.shell_keys.insert(code);
                            return FilterResult::Intercept(());
                        }
                        // Visibility remains available from launchers and modal shell panels.
                        if pressed {
                            if let Some(command) = state
                                .resolve_shortcut(
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
                                })
                            {
                                state.shell_keys.insert(code);
                                state.menu = None;
                                state.command(command);
                                return FilterResult::Intercept(());
                            }
                        }
                        if pressed && state.history_key(sym.raw()) {
                            state.shell_keys.insert(code);
                            return FilterResult::Intercept(());
                        }
                        if pressed && state.xr_settings_key(sym.raw()) {
                            state.shell_keys.insert(code);
                            return FilterResult::Intercept(());
                        }
                        if pressed && sym.raw() == 0xff1b && state.tray.state().menu.is_some() {
                            state.tray.close();
                            state.shell_keys.insert(code);
                            return FilterResult::Intercept(());
                        }
                        if pressed && state.task_context_key(sym.raw()) {
                            state.shell_keys.insert(code);
                            return FilterResult::Intercept(());
                        }
                        if pressed && state.task_context && sym.raw() != 0xff1b {
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
                            if let Some(command) = state.resolve_shortcut(
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
                                        state.launch_index = 0;
                                    }
                                    0xff52 | 0xff51 => {
                                        state.launch_index = state.launch_index.saturating_sub(1)
                                    }
                                    0xff54 | 0xff53 => {
                                        state.launch_index = (state.launch_index + 1)
                                            .min(state.matching_apps().len().saturating_sub(1))
                                    }
                                    0xff50 => state.launch_index = 0,
                                    0xff57 => {
                                        state.launch_index =
                                            state.matching_apps().len().saturating_sub(1)
                                    }
                                    0xff0d => state.launch_selected(),
                                    _ => {
                                        if let Some(c) = sym.key_char().filter(|c| !c.is_control())
                                        {
                                            if state.launch_query.len() < 80 {
                                                state.launch_query.push(c);
                                                state.launch_index = 0;
                                            }
                                        }
                                    }
                                }
                            }
                            return FilterResult::Intercept(());
                        }
                        if state.input_xr_active
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
                self.update_keyboard();
            }
            InputEvent::PointerMotion { event, .. } => {
                let delta = event.delta();
                if delta.x != 0. || delta.y != 0. {
                    if self.mouse_takeover() {
                        self.xr_mouse_motion(delta.x, delta.y);
                    } else {
                        let pointer = self.seat.get_pointer().unwrap();
                        let current = pointer.current_location();
                        self.desktop_mouse_motion(
                            self.clamp_desktop_pointer(
                                (current.x + delta.x, current.y + delta.y).into(),
                            ),
                            event.time_msec(),
                        );
                    }
                }
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output_geo = self.primary_output_geometry();

                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
                let previous = self.mouse_last_absolute.replace([pos.x, pos.y]);
                let delta = previous
                    .map(|p| [pos.x - p[0], pos.y - p[1]])
                    .unwrap_or([0., 0.]);
                if previous.is_none_or(|p| p != [pos.x, pos.y]) && self.mouse_takeover() {
                    self.xr_mouse_motion(delta[0], delta[1]);
                    return;
                }
                if self.input_xr_active {
                    return;
                }
                self.input_realm = dome::Realm::Desktop;

                self.desktop_mouse_motion(pos, event.time_msec());
            }
            InputEvent::PointerButton { event, .. } => {
                let code = event.button_code();
                if event.state() == ButtonState::Released {
                    self.physical_buttons.remove(&code);
                    if self.ignored_buttons.remove(&code) {
                        return;
                    }
                } else {
                    self.ignored_buttons.remove(&code);
                    self.physical_buttons.insert(code);
                }
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
                if button_state == ButtonState::Released
                    && self.context_scroll_buttons.remove(&button)
                {
                    return;
                }
                if button_state == ButtonState::Pressed && !pointer.is_grabbed() {
                    let geometry = self.output_geometry_at(pos);
                    let local = pos - geometry.loc.to_f64();
                    if let Some(toast) = self
                        .notifications
                        .layout(geometry.size.w, geometry.size.h)
                        .into_iter()
                        .find(|n| n.contains(local.x, local.y))
                    {
                        self.context_scroll_buttons.insert(button);
                        if button == 0x110 {
                            self.notifications.click(&toast, local.x, local.y);
                        }
                        return;
                    }
                }
                if button_state == ButtonState::Pressed && button == 0x110 {
                    if let Some((id, direction)) = self.desktop_context_control(pos) {
                        self.context_scroll_buttons.insert(button);
                        self.scroll_context(id, direction as f32 * 96.);
                        return;
                    }
                }
                if self.chrome_button.is_some() {
                    if button_state == ButtonState::Released && button == 0x110 {
                        self.chrome_button_release(pos.x, pos.y);
                    }
                    return;
                }
                if self.chrome_drag.is_some() {
                    if button_state == ButtonState::Released {
                        if let Some(drag) = self.chrome_drag.take() {
                            if !drag.resize {
                                self.finish_desktop_move(drag.id);
                            }
                        }
                    }
                    return;
                }
                if button_state == ButtonState::Pressed
                    && !pointer.is_grabbed()
                    && !self.about
                    && !self.appearance_open
                    && !self.xr_settings_open
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
                        let size = self.shell_geometry().size;
                        let global_pos = pos;
                        let pos = pos - self.shell_geometry().loc.to_f64();
                        let tray = self.visible_tray(size.w);
                        let tray_start = size.w as f64 - 220. - tray.items.len() as f64 * 28.;
                        if pos.y < 36.
                            && pos.x >= tray_start
                            && pos.x < size.w as f64 - 220.
                            && tray.menu.is_none()
                        {
                            self.activate_tray(
                                ((pos.x - tray_start) / 28.) as usize,
                                pos.x as i32,
                                pos.y as i32,
                                button == 0x111,
                            );
                            return;
                        }
                        if pos.y >= (size.h - 60) as f64
                            && pos.x >= 110.
                            && pos.x < (size.w - 250) as f64
                        {
                            self.activate_task(((pos.x - 110.) / 130.) as usize, button);
                        } else {
                            self.shell_click(global_pos.x, global_pos.y);
                        }
                    }
                    return;
                }
                if ButtonState::Pressed == button_state && !pointer.is_grabbed() {
                    if let Some((id, surface, _)) = self.popup_under(pos) {
                        let focus_id = {
                            let scene = self.scene.lock().unwrap();
                            if scene.window(id).is_some() {
                                id
                            } else {
                                scene.popup_owner(id)
                            }
                        };
                        self.action(dome::Action::Focus { id: focus_id });
                        if !self.ime_popups.iter().any(|p| p.wl_surface() == &surface) {
                            keyboard.set_focus(self, Some(surface), serial);
                        }
                    } else if let Some((window, _loc)) = self
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

                if button_state == ButtonState::Pressed {
                    self.desktop_buttons.insert(button);
                } else {
                    self.desktop_buttons.remove(&button);
                }
                self.context_button_origin(button, button_state == ButtonState::Pressed);
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
                if self.history_scroll(vertical_amount) {
                    return;
                }
                if self.xr_settings_scroll(vertical_amount) {
                    return;
                }
                let pos = self.seat.get_pointer().unwrap().current_location();
                if let Some((id, _, _)) = self.popup_under(pos) {
                    if self.scroll_context(id, vertical_amount as f32 * 3.) {
                        return;
                    }
                }
                if let Some((id, _)) = self.desktop_context_control(pos) {
                    if self.scroll_context(id, vertical_amount as f32 * 3.) {
                        return;
                    }
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
    pub fn desktop_mouse_motion(
        &mut self,
        pos: smithay::utils::Point<f64, smithay::utils::Logical>,
        time: u32,
    ) {
        self.desktop_pointer = Some([pos.x, pos.y]);
        let size = self.output_geometry_at(pos).size;
        let serial = SERIAL_COUNTER.next_serial();

        let pointer = self.seat.get_pointer().unwrap();

        let local = pos - self.output_geometry_at(pos).loc.to_f64();
        self.task_motion(local.x, local.y, size.w, size.h);
        if self.chrome_button_motion(pos.x, pos.y) || self.chrome_motion(pos.x, pos.y) {
            // Relative libinput deltas must continue from the dragged pointer's new position.
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location: pos,
                    serial,
                    time,
                },
            );
            pointer.frame(self);
            return;
        }
        if let Some(menu) = self.menu {
            let x0 = [0., 46., 230., 280., 332., 390., 470.][menu];
            self.menu_hover = if local.x >= x0 && local.x < x0 + 380. && local.y >= 40. {
                ((local.y - 40.) / 28.) as i32
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
                time,
            },
        );
        pointer.frame(self);
    }
}
