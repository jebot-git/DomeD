use crate::Dome;
use smithay::{reexports::wayland_server::protocol::wl_surface::WlSurface, utils::SERIAL_COUNTER};

pub struct Trigger {
    pub surface: WlSurface,
    pub point: [f32; 2],
    pub time: std::time::Instant,
}
pub struct Context {
    pub surface: WlSurface,
    pub point: [f32; 2],
    pub offset: f32,
}
impl Dome {
    /// Called before seat dispatch, so even a popup created on button press sees its origin.
    pub fn context_button_origin(&mut self, button: u32, pressed: bool) {
        if !pressed {
            return;
        }
        self.context_trigger = None;
        if button != 0x111 {
            return;
        }
        let pointer = self.seat.get_pointer().unwrap();
        let target = if self.input_realm == dome::Realm::Xr {
            let cursor = self.xr_bridge.lock().unwrap().cursor.clone();
            cursor.and_then(|c| {
                self.popup_surface(c.id).map(|(_, geo)| {
                    (
                        c.id,
                        [
                            c.uv[0] * geo.size.w as f32,
                            c.uv[1]
                                * self
                                    .scene
                                    .lock()
                                    .unwrap()
                                    .popup(c.id)
                                    .map_or(geo.size.h as f32, |p| p.height()),
                        ],
                    )
                })
            })
        } else {
            let pos = pointer.current_location();
            if let Some((id, _, _)) = self.popup_under(pos) {
                self.desktop_popup_rects(&self.scene.lock().unwrap())
                    .into_iter()
                    .find(|(i, _)| *i == id)
                    .map(|(_, rect)| {
                        (
                            id,
                            [
                                (pos.x - rect.x as f64) as f32,
                                (pos.y - rect.y as f64) as f32,
                            ],
                        )
                    })
            } else {
                self.space.element_under(pos).and_then(|(w, location)| {
                    self.id_for(w.toplevel().unwrap().wl_surface()).map(|id| {
                        (
                            id,
                            [
                                (pos.x - location.x as f64) as f32,
                                (pos.y - location.y as f64) as f32,
                            ],
                        )
                    })
                })
            }
        };
        if let Some((id, mut point)) = target {
            if let Some(p) = self.scene.lock().unwrap().popup(id) {
                if let Some(viewport) = p.viewport {
                    point[1] = viewport.content_y(point[1]);
                }
            }
            if let Some((surface, _)) = self.popup_surface(id) {
                self.context_trigger = Some(Trigger {
                    surface,
                    point,
                    time: std::time::Instant::now(),
                });
            }
        }
    }

    pub fn associate_context_menu(&mut self, surface: &smithay::wayland::shell::xdg::PopupSurface) {
        let Some(parent) = surface.get_parent_surface() else {
            return;
        };
        if self
            .context_trigger
            .as_ref()
            .is_some_and(|t| t.surface == parent && t.time.elapsed().as_secs_f32() <= 1.)
        {
            let trigger = self.context_trigger.take().unwrap();
            self.context_menus.push(Context {
                surface: surface.wl_surface().clone(),
                point: trigger.point,
                offset: 0.,
            });
        }
    }

    pub fn context_focus_surface(&self) -> Option<WlSurface> {
        let scene = self.scene.lock().unwrap();
        let popup = scene
            .popups
            .iter()
            .filter(|p| {
                p.context_at.is_some()
                    && scene.popup_visible(p, !self.input_xr_active)
                    && scene.focused.map(|id| scene.popup_owner(id)) == Some(p.root)
            })
            .max_by_key(|p| p.id)?;
        self.popup_surface(popup.id).map(|(surface, _)| surface)
    }

    pub fn scroll_context(&mut self, id: u64, delta: f32) -> bool {
        let (surface, _) = match self.popup_surface(id) {
            Some(s) => s,
            None => return false,
        };
        let viewport = self
            .scene
            .lock()
            .unwrap()
            .popup(id)
            .and_then(|p| p.viewport);
        let Some(mut viewport) = viewport.filter(|v| v.band() > 0.) else {
            return false;
        };
        viewport.scroll(delta);
        if let Some(menu) = self.context_menus.iter_mut().find(|m| m.surface == surface) {
            menu.offset = viewport.offset;
        }
        self.sync_spatial_popups();
        true
    }

    pub fn context_control_at(&self, id: u64, uv: [f32; 2]) -> Option<i32> {
        let scene = self.scene.lock().unwrap();
        let viewport = scene.popup(id)?.viewport?;
        viewport.control(uv[1] * viewport.height)
    }

    pub fn desktop_context_control(
        &self,
        pos: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) -> Option<(u64, i32)> {
        let scene = self.scene.lock().unwrap();
        for (id, rect) in self.desktop_popup_rects(&scene).into_iter().rev() {
            if pos.x >= rect.x as f64
                && pos.x < (rect.x + rect.w) as f64
                && pos.y >= rect.y as f64
                && pos.y < (rect.y + rect.h) as f64
            {
                return scene
                    .popup(id)?
                    .viewport?
                    .control(pos.y as f32 - rect.y)
                    .map(|dir| (id, dir));
            }
        }
        None
    }

    pub fn focus_context_menu(&mut self) {
        if let Some(surface) = self.context_focus_surface() {
            if let Some(keyboard) = self.seat.get_keyboard().filter(|k| !k.is_grabbed()) {
                keyboard.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
            }
        }
    }
}
