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
        let Some((id, hit)) = self.decoration_at(x, y) else {
            return false;
        };
        self.action(Action::Focus { id });
        match hit {
            1 => self.action(Action::Close { id }),
            2 => self.action(Action::Minimize { id }),
            3 => self.action(Action::Maximize { id }),
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
                        (d.rect.y + d.rect.h - h).max(66.)
                    } else {
                        d.rect.y
                    },
                });
            }
        } else {
            self.action(Action::DesktopMove {
                id: d.id,
                x: d.rect.x + dx,
                y: (d.rect.y + dy).max(66.),
            });
        }
        true
    }
}
