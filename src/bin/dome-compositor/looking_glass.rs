//! Input and grouped dock for the natural-placement Looking Glass experiment.
use crate::Dome;
use dome::{looking_glass as lg, Action};
impl Dome {
    pub fn glass_json(&self, _width: i32, _height: i32) -> serde_json::Value {
        if !self.scene.lock().unwrap().looking_glass.enabled {
            return serde_json::json!({"enabled":false});
        }
        let groups: Vec<_> = self
            .task_list()
            .into_iter()
            .map(|task| {
                let previews: Vec<_> = task
                    .windows
                    .iter()
                    .rev()
                    .take(3)
                    .map(|id| self.task_previews.json(*id))
                    .collect();
                let mut group = serde_json::to_value(task).unwrap();
                group["previews"] = serde_json::json!(previews);
                group
            })
            .collect();
        serde_json::json!({"enabled":true,"groups":groups})
    }
    pub fn glass_open(&mut self, id: u64) {
        self.action(Action::GlassWindow {
            id,
            yaw_deg: Some(0.),
            parked: None,
            back: None,
            note: None,
        });
        self.action(Action::Focus { id });
    }
    pub fn glass_click(&mut self, x: f64, y: f64, width: i32, height: i32) -> bool {
        let enabled = self.scene.lock().unwrap().looking_glass.enabled;
        if enabled
            && y >= height as f64 - 60.
            && x >= width as f64 - 250.
            && x < width as f64 - 170.
        {
            self.action(Action::LookingGlass { enabled: false });
            return true;
        }
        false
    }
    pub fn glass_scroll(&mut self, vertical: f64) -> bool {
        let mods = self.seat.get_keyboard().unwrap().modifier_state();
        if !mods.logo
            || mods.ctrl
            || mods.alt
            || mods.shift
            || !vertical.is_finite()
            || vertical == 0.
        {
            return false;
        }
        let scene = self.scene.lock().unwrap().clone();
        if !scene.looking_glass.enabled || scene.native_owner().is_some() {
            return false;
        }
        let pointer = self.seat.get_pointer().unwrap().current_location();
        let target = if self.input_realm == dome::Realm::Xr {
            self.xr_target
                .filter(|id| !dome::xr_transport::is_shell(*id))
                .or(scene.focused)
        } else {
            self.glass_window_at(pointer)
                .or(scene.focused)
        };
        let Some(id) = target
            .map(|id| scene.popup_owner(id))
            .filter(|id| scene.window(*id).is_some())
        else {
            return false;
        };
        let current = scene.looking_glass.cards.get(&id).map_or(0., |c| c.yaw_deg);
        self.action(Action::GlassWindow {
            id,
            yaw_deg: Some((current + vertical as f32 * (10. / 15.)).clamp(-90., 90.)),
            parked: None,
            back: None,
            note: None,
        });
        true
    }
    /// A tilted presentation is activated as a window, not accidentally clicked through.
    pub fn glass_window_at(
        &self,
        point: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) -> Option<u64> {
        let scene = self.scene.lock().unwrap();
        if !scene.looking_glass.enabled {
            return None;
        }
        for window in self.space.elements().rev() {
            let Some(id) = self.id_for(window.toplevel().unwrap().wl_surface()) else {
                continue;
            };
            let Some(w) = scene.window(id) else {
                continue;
            };
            let Some(pos) = self.space.element_location(window) else {
                continue;
            };
            let geo = window.geometry();
            let cap = if w.maximized {
                0
            } else {
                self.appearance.caption_height()
            };
            let border = if w.maximized { 0 } else { 4 };
            let rect = dome::Rect {
                x: (pos.x - border) as f32,
                y: (pos.y - cap) as f32,
                w: (geo.size.w + 2 * border) as f32,
                h: (geo.size.h + cap + border) as f32,
            };
            let tilt = lg::angle(&scene, id);
            if lg::inverse(rect, tilt, [point.x as f32, point.y as f32]).is_some() {
                return Some(id);
            }
        }
        None
    }
    pub fn glass_tilted_at(
        &self,
        point: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) -> Option<u64> {
        self.glass_window_at(point)
            .filter(|id| dome::looking_glass::angle(&self.scene.lock().unwrap(), *id).abs() > 0.01)
    }
    pub fn glass_element_under(
        &self,
        point: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) -> Option<(
        &smithay::desktop::Window,
        smithay::utils::Point<i32, smithay::utils::Logical>,
    )> {
        let enabled = self.scene.lock().unwrap().looking_glass.enabled;
        if !enabled {
            return self.space.element_under(point);
        }
        let id = self.glass_window_at(point)?;
        self.space.elements().find_map(|w| {
            (self.id_for(w.toplevel().unwrap().wl_surface()) == Some(id))
                .then(|| self.space.element_location(w).map(|loc| (w, loc)))
                .flatten()
        })
    }
    pub fn glass_tick(&mut self) {
        let now = self.start_time.elapsed().as_millis() as u64;
        self.scene.lock().unwrap().glass_tick(now);
    }
}
