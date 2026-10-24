use crate::Dome;
use dome::{Action, Realm, shortcuts::Mods, super_mmb::MIDDLE};
#[derive(Clone,Copy)]
pub enum Gesture { Desktop { id:u64 }, Xr { workspace:usize } }
impl Dome {
    pub fn cancel_super_mmb(&mut self) {
        if let Some(gesture)=self.super_mmb.take() {
            if matches!(gesture,Gesture::Desktop{..}) {self.chrome_drag=None;self.snap_preview=None;}
            self.xr_bridge.lock().unwrap().mouse_buttons.remove(&MIDDLE);
        }
    }
    pub fn super_mmb_button(&mut self,button:u32,pressed:bool,xr:bool)->bool {
        if let Some(gesture)=self.super_mmb {
            if button!=MIDDLE {
                if pressed {self.ignored_buttons.insert(button);}
                return true;
            }
            if !pressed {
                if let Gesture::Desktop{id}=gesture {self.finish_desktop_move(id);self.chrome_drag=None;}
                self.super_mmb=None;
                self.xr_bridge.lock().unwrap().mouse_buttons.remove(&MIDDLE);
                if matches!(gesture,Gesture::Xr{..}) {self.xr_mouse_motion(0.,0.);}
            }
            return true;
        }
        let m=self.seat.get_keyboard().unwrap().modifier_state();
        if !pressed || !dome::super_mmb::starts(button,Mods{logo:m.logo,ctrl:m.ctrl,alt:m.alt,shift:m.shift}) {return false;}
        let pointer=self.seat.get_pointer().unwrap();
        if pointer.is_grabbed() || self.chrome_drag.is_some() || self.chrome_button.is_some()
            || self.shell_modal_open() || self.launcher || self.menu.is_some() || self.desktop_manager
            || self.task_context || self.tray.state().menu.is_some() {return false;}
        if xr {
            let workspace=self.scene.lock().unwrap().active_workspace;
            self.super_mmb=Some(Gesture::Xr{workspace});
            self.xr_bridge.lock().unwrap().mouse_buttons.insert(MIDDLE);
            self.dispatch_xr(dome::xr_input::Packet{events:vec![dome::xr_input::SeatEvent::Leave],actions:vec![]});
            self.mouse_cursor=None;
            self.mouse_angles.get_or_insert([0.,0.]);
            let mut bridge=self.xr_bridge.lock().unwrap();bridge.cursor=None;bridge.mouse_angles=self.mouse_angles;
        } else {
            let p=pointer.current_location();
            if self.popup_under(p).is_some() {return false;}
            let id=self.decoration_at(p.x,p.y).map(|(id,_)|id).or_else(|| {
                if self.shell_at(p.x,p.y){return None;}
                self.space.element_under(p).and_then(|(window,_)|window.toplevel().and_then(|surface|self.id_for(surface.wl_surface())))
            });
            let Some(id)=id else{return false;};
            if self.selection_tooltips.contains_key(&id) {return false;}
            let old={let scene=self.scene.lock().unwrap();scene.window(id).filter(|w|w.visible_in(Realm::Desktop)&&!w.minimized).cloned()};
            let Some(old)=old else{return false;};
            if old.maximized {
                self.action(Action::Maximize{id});
                let rect=self.scene.lock().unwrap().window(id).unwrap().desktop;
                self.action(Action::DesktopMove{id,
                    x:p.x as f32-rect.w*((p.x as f32-old.desktop.x)/old.desktop.w).clamp(0.,1.),
                    y:p.y as f32-rect.h*((p.y as f32-old.desktop.y)/old.desktop.h).clamp(0.,1.)});
            }
            self.action(Action::Focus{id});
            let rect=self.scene.lock().unwrap().window(id).unwrap().desktop;
            self.chrome_drag=Some(crate::decorations::Drag{id,start:[p.x,p.y],rect,resize:false,edges:[false;4]});
            self.super_mmb=Some(Gesture::Desktop{id});
            self.drag_snap_preview(id,p);
        }
        self.cursor_finder=Default::default();
        self.xr_bridge.lock().unwrap().finder_started=None;
        true
    }
    pub fn super_mmb_motion(&mut self,dx:f64)->bool {
        let Some(Gesture::Xr{workspace})=self.super_mmb else{return false;};
        let current=self.scene.lock().unwrap().active_workspace;
        if workspace!=current {self.cancel_super_mmb();self.ignored_buttons.insert(MIDDLE);return true;}
        if let Some(delta)=dome::super_mmb::lane_delta(dx) {
            if delta!=0. {self.action(Action::RotateLanes{workspace,delta_deg:delta});}
        }
        true
    }
}
