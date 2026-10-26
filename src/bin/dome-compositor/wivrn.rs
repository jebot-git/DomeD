//! WayVR control and leased laser input. Called exclusively on the seat thread.
use crate::Dome;
use dome::{
    wivrn::{Command, Frame, LEASE},
    xr_input::{Packet, SeatEvent},
    Action, Realm,
};
impl Dome {
    pub fn mark_explicit_stop(&self) {
        if self.tray.integration.lock().unwrap().owns_bus {
            dome::wivrn::explicit_stop();
        }
    }
    pub fn release_wivrn_pointer(&mut self) {
        let buttons = {
            let mut c = self.tray.integration.lock().unwrap();
            let b = c.buttons;
            let target = c.target;
            c.buttons = 0;
            c.hand = None;
            c.armed = false;
            c.target = None;
            c.hits = [None; 2];
            (b, target)
        };
        if buttons.0 != 0 || buttons.1.is_some() {
            let mut events: Vec<_> = (0..3)
                .filter(|i| buttons.0 & (1 << i) != 0)
                .map(|i| SeatEvent::Button {
                    button: [272, 274, 273][i],
                    pressed: false,
                })
                .collect();
            events.push(SeatEvent::Leave);
            self.dispatch_xr(Packet {
                events,
                actions: vec![],
            });
        }
    }
    pub fn poll_wivrn(&mut self) {
        let (commands, frames, expired) = {
            let mut c = self.tray.integration.lock().unwrap();
            let p = self.scene.lock().unwrap().presentation;
            let backend = c
                .status
                .get("backend")
                .and_then(|v| v.as_str())
                .unwrap_or("nested")
                .to_owned();
            c.status = serde_json::json!({"version":1,"ready":true,"backend":backend,"xr_active":self.xr_bridge.lock().unwrap().visible(),"shown":p.desktop||p.top||p.bottom,"notifications":if self.notifications.owns_bus(){"domed"}else{"host"},"pointer_protocol":1});
            (
                c.commands.drain(..).collect::<Vec<_>>(),
                c.frames.drain(..).collect::<Vec<_>>(),
                c.received.is_some_and(|t| t.elapsed() > LEASE),
            )
        };
        if expired || self.session.locked() || !self.xr_owns_input() {
            self.release_wivrn_pointer();
        }
        let notices = self
            .tray
            .integration
            .lock()
            .unwrap()
            .notices
            .drain(..)
            .collect::<Vec<_>>();
        for notice in notices {
            self.notifications.forward(notice);
        }
        for command in commands {
            if command == Command::Stop {
                dome::wivrn::explicit_stop();
                self.save_session_on_exit();
                self.loop_signal.stop();
                continue;
            }
            if self.session.locked() {
                continue;
            }
            let before = self.input_realm;
            self.input_realm = Realm::Xr;
            match command {
                Command::ToggleDesktop => {
                    let p = self.scene.lock().unwrap().presentation;
                    let show = !(p.desktop || p.top || p.bottom);
                    if show && !self.xr_bridge.lock().unwrap().visible() {
                        self.start_xr();
                    }
                    self.action(Action::XrPresentation {
                        top: Some(show),
                        bottom: Some(show),
                        desktop: Some(show),
                    });
                }
                Command::Launcher => {
                    self.action(Action::XrPresentation {
                        top: Some(true),
                        bottom: None,
                        desktop: None,
                    });
                    self.open_launcher();
                }
                Command::Screenshot => self.start_screenshot(),
                Command::Terminal => {
                    if let Err(e) = dome::terminal::toggle(&self.launch_socket()) {
                        self.shell_error = e.to_string();
                    }
                }
                Command::Stop => unreachable!(),
            }
            self.input_realm = before;
        }
        for (owner, frame) in frames {
            if !frame.valid(dome::wivrn::monotonic_ns()) {
                continue;
            }
            let changed = {
                let c = self.tray.integration.lock().unwrap();
                c.owner != owner
            };
            if changed {
                self.release_wivrn_pointer();
                let mut c = self.tray.integration.lock().unwrap();
                c.owner = owner;
                c.last_sequence = 0;
            }
            {
                let mut c = self.tray.integration.lock().unwrap();
                if frame.sequence <= c.last_sequence {
                    continue;
                }
                c.last_sequence = frame.sequence;
                c.sequence = frame.sequence;
                c.received = Some(std::time::Instant::now());
            }
            self.wivrn_frame(frame);
        }
    }
    fn wivrn_frame(&mut self, frame: Frame) {
        let blocked = {
            let b = self.xr_bridge.lock().unwrap();
            !b.visible()
                || !b.mouse_buttons.is_empty()
                || b.last_mouse.is_some_and(|t| t.elapsed().as_millis() < 350)
        };
        if blocked || self.session.locked() || !self.xr_owns_input() {
            self.release_wivrn_pointer();
            return;
        }
        let (session, layout) = {
            let b = self.xr_bridge.lock().unwrap();
            (b.session.clone(), b.shell.clone())
        };
        let scene = self.scene.lock().unwrap().clone();
        let mut quads = dome::xr_input::quads_in_fov(&scene, session.head, session.mouse_viewport);
        if let Some(view) = session.mouse_viewport {
            quads.extend(dome::xr_input::shell_quads_in_fov(
                &scene,
                session.head,
                &layout,
                view,
            ));
        }
        quads.retain(|q| self.xr_animation.interactive(scene.presentation, q.id));
        let (owner, buttons, target, armed) = {
            let c = self.tray.integration.lock().unwrap();
            (c.hand, c.buttons, c.target, c.armed)
        };
        let hits = std::array::from_fn::<_, 2, _>(|i| {
            let r = &frame.rays[i];
            if !r.tracked {
                return None;
            }
            let origin = glam::Vec3::from_array(session.head.transform(r.origin));
            let direction = glam::Vec3::from_array(session.head.rotate(r.direction));
            dome::xr_input::mouse_target_with_ui(
                &scene,
                &quads,
                origin,
                direction,
                if owner == Some(i) && buttons != 0 {
                    target
                } else {
                    None
                },
                self.shell_modal_open(),
            )
            .filter(|h| buttons != 0 && owner == Some(i) || h.2 < r.max_distance)
        });
        self.tray.integration.lock().unwrap().hits = hits.map(|h| h.map(|v| v.2));
        let hand = owner
            .filter(|i| buttons != 0 && frame.rays[*i].tracked)
            .or_else(|| {
                (0..2)
                    .filter(|i| hits[*i].is_some())
                    .min_by(|a, b| hits[*a].unwrap().2.total_cmp(&hits[*b].unwrap().2))
            });
        let Some(hand) = hand else {
            self.release_wivrn_pointer();
            return;
        };
        let r = &frame.rays[hand];
        let Some((id, uv, _)) = hits[hand] else {
            self.release_wivrn_pointer();
            return;
        };
        // A newly acquired ray must observe neutral before it can press anything.
        let enabled = armed && owner == Some(hand);
        let now = if enabled { r.buttons } else { 0 };
        let mut events = vec![SeatEvent::Motion { id, uv }];
        let mut actions = vec![];
        for i in 0..3 {
            if (now ^ buttons) & (1 << i) != 0 {
                let pressed = now & (1 << i) != 0;
                if pressed && !dome::xr_transport::is_shell(id) {
                    let focused = if scene.window(id).is_some() {
                        id
                    } else {
                        scene.popup_owner(id)
                    };
                    actions.push(Action::Focus { id: focused });
                }
                events.push(SeatEvent::Button {
                    button: [272, 274, 273][i],
                    pressed,
                });
            }
        }
        if enabled && r.scroll != [0., 0.] {
            events.push(SeatEvent::Scroll {
                horizontal: r.scroll[0],
                vertical: r.scroll[1],
            });
        }
        {
            let mut b = self.xr_bridge.lock().unwrap();
            b.last_mouse = None;
            b.mouse_angles = None;
            b.last_input = None;
            b.cursor = Some(dome::xr_transport::Cursor { id, uv });
        }
        self.mouse_angles = None;
        self.dispatch_xr(Packet { events, actions });
        let mut c = self.tray.integration.lock().unwrap();
        c.hand = Some(hand);
        c.buttons = now;
        c.target = Some(id);
        c.armed = enabled || r.buttons == 0;
    }
}
