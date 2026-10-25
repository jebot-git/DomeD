use crate::Dome;
use dome::{
    spatial_popup::{Popup, POPUP_ID_BASE},
    Rect,
};
use smithay::{
    desktop::{
        utils::{bbox_from_surface_tree, under_from_surface_tree},
        PopupKind, PopupManager, WindowSurfaceType,
    },
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::{Logical, Point},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

impl Dome {
    pub fn popup_work_area(&self) -> Rect {
        let geo = self.shell_geometry();
        Rect {
            x: geo.loc.x as f32,
            y: geo.loc.y as f32 + 36.,
            w: geo.size.w as f32,
            h: (geo.size.h - 96).max(1) as f32,
        }
    }

    /// Refresh from committed Wayland resources, never from client-controlled IDs.
    pub fn sync_spatial_popups(&mut self) {
        self.cleanup_popup_grab();
        let mut live = Vec::new();
        for (id, window) in &self.registry {
            for (popup, location) in
                PopupManager::popups_for_surface(window.toplevel().unwrap().wl_surface())
            {
                if bbox_from_surface_tree(popup.wl_surface(), (0, 0)).is_empty() {
                    continue;
                }
                let existing = self
                    .spatial_popups
                    .iter()
                    .find(|(_, p)| p.wl_surface() == popup.wl_surface());
                let popup_id = existing.map(|(id, _)| *id).unwrap_or_else(|| {
                    self.next_popup_id += 1;
                    POPUP_ID_BASE + self.next_popup_id
                });
                live.push((popup_id, *id, popup, location));
            }
        }
        self.ime_popups.retain(|p| p.alive());
        for popup in &self.ime_popups {
            let Some(parent) = popup.get_parent() else {
                continue;
            };
            let Some(owner) = self.id_for(&parent.surface) else {
                continue;
            };
            if bbox_from_surface_tree(popup.wl_surface(), (0, 0)).is_empty() {
                continue;
            }
            let id = if let Some((id, _)) = self
                .spatial_popups
                .iter()
                .find(|(_, p)| p.wl_surface() == popup.wl_surface())
            {
                *id
            } else {
                self.next_popup_id += 1;
                POPUP_ID_BASE + self.next_popup_id
            };
            let r = popup.text_input_rectangle();
            live.push((
                id,
                owner,
                PopupKind::InputMethod(popup.clone()),
                (r.loc.x, r.loc.y + r.size.h).into(),
            ));
        }
        self.selection_tooltips.retain(|id,_|self.registry.iter().any(|(i,_)|i==id));
        let mut parents: Vec<(u64, u64)> = self
            .registry
            .iter()
            .filter_map(|(id, w)| {
                let parent = with_states(w.toplevel().unwrap().wl_surface(), |s| {
                    s.data_map
                        .get::<XdgToplevelSurfaceData>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .parent
                        .clone()
                })?;
                let parent = self.id_for(&parent)?;
                (parent != *id).then_some((*id, parent))
            })
            .collect();
        for(id,anchor)in &self.selection_tooltips {
            if let Some(parent)=anchor.parent.filter(|p|self.registry.iter().any(|(i,_)|i==p)) {
                parents.retain(|(child,_)|child!=id);parents.push((*id,parent));
            }
        }
        let root_for = |id: u64| -> Option<u64> {
            let mut root = id;
            for _ in 0..=parents.len() {
                if let Some((_, parent)) = parents.iter().find(|(child, _)| *child == root) {
                    root = *parent;
                } else {
                    return Some(root);
                }
            }
            None // Ignore invalid cyclic transient-parent chains.
        };
        let mut descriptors = Vec::new();
        for (id, parent) in &parents {
            let Some(root) = root_for(*id) else { continue };
            let window = &self.registry.iter().find(|(i, _)| i == id).unwrap().1;
            let geo = window.geometry();
            if geo.is_empty() {
                continue;
            }
            let mut scene = self.scene.lock().unwrap();
            let Some(owner) = scene.window(root).cloned() else {
                continue;
            };
            if let Some(w) = scene.windows.iter_mut().find(|w| w.id == *id) {
                w.workspace = owner.workspace;
                w.desktop_visible = owner.desktop_visible;
                w.xr_visible = owner.xr_visible;
                w.desktop.w = geo.size.w as f32;
                w.desktop.h = geo.size.h as f32;
            }
            // Transient toplevels have no xdg_positioner. Capture the cursor
            // once so the fallback follows the parent rather than pointer motion.
            let origin = scene.popups.iter().find(|p| p.id == *id)
                .map(|p| [p.anchor.x, p.anchor.y])
                .or_else(||self.selection_tooltips.get(id).map(|a|a.offset))
                .unwrap_or_else(|| {
                    let cursor = self.seat.get_pointer().unwrap().current_location();
                    let parent_rect = scene.window(*parent).map_or(owner.desktop, |w| w.desktop);
                    [cursor.x as f32 - parent_rect.x, cursor.y as f32 + 8. - parent_rect.y]
                });
            descriptors.push(Popup {
                rollout: false,
                context_at: self.selection_tooltips.get(id).and_then(|a|a.point),
                viewport: None,
                id: *id,
                parent: *parent,
                root,
                anchor: Rect {
                    x: origin[0],
                    y: origin[1],
                    w: geo.size.w as f32,
                    h: geo.size.h as f32,
                },
            });
        }
        for (id, owner, popup, location) in &live {
            let Some(root) = root_for(*owner) else {
                continue;
            };
            let parent_surface = match popup {
                PopupKind::Xdg(p) => p.get_parent_surface(),
                _ => None,
            };
            let parent_popup = parent_surface
                .and_then(|surface| live.iter().find(|(_, _, p, _)| p.wl_surface() == &surface));
            let (parent, offset) = parent_popup
                .map(|(id, _, _, loc)| (*id, *loc))
                .unwrap_or((*owner, (0, 0).into()));
            let mut geo = popup_geometry(popup);
            if geo.is_empty() {
                geo = bbox_from_surface_tree(popup.wl_surface(), (0, 0));
            }
            let context = self
                .context_menus
                .iter()
                .find(|m| m.surface == *popup.wl_surface());
            let owner = self
                .scene
                .lock()
                .unwrap()
                .window(root)
                .map(|w| (w.desktop, w.maximized));
            let owner_rect = owner.map(|(rect, _)| rect);
            let work_height = if self.input_xr_active {
                672.
            } else {
                owner_rect
                    .map(|r| {
                        self.output_geometry_at(
                            (r.x as f64 + r.w as f64 / 2., r.y as f64 + r.h as f64 / 2.).into(),
                        )
                        .size
                        .h - 96
                    })
                    .unwrap_or(704)
                    .max(1) as f32
            };
            let mut anchor = Rect {
                x: (location.x - offset.x) as f32,
                y: (location.y - offset.y) as f32,
                w: geo.size.w as f32,
                h: geo.size.h as f32,
            };
            // Input-method popups have no xdg configure. Their cursor rectangle
            // is an origin hint; apply DomeD placement here, independently of
            // xdg popups whose configured geometry must be used unchanged.
            if matches!(popup, PopupKind::InputMethod(_)) {
                if let Some(parent) = owner_rect {
                    let output = self.output_geometry_at(
                        (
                            parent.x as f64 + parent.w as f64 / 2.,
                            parent.y as f64 + parent.h as f64 / 2.,
                        )
                            .into(),
                    );
                    let work = Rect {
                        x: output.loc.x as f32,
                        y: output.loc.y as f32 + 36.,
                        w: output.size.w as f32,
                        h: (output.size.h - 96).max(1) as f32,
                    };
                    let parent = if owner.is_some_and(|(_, maximized)| maximized) {
                        work
                    } else {
                        parent
                    };
                    let placed = if self.input_realm == dome::Realm::Xr {
                        Rect {
                            x: parent.x + (parent.w - anchor.w) / 2.,
                            y: parent.y - anchor.h - 8.,
                            ..anchor
                        }
                    } else {
                        dome::spatial_popup::desktop_rect(parent, Some([anchor.x, anchor.y]), [parent.x, parent.y], [anchor.w, anchor.h], work)
                    };
                    anchor.x = placed.x - parent.x;
                    anchor.y = placed.y - parent.y;
                }
            }
            descriptors.push(Popup {
                rollout: context.is_none() && matches!(popup, PopupKind::Xdg(_)),
                context_at: context.map(|m| m.point),
                viewport: context.map(|m| {
                    dome::context_menu::Viewport::new(geo.size.h as f32, work_height, m.offset)
                }),
                id: *id,
                parent,
                root,
                anchor,
            });
        }
        self.spatial_popups = live.into_iter().map(|(id, _, p, _)| (id, p)).collect();
        let mut scene = self.scene.lock().unwrap();
        if scene.popups != descriptors {
            scene.popups = descriptors;
            scene.revision += 1;
        }
        let dismiss = self.spatial_popups.iter().any(|(id, popup)| {
            if matches!(popup, PopupKind::InputMethod(_)) {
                return false;
            }
            scene.popup(*id).is_some_and(|p| {
                !scene.popup_visible(p, !self.input_xr_active)
                    || scene.focused.map(|id| scene.popup_owner(id)) != Some(p.root)
            })
        });
        let valid: Vec<_> = scene
            .windows
            .iter()
            .map(|w| w.id)
            .chain(scene.popups.iter().map(|p| p.id))
            .collect();
        let scrollable: Vec<_> = scene
            .popups
            .iter()
            .filter(|p| p.viewport.is_some_and(|v| v.band() > 0.))
            .map(|p| p.id)
            .collect();
        drop(scene);
        let mut bridge = self.xr_bridge.lock().unwrap();
        for id in scrollable {
            bridge.requested.insert(id, std::time::Instant::now());
        }
        bridge
            .requested
            .retain(|id, _| valid.contains(id) || dome::xr_transport::is_shell(*id));
        bridge
            .frames
            .retain(|id, _| valid.contains(id) || dome::xr_transport::is_shell(*id));
        drop(bridge);
        self.context_menus.retain(|m| m.surface.is_alive());
        if dismiss {
            self.dismiss_wayland_popups();
        } else {
            self.focus_context_menu();
        }
    }

    pub fn dismiss_wayland_popups(&mut self) {
        for (_, popup) in &self.spatial_popups {
            if matches!(popup, PopupKind::InputMethod(_)) {
                continue;
            }
            if let Ok(root) = smithay::desktop::find_popup_root_surface(popup) {
                let _ = PopupManager::dismiss_popup(&root, popup);
            }
        }
        use smithay::wayland::input_method::InputMethodSeat;
        if let Some(keyboard) = self
            .seat
            .get_keyboard()
            .filter(|k| k.is_grabbed() && !self.seat.input_method().keyboard_grabbed())
        {
            keyboard.unset_grab(self);
        }
        if let Some(pointer) = self.seat.get_pointer().filter(|p| p.is_grabbed()) {
            pointer.unset_grab(
                self,
                smithay::utils::SERIAL_COUNTER.next_serial(),
                self.start_time.elapsed().as_millis() as u32,
            );
        }
        self.applied_revision = u64::MAX;
        self.popup_grab = None;
    }

    pub fn cleanup_popup_grab(&mut self) {
        if !self.popup_grab.as_ref().is_some_and(|g| g.has_ended()) {
            return;
        }
        self.popup_grab = None;
        if let Some(pointer) = self.seat.get_pointer().filter(|p| {
            p.with_grab(|_, g| g.is::<smithay::desktop::PopupPointerGrab<Self>>())
                .unwrap_or(false)
        }) {
            pointer.unset_grab(
                self,
                smithay::utils::SERIAL_COUNTER.next_serial(),
                self.start_time.elapsed().as_millis() as u32,
            );
        }
        if let Some(keyboard) = self.seat.get_keyboard().filter(|k| {
            k.with_grab(|_, g| g.is::<smithay::desktop::PopupKeyboardGrab<Self>>())
                .unwrap_or(false)
        }) {
            keyboard.unset_grab(self);
            let focused = self.scene.lock().unwrap().focused;
            let surface = focused
                .and_then(|id| self.registry.iter().find(|(i, _)| *i == id))
                .map(|(_, w)| w.toplevel().unwrap().wl_surface().clone());
            keyboard.set_focus(self, surface, smithay::utils::SERIAL_COUNTER.next_serial());
        }
        self.applied_revision = u64::MAX;
    }

    pub fn popup_surface(
        &self,
        id: u64,
    ) -> Option<(WlSurface, smithay::utils::Rectangle<i32, Logical>)> {
        if let Some((_, popup)) = self.spatial_popups.iter().find(|(i, _)| *i == id) {
            let mut geo = popup_geometry(popup);
            if geo.is_empty() {
                geo = bbox_from_surface_tree(popup.wl_surface(), (0, 0));
            }
            return Some((popup.wl_surface().clone(), geo));
        }
        let (_, window) = self.registry.iter().find(|(i, _)| *i == id)?;
        Some((
            window.toplevel().unwrap().wl_surface().clone(),
            window.geometry(),
        ))
    }

    pub fn popup_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(u64, WlSurface, Point<f64, Logical>)> {
        let scene = self.scene.lock().unwrap();
        for (id, rect) in self.desktop_popup_rects(&scene).into_iter().rev() {
            if pos.x < rect.x as f64
                || pos.x >= (rect.x + rect.w) as f64
                || pos.y < rect.y as f64
                || pos.y >= (rect.y + rect.h) as f64
            {
                continue;
            }
            let viewport = scene.popup(id).and_then(|p| p.viewport);
            if viewport.is_some_and(|v| v.control(pos.y as f32 - rect.y).is_some()) {
                return None;
            }
            let shift = viewport.map_or(0., |v| v.offset - v.band());
            let (surface, geo) = self.popup_surface(id)?;
            let origin: Point<i32, Logical> = (
                rect.x.round() as i32 - geo.loc.x,
                rect.y.round() as i32 - geo.loc.y,
            )
                .into();
            if let Some((surface, offset)) = under_from_surface_tree(
                &surface,
                (pos.x, pos.y + shift as f64).into(),
                origin,
                WindowSurfaceType::ALL,
            ) {
                return Some((
                    id,
                    surface,
                    offset.to_f64() - smithay::utils::Point::from((0., shift as f64)),
                ));
            }
        }
        None
    }
}

fn popup_geometry(popup: &PopupKind) -> smithay::utils::Rectangle<i32, Logical> {
    if matches!(popup, PopupKind::InputMethod(_)) {
        bbox_from_surface_tree(popup.wl_surface(), (0, 0))
    } else {
        popup.geometry()
    }
}
