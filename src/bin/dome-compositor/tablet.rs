//! Native tablet tools keep their surface while pressed; invalid owners leave proximity.
use std::collections::HashMap;
use crate::Dome;
use smithay::{
    backend::input::{ButtonState, InputBackend, TabletToolDescriptor, TabletToolEvent},
    input::pointer::MotionEvent,
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::tablet_manager::{TabletDescriptor, TabletHandle, TabletSeatHandler, TabletSeatTrait, TabletToolHandle},
};

#[derive(Default)]
pub struct TabletState {
    devices: HashMap<TabletDescriptor, TabletHandle>,
    tools: HashMap<TabletToolDescriptor, Tool>,
}
struct Tool {
    handle: TabletToolHandle,
    device: TabletDescriptor,
    owner: Option<Owner>,
    pressed: bool,
}
#[derive(Clone)]
struct Owner { surface: WlSurface, origin: Point<f64, Logical>, id: u64, geometry: [f64;4], maximized: bool }

#[derive(Default, Clone, Copy)]
pub struct Axes {
    pub pressure: Option<f64>,
    pub distance: Option<f64>,
    pub tilt: Option<(f64,f64)>,
    pub rotation: Option<f64>,
    pub slider: Option<f64>,
    pub wheel: Option<(f64,i32)>,
}
impl Axes {
    fn queue(self, tool: &TabletToolHandle) {
        if let Some(v)=self.pressure.filter(|v|v.is_finite()) {tool.pressure(v.clamp(0.,1.));}
        if let Some(v)=self.distance.filter(|v|v.is_finite()) {tool.distance(v.clamp(0.,1.));}
        if let Some((x,y))=self.tilt.filter(|(x,y)|x.is_finite()&&y.is_finite()) {tool.tilt((x.clamp(-90.,90.),y.clamp(-90.,90.)));}
        if let Some(v)=self.rotation.filter(|v|v.is_finite()) {tool.rotation(v.rem_euclid(360.));}
        if let Some(v)=self.slider.filter(|v|v.is_finite()) {tool.slider_position(v.clamp(-1.,1.));}
        if let Some((v,n))=self.wheel.filter(|(v,_)|v.is_finite()) {tool.wheel(v,n);}
    }
}

impl TabletSeatHandler for Dome {}
smithay::delegate_tablet_manager!(Dome);

