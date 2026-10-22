//! Foreign selection tooltips inherit the source's workspace and spatial pose.
use crate::Dome;
use smithay::{
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};
#[derive(Clone, Copy)]
pub struct Anchor {
    pub parent: Option<u64>,
    pub offset: [f32; 2],
    pub point: Option<[f32; 2]>,
}
pub fn translator_role(app: &str) -> bool {
    app == "org.domed.Translator.Selection"
}
impl Dome {
    pub fn selection_tooltip_metadata(&mut self, surface: &WlSurface) {
        let Some(id) = self.id_for(surface) else {
            return;
        };
        let role = with_states(surface, |s| {
            s.data_map
                .get::<XdgToplevelSurfaceData>()
                .unwrap()
                .lock()
                .unwrap()
                .app_id
                .clone()
        });
        if !role.as_deref().is_some_and(translator_role)
            || self.selection_tooltips.contains_key(&id)
        {
            return;
        }
        let previous = self
            .notification_windows
            .previous_focus
            .get(&id)
            .copied()
            .flatten();
        let cursor = self.seat.get_pointer().unwrap().current_location();
        let xr_cursor = self.xr_bridge.lock().unwrap().cursor.clone();
        let mut scene = self.scene.lock().unwrap();
        let parent = previous.filter(|p| *p != id && scene.window(*p).is_some());
        let owner = parent.and_then(|p| scene.window(p)).cloned();
        let offset = owner
            .as_ref()
            .map_or([cursor.x as f32, cursor.y as f32 + 8.], |w| {
                [
                    cursor.x as f32 - w.desktop.x,
                    cursor.y as f32 + 8. - w.desktop.y,
                ]
            });
        let point = if self.input_realm == dome::Realm::Xr {
            owner
                .as_ref()
                .and_then(|w| {
                    xr_cursor
                        .as_ref()
                        .filter(|c| c.id == w.id)
                        .map(|c| [c.uv[0] * w.desktop.w, c.uv[1] * w.desktop.h])
                })
                .or_else(|| owner.as_ref().map(|w| [w.desktop.w / 2., w.desktop.h / 2.]))
        } else {
            None
        };
        self.selection_tooltips.insert(
            id,
            Anchor {
                parent,
                offset,
                point,
            },
        );
        scene.focused = previous;
        if let Some(w) = scene.windows.iter_mut().find(|w| w.id == id) {
            if let Some(owner) = owner {
                w.workspace = owner.workspace;
                w.desktop_visible = owner.desktop_visible;
                w.xr_visible = owner.xr_visible;
            } else {
                w.desktop.x = offset[0];
                w.desktop.y = offset[1];
            }
        }
        scene.revision += 1;
        self.applied_revision = u64::MAX;
        if self.pending_raise == Some(id) {
            self.pending_raise = None;
        }
    }
    pub fn dismiss_selection_tooltips(&self) -> bool {
        let mut found = false;
        for (id, w) in &self.registry {
            if self.selection_tooltips.contains_key(id) {
                w.toplevel().unwrap().send_close();
                found = true;
            }
        }
        found
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_role_is_explicit() {
        assert!(translator_role("org.domed.Translator.Selection"));
        assert!(!translator_role("org.domed.Translator"));
        assert!(!translator_role("arbitrary.tooltip"));
    }
}
