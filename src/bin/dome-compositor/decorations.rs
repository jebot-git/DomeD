use crate::Dome;
use dome::Action;
#[derive(Clone)]
pub struct Drag {
    pub id: u64,
    pub start: [f64; 2],
    pub rect: dome::Rect,
    pub resize: bool,
    pub edges: [bool; 4], // left, top, right, bottom
}
impl Dome {
    pub fn decoration_at(&self, x: f64, y: f64) -> Option<(u64, i32)> {
        if self.popup_under((x, y).into()).is_some() {
            return None;
        }
        let scene = self.scene.lock().unwrap();
        for window in self.space.elements().rev() {
            let id = self.id_for(window.toplevel().unwrap().wl_surface())?;
            let w = scene.window(id)?;
            if w.maximized {
                continue;
            }
            let pos = self.space.element_location(window)?;
            let width = w.desktop.w as i32 + 8;
            let height = w.desktop.h as i32 + 34;
            let (px, py) = (x - pos.x as f64 + 4., y - pos.y as f64 + 30.);
            if px >= 0. && py >= 0. && px < width as f64 && py < height as f64 {
                let hit = crate::qt::hit(width, height, px as i32, py as i32);
                return if hit == 0 { None } else { Some((id, hit)) };
            }
        }
        None
    }
    pub fn chrome_press(&mut self, x: f64, y: f64) -> bool {
        if self.popup_under((x,y).into()).is_some() { return false; }
        let Some((id, hit)) = self.decoration_at(x, y) else {
            return false;
        };
        if matches!(hit, 1 | 3 | 6) {
            self.chrome_button = Some((id, hit, true));
            return true;
        }
        self.action(Action::Focus { id });
        match hit {
            _ => {
                let rect = self.scene.lock().unwrap().window(id).unwrap().desktop;
                self.chrome_drag = Some(Drag {
                    id,
                    start: [x, y],
                    rect,
                    resize: hit == 5,
                    edges: [
                        x < rect.x as f64,
                        y < (rect.y - 26.) as f64,
                        x >= (rect.x + rect.w) as f64,
                        y >= (rect.y + rect.h) as f64,
                    ],
                });
            }
        }
        true
    }
    pub fn chrome_motion(&mut self, x: f64, y: f64) -> bool {
        let Some(d) = self.chrome_drag.clone() else {
            return false;
        };
        let dx = (x - d.start[0]) as f32;
        let dy = (y - d.start[1]) as f32;
        if d.resize {
            let [left, top, right, bottom] = d.edges;
            let w = (d.rect.w
                + if left {
                    -dx
                } else if right {
                    dx
                } else {
                    0.
                })
            .clamp(160., 4096.);
            let h = (d.rect.h
                + if top {
                    -dy
                } else if bottom {
                    dy
                } else {
                    0.
                })
            .clamp(100., 4096.);
            self.action(Action::DesktopResize { id: d.id, w, h });
            if left || top {
                self.action(Action::DesktopMove {
                    id: d.id,
                    x: if left {
                        d.rect.x + d.rect.w - w
                    } else {
                        d.rect.x
                    },
                    y: if top {
                        (d.rect.y + d.rect.h - h)
                            .max(self.output_geometry_at((x, y).into()).loc.y as f32 + 66.)
                    } else {
                        d.rect.y
                    },
                });
            }
        } else {
            self.drag_snap_preview(d.id, (x, y).into());
            self.action(Action::DesktopMove {
                id: d.id,
                x: d.rect.x + dx,
                y: (d.rect.y + dy).max(self.output_geometry_at((x, y).into()).loc.y as f32 + 66.),
            });
        }
        true
    }
}

#[derive(Clone, Copy)]
pub struct ZoomState {
    normal: dome::Rect,
    zoom: dome::Rect,
    pub zoomed: bool,
}
impl Dome {
    fn activate_without_raising(&mut self, id: u64) {
        if let Err(error) = self.scene.lock().unwrap().apply(Action::Focus { id }) {
            self.shell_error = error;
        }
        self.no_raise_focus = Some(id);
        self.pending_raise = None;
    }
    pub fn chrome_button_motion(&mut self, x: f64, y: f64) -> bool {
        let Some((id, hit, _)) = self.chrome_button else {
            return false;
        };
        let inside = self.decoration_at(x, y) == Some((id, hit));
        self.chrome_button = Some((id, hit, inside));
        true
    }
    pub fn chrome_button_release(&mut self, x: f64, y: f64) {
        let Some((id, hit, _)) = self.chrome_button.take() else {
            return;
        };
        if self.decoration_at(x, y) != Some((id, hit)) {
            return;
        }
        match hit {
            1 => self.action(Action::Close { id }),
            3 => self.workbench_zoom_window(id),
            6 => self.workbench_depth(id),
            _ => (),
        }
    }
    fn workbench_zoom_window(&mut self, id: u64) {
        let Some(rect) = self.scene.lock().unwrap().window(id).map(|w| w.desktop) else {
            return;
        };
        let g = self.output_geometry_at(
            (
                rect.x as f64 + rect.w as f64 / 2.,
                rect.y as f64 + rect.h as f64 / 2.,
            )
                .into(),
        );
        let preset = dome::Rect {
            x: g.loc.x as f32 + 4.,
            y: g.loc.y as f32 + 66.,
            w: (g.size.w - 8).max(160) as f32,
            h: (g.size.h - 130).max(100) as f32,
        };
        let state = self.workbench_zoom.entry(id).or_insert(ZoomState {
            normal: rect,
            zoom: preset,
            zoomed: false,
        });
        let target = if state.zoomed {
            state.zoom = rect;
            state.normal
        } else {
            state.normal = rect;
            state.zoom
        };
        state.zoomed = !state.zoomed;
        self.activate_without_raising(id);
        self.action(Action::DesktopResize {
            id,
            w: target.w,
            h: target.h,
        });
        self.action(Action::DesktopMove {
            id,
            x: target.x,
            y: target.y,
        });
    }
    fn workbench_depth(&mut self, id: u64) {
        let windows: Vec<_> = self.space.elements().cloned().collect();
        let Some(window) = windows
            .iter()
            .find(|w| self.id_for(w.toplevel().unwrap().wl_surface()) == Some(id))
        else {
            return;
        };
        let front = windows.last() == Some(window);
        let lower = front || self.seat.get_keyboard().unwrap().modifier_state().shift;
        self.activate_without_raising(id);
        if lower {
            // Raising the remaining elements in order lowers this window without
            // unmapping its surfaces or losing output membership/frame callbacks.
            for other in &windows {
                if other != window {
                    self.space.raise_element(other, false);
                }
            }
        } else {
            self.space.raise_element(window, false);
        }
    }
}