impl Dome {
    pub fn tablet_add(&mut self, device: TabletDescriptor) {
        if self.tablets.devices.contains_key(&device) || self.tablets.devices.len()>=32 {return;}
        let handle=self.seat.tablet_seat().add_tablet::<Self>(&self.display_handle,&device);
        self.tablets.devices.insert(device,handle);
    }
    pub fn tablet_remove(&mut self, device: &TabletDescriptor, time: u32) {
        let descriptors: Vec<_>=self.tablets.tools.iter().filter(|(_,t)|&t.device==device).map(|(d,_)|d.clone()).collect();
        for descriptor in descriptors {
            self.tablet_out(&descriptor,time);
            self.tablets.tools.remove(&descriptor);
            self.seat.tablet_seat().remove_tool(&descriptor);
        }
        self.tablets.devices.remove(device);
        self.seat.tablet_seat().remove_tablet(device);
    }
    pub fn tablet_out(&mut self, descriptor: &TabletToolDescriptor, time: u32) {
        if let Some(tool)=self.tablets.tools.get_mut(descriptor) {
            tool.owner=None;
            tool.handle.proximity_out(time);
            tool.pressed=false;
        }
    }
    fn tablet_owner_valid(&self, owner: &Owner) -> bool {
        if self.session.locked() || self.input_xr_active || !owner.surface.is_alive() {return false;}
        let scene=self.scene.lock().unwrap();
        scene.window(owner.id).is_some_and(|w|!scene.locked && w.desktop_visible && !w.minimized &&
            w.workspace==scene.active_workspace && (scene.presentation.desktop || scene.native_owner()==Some(owner.id)) &&
            owner.geometry==[w.desktop.x as f64,w.desktop.y as f64,w.desktop.w as f64,w.desktop.h as f64] && owner.maximized==w.maximized)
    }
    pub fn validate_tablets(&mut self) {
        let invalid:Vec<_>=self.tablets.tools.iter().filter(|(_,t)|t.owner.as_ref().is_some_and(|o|!self.tablet_owner_valid(o)))
            .map(|(d,_)|d.clone()).collect();
        let time=self.start_time.elapsed().as_millis() as u32;
        for descriptor in invalid {self.tablet_out(&descriptor,time);}
    }
    fn tablet_target(&self, point: Point<f64,Logical>) -> Option<Owner> {
        if self.session.locked() || self.input_xr_active || self.shell_at(point.x,point.y) {return None;}
        let (id,surface,origin)=self.popup_under(point).or_else(|| {
            let (surface,origin)=self.surface_under(point)?;
            let (window,_)=self.space.element_under(point)?;
            Some((self.id_for(window.toplevel()?.wl_surface())?,surface,origin))
        })?;
        let scene=self.scene.lock().unwrap();
        let w=scene.window(id)?;
        if scene.locked || !w.desktop_visible || w.minimized || w.workspace!=scene.active_workspace ||
            (!scene.presentation.desktop && scene.native_owner()!=Some(id)) {return None;}
        Some(Owner {surface,origin,id,geometry:[w.desktop.x as f64,w.desktop.y as f64,w.desktop.w as f64,w.desktop.h as f64],maximized:w.maximized})
    }
    pub fn tablet_in(&mut self, device: TabletDescriptor, descriptor: TabletToolDescriptor, point: Point<f64,Logical>, axes: Axes, time: u32) {
        self.tablet_add(device.clone());
        if !self.tablets.devices.contains_key(&device) {return;}
        self.tablet_out(&descriptor,time);
        if !self.tablets.tools.contains_key(&descriptor) {
            if self.tablets.tools.len()>=32 {return;}
            let dh=self.display_handle.clone();
            let handle=self.seat.tablet_seat().add_tool(self,&dh,&descriptor);
            self.tablets.tools.insert(descriptor.clone(),Tool {handle,device:device.clone(),owner:None,pressed:false});
        }
        self.tablets.tools.get_mut(&descriptor).unwrap().device=device;
        self.tablet_motion(&descriptor,point,axes,time);
    }
    pub fn tablet_motion(&mut self, descriptor: &TabletToolDescriptor, point: Point<f64,Logical>, axes: Axes, time: u32) {
        self.validate_tablets();
        if !point.x.is_finite() || !point.y.is_finite() {self.tablet_out(descriptor,time);return;}
        let Some(tool)=self.tablets.tools.get(descriptor) else {return;};
        let owner=if tool.pressed {tool.owner.clone()} else {self.tablet_target(point)};
        let Some(tablet)=self.tablets.devices.get(&tool.device).cloned() else {return;};
        let tool=self.tablets.tools.get_mut(descriptor).unwrap();
        // Keep all axis updates paired with motion in the same protocol frame.
        // A new proximity frame precedes axes, so flush once more after entry.
        let entering=owner.as_ref().map(|o|&o.surface)!=tool.owner.as_ref().map(|o|&o.surface);
        let focus=owner.as_ref().map(|o|(o.surface.clone(),o.origin));
        if entering {tool.handle.motion(point,focus.clone(),&tablet,SERIAL_COUNTER.next_serial(),time);}
        axes.queue(&tool.handle);
        tool.handle.motion(point,focus,&tablet,SERIAL_COUNTER.next_serial(),time);
        tool.owner=owner;
        if let Some(pointer)=self.seat.get_pointer().filter(|pointer|!pointer.is_grabbed()) {
            // Update DomeD's cursor position without synthesizing mouse motion in
            // the tablet client. An existing physical mouse grab keeps its own
            // position and events. Tablet input has its own Wayland protocol.
            pointer.motion(self,None,&MotionEvent {location:point,serial:SERIAL_COUNTER.next_serial(),time});
            pointer.frame(self);
        }
    }
    pub fn tablet_tip(&mut self, descriptor: &TabletToolDescriptor, down: bool, time: u32) {
        self.validate_tablets();
        let Some(tool)=self.tablets.tools.get(descriptor) else {return;};
        if tool.pressed==down {return;}
        let Some(owner)=tool.owner.clone() else {return;};
        let handle=tool.handle.clone();
        self.tablets.tools.get_mut(descriptor).unwrap().pressed=down;
        if down {
            self.session_interacted();
            self.action(dome::Action::Focus {id:owner.id});
            if let Some(keyboard)=self.seat.get_keyboard() {keyboard.set_focus(self,Some(owner.surface),SERIAL_COUNTER.next_serial());}
            handle.tip_down(SERIAL_COUNTER.next_serial(),time);
        } else {handle.tip_up(time);}
    }
    pub fn tablet_button(&mut self, descriptor: &TabletToolDescriptor, button: u32, state: ButtonState, time: u32) {
        self.validate_tablets();
        if let Some(tool)=self.tablets.tools.get(descriptor) {tool.handle.button(button,state,SERIAL_COUNTER.next_serial(),time);}
    }
    pub fn tablet_axes<I:InputBackend,E:TabletToolEvent<I>>(&mut self,event:&E) {
        let geometry=self.desktop_geometry();
        self.tablet_motion(&event.tool(),event.position_transformed(geometry.size)+geometry.loc.to_f64(),Axes {
            pressure:event.pressure_has_changed().then(||event.pressure()),
            distance:event.distance_has_changed().then(||event.distance()),
            tilt:event.tilt_has_changed().then(||event.tilt()),
            rotation:event.rotation_has_changed().then(||event.rotation()),
            slider:event.slider_has_changed().then(||event.slider_position()),
            wheel:event.wheel_has_changed().then(||(event.wheel_delta(),event.wheel_delta_discrete())),
        },event.time_msec());
    }
}
