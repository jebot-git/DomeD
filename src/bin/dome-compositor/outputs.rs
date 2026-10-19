//! Desktop coordinates stay global; shell controls use the containing output's coordinates.
use crate::Dome;
use smithay::utils::{Logical, Point, Rectangle};
impl Dome {
    pub fn output_geometry_at(&self, p: Point<f64, Logical>) -> Rectangle<i32, Logical> {
        self.space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o))
            .min_by(|a, b| distance(*a, p).total_cmp(&distance(*b, p)))
            .unwrap_or_else(|| Rectangle::from_size((1280, 800).into()))
    }
    pub fn clamp_desktop_pointer(&self, p: Point<f64, Logical>) -> Point<f64, Logical> {
        let g = self.output_geometry_at(p);
        (
            p.x.clamp(g.loc.x as f64, (g.loc.x + g.size.w - 1) as f64),
            p.y.clamp(g.loc.y as f64, (g.loc.y + g.size.h - 1) as f64),
        )
            .into()
    }
    pub fn primary_output_geometry(&self) -> Rectangle<i32, Logical> {
        let config = self.scene.lock().unwrap().config.clone();
        self.space
            .outputs()
            .find(|o| {
                config
                    .displays
                    .iter()
                    .any(|d| d.name == o.name() && d.primary)
            })
            .or_else(|| self.space.outputs().next())
            .and_then(|o| self.space.output_geometry(o))
            .unwrap_or_else(|| Rectangle::from_size((1280, 800).into()))
    }
    pub fn shell_geometry(&self) -> Rectangle<i32, Logical> {
        if self.input_realm == dome::Realm::Xr {
            return Rectangle::from_size((1024, 768).into());
        }
        self.output_geometry_at(self.seat.get_pointer().unwrap().current_location())
    }
    pub fn desktop_popup_rects(&self, scene: &dome::Scene) -> Vec<(u64, dome::Rect)> {
        let mut result = Vec::new();
        for popup in &scene.popups {
            let Some(owner) = scene.window(popup.root) else {
                continue;
            };
            let g = self.output_geometry_at(
                (
                    owner.desktop.x as f64 + owner.desktop.w as f64 / 2.,
                    owner.desktop.y as f64 + owner.desktop.h as f64 / 2.,
                )
                    .into(),
            );
            let work = dome::Rect {
                x: g.loc.x as f32,
                y: g.loc.y as f32 + 36.,
                w: g.size.w as f32,
                h: (g.size.h - 96).max(1) as f32,
            };
            if let Some(rect) = scene
                .popup_desktop_rects(work)
                .into_iter()
                .find(|(id, _)| *id == popup.id)
            {
                result.push(rect);
            }
        }
        result
    }
    pub fn recover_output_layout(&mut self) {
        // Bring windows back when their monitor disappears; preserve reachable positions.
        let mut scene = self.scene.lock().unwrap();
        for w in &mut scene.windows {
            let g = self.output_geometry_at((w.desktop.x as f64, w.desktop.y as f64).into());
            let x = w.desktop.x.clamp(
                g.loc.x as f32,
                (g.loc.x + g.size.w - 80).max(g.loc.x) as f32,
            );
            let y = w.desktop.y.clamp(
                (g.loc.y + 66) as f32,
                (g.loc.y + g.size.h - 100).max(g.loc.y + 66) as f32,
            );
            w.desktop.x = x;
            w.desktop.y = y;
        }
        scene.revision += 1;
        drop(scene);
        let p = self.clamp_desktop_pointer(self.seat.get_pointer().unwrap().current_location());
        self.desktop_mouse_motion(p, self.start_time.elapsed().as_millis() as u32);
    }
}
fn distance(g: Rectangle<i32, Logical>, p: Point<f64, Logical>) -> f64 {
    let dx = p.x - p.x.clamp(g.loc.x as f64, (g.loc.x + g.size.w - 1) as f64);
    let dy = p.y - p.y.clamp(g.loc.y as f64, (g.loc.y + g.size.h - 1) as f64);
    dx * dx + dy * dy
}
