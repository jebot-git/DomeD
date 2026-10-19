use crate::Dome;
use dome::Realm;
use smithay::{
    backend::input::ButtonState,
    input::pointer::{ButtonEvent, MotionEvent},
    utils::SERIAL_COUNTER,
};
impl Dome {
    pub fn xr_owns_input(&self) -> bool {
        let healthy = self.xr_bridge.lock().unwrap().visible();
        let scene = self.scene.lock().unwrap();
        let panels = scene
            .windows
            .iter()
            .any(|w| !w.minimized && w.workspace == scene.active_workspace);
        self.native_session_active && healthy && self.xr_animation.has_surfaces(panels)
    }
    pub fn sync_input_route(&mut self) {
        let desired = self.xr_owns_input();
        let mut scene = self.scene.lock().unwrap().clone();
        // Application close/minimize fallback must not focus an unmapped XR-only client.
        if !desired
            && !self.input_xr_active
            && scene
                .focused
                .is_some_and(|id| scene.window(id).is_some_and(|w| !w.desktop_visible))
        {
            let stacked = self
                .space
                .elements()
                .rev()
                .filter_map(|w| self.id_for(w.toplevel().unwrap().wl_surface()))
                .find(|id| {
                    scene.window(*id).is_some_and(|w| {
                        w.desktop_visible && !w.minimized && w.workspace == scene.active_workspace
                    })
                });
            let focus =
                dome::input_routing::restore_focus(&scene, stacked.map(Some), Realm::Desktop);
            {
                let mut current = self.scene.lock().unwrap();
                current.focused = focus;
                current.revision += 1;
                scene = current.clone();
            }
            self.applied_revision = u64::MAX;
            self.sync_scene();
        }
        let old = usize::from(self.input_xr_active);
        self.focus_history[old] = Some(scene.focused);
        if desired == self.input_xr_active {
            return;
        }
        let healthy = self.xr_bridge.lock().unwrap().visible();
        let realm = if desired { Realm::Xr } else { Realm::Desktop };
        let remembered = if !desired && !healthy {
            None
        } else {
            self.focus_history[usize::from(desired)]
        };
        let focus = dome::input_routing::restore_focus(&scene, remembered, realm);
        let pointer = self.seat.get_pointer().unwrap();
        let time = self.start_time.elapsed().as_millis() as u32;
        self.ignored_buttons.extend(self.physical_buttons.drain());
        let mut buttons = self
            .desktop_buttons
            .drain()
            .collect::<std::collections::HashSet<_>>();
        if self.xr_shell_pos.is_none() {
            buttons.extend(self.xr_buttons.drain());
        } else {
            self.xr_buttons.clear();
        }
        for button in buttons {
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
        pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), time);
        self.dismiss_wayland_popups();
        self.chrome_drag = None;
        self.xr_target = None;
        self.xr_shell_pos = None;
        self.mouse_angles = None;
        self.xr_bridge.lock().unwrap().mouse_angles = None;
        self.menu = None;
        self.launcher = false;
        self.launch_query.clear();
        self.desktop_manager = false;
        self.task_popup = None;
        self.task_context = false;
        self.about = false;
        self.appearance_open = false;
        self.xr_settings_open = false;
        self.session_confirm = None;
        self.tray.close();
        self.input_xr_active = desired;
        self.input_realm = realm;
        {
            let mut b = self.xr_bridge.lock().unwrap();
            b.packets.clear();
            b.cursor = None;
            b.mouse_buttons.clear();
            b.last_mouse = None;
            b.input_realm = realm;
        }
        {
            let mut s = self.scene.lock().unwrap();
            if s.focused != focus {
                s.focused = focus;
                s.revision += 1;
            }
        }
        self.applied_revision = u64::MAX;
        self.sync_scene();
        let position = self.desktop_pointer.unwrap_or([640., 400.]);
        let position = if desired {
            pointer.current_location()
        } else {
            self.clamp_desktop_pointer((position[0], position[1]).into())
        };
        let under = if desired || self.shell_at(position.x, position.y) {
            None
        } else {
            self.surface_under(position)
        };
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: position,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
    }
}
