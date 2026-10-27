//! Native client-surface touch. A sequence keeps its original Wayland focus.
use crate::Dome;
use smithay::{
    backend::input::TouchSlot,
    input::touch::{DownEvent, MotionEvent, UpEvent},
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::{Logical, Point, SERIAL_COUNTER},
};
use std::collections::HashSet;

#[derive(Default)]
pub struct TouchState {
    slots: HashSet<TouchSlot>,
    owner: Option<Owner>,
    frame_pending: bool,
}
struct Owner {
    surface: WlSurface,
    id: u64,
    geometry: [f64; 4],
    maximized: bool,
}
impl Dome {
    pub fn cancel_touches(&mut self) {
        if self.touch.slots.is_empty() && !self.touch.frame_pending { return; }
        self.touch.slots.clear();
        self.touch.owner = None;
        self.touch.frame_pending = false;
        if let Some(touch) = self.seat.get_touch() { touch.cancel(self); }
    }
    pub fn validate_touches(&mut self) {
        if self.touch.slots.is_empty() { return; }
        let valid = !self.session.locked() && !self.input_xr_active && self.touch.owner.as_ref().is_none_or(|owner| {
            if !owner.surface.is_alive() { return false; }
            let scene = self.scene.lock().unwrap();
            scene.window(owner.id).is_some_and(|window| {
                !scene.locked && window.desktop_visible && !window.minimized && window.workspace == scene.active_workspace &&
                (scene.presentation.desktop || scene.native_owner() == Some(owner.id)) &&
                owner.geometry == [window.desktop.x as f64,window.desktop.y as f64,window.desktop.w as f64,window.desktop.h as f64] &&
                owner.maximized == window.maximized
            })
        });
        if !valid { self.cancel_touches(); }
    }
    pub fn touch_down(&mut self, slot: TouchSlot, point: Point<f64, Logical>, time: u32) {
        self.validate_touches();
        if self.session.locked() || !point.x.is_finite() || !point.y.is_finite() { return; }
        if self.touch.slots.len() >= 32 || self.touch.slots.contains(&slot) { self.cancel_touches();return; }
        if self.touch.slots.is_empty() {
            self.session_interacted();
            self.input_focus_xr = Some(false);
            self.sync_input_route();
            self.input_realm = dome::Realm::Desktop;
        }
        let mut focus = None;
        if self.touch.slots.is_empty() && !self.shell_at(point.x, point.y) {
            let under = self.popup_under(point).or_else(|| {
                let (surface, origin) = self.surface_under(point)?;
                let (window, _) = self.space.element_under(point)?;
                let id = self.id_for(window.toplevel()?.wl_surface())?;
                Some((id,surface,origin))
            });
            if let Some((id,surface,origin)) = under {
                let allowed = {
                    let scene = self.scene.lock().unwrap();
                    scene.window(id).filter(|w| !w.minimized && w.desktop_visible && w.workspace == scene.active_workspace &&
                        (scene.presentation.desktop || scene.native_owner()==Some(id)))
                        .map(|w|([w.desktop.x as f64,w.desktop.y as f64,w.desktop.w as f64,w.desktop.h as f64],w.maximized))
                };
                if let Some((geometry,maximized)) = allowed {
                    self.action(dome::Action::Focus { id });
                    if let Some(keyboard)=self.seat.get_keyboard() {
                        keyboard.set_focus(self,Some(surface.clone()),SERIAL_COUNTER.next_serial());
                    }
                    self.touch.owner=Some(Owner {surface:surface.clone(),id,geometry,maximized});
                    focus=Some((surface,origin));
                }
            }
        }
        self.touch.slots.insert(slot);
        self.touch.frame_pending = true;
        if let Some(touch)=self.seat.get_touch() {
            touch.down(self,focus,&DownEvent {slot,location:point,serial:SERIAL_COUNTER.next_serial(),time});
        }
    }
    pub fn touch_motion(&mut self, slot: TouchSlot, point: Point<f64, Logical>, time: u32) {
        self.validate_touches();
        if !self.touch.slots.contains(&slot) { return; }
        if !point.x.is_finite() || !point.y.is_finite() { self.cancel_touches();return; }
        self.touch.frame_pending = true;
        if let Some(touch)=self.seat.get_touch() {
            touch.motion(self,None,&MotionEvent {slot,location:point,time});
        }
    }
    pub fn touch_up(&mut self, slot: TouchSlot, time: u32) {
        self.validate_touches();
        if !self.touch.slots.remove(&slot) { return; }
        self.touch.frame_pending = true;
        if let Some(touch)=self.seat.get_touch() { touch.up(self,&UpEvent {slot,serial:SERIAL_COUNTER.next_serial(),time}); }
        if self.touch.slots.is_empty() { self.touch.owner=None; }
    }
    pub fn touch_frame(&mut self) {
        self.validate_touches();
        if let Some(touch)=self.seat.get_touch() { touch.frame(self); }
        self.touch.frame_pending = false;
    }
}
