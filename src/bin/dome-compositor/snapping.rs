use crate::Dome;
use smithay::utils::{Logical, Point};
impl Dome {
    pub fn drag_snap_preview(&mut self, id: u64, p: Point<f64, Logical>) {
        self.snap_preview = if self.seat.get_keyboard().unwrap().modifier_state().shift {
            let g = self.output_geometry_at(p);
            let work = dome::Rect {
                x: g.loc.x as f32,
                y: g.loc.y as f32 + 36.,
                w: g.size.w as f32,
                h: (g.size.h - 96) as f32,
            };
            dome::desktop_snap::tile(work, [p.x as f32, p.y as f32], 24.).map(|r| (id, r))
        } else {
            None
        };
    }
    pub fn finish_desktop_move(&mut self, id: u64) {
        let preview = self.snap_preview.take().filter(|(owner, _)| *owner == id);
        if !self.seat.get_keyboard().unwrap().modifier_state().shift {
            return;
        }
        if let Some((_, r)) = preview {
            self.action(dome::Action::DesktopResize {
                id,
                w: r.w - 8.,
                h: r.h - 34.,
            });
            self.action(dome::Action::DesktopMove {
                id,
                x: r.x + 4.,
                y: r.y + 30.,
            });
        }
    }
}
