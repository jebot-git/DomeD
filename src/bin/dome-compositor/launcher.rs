//! Launcher selection, scrolling and compositor-owned cursor-key repeat.
use crate::state::Dome;
use smithay::backend::input::Keycode;
use std::time::{Duration, Instant};

pub const ROWS: usize = 8;
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_INTERVAL: Duration = Duration::from_millis(40);

#[derive(Default)]
pub struct Navigation {
    pub quick_focus: Option<usize>,
    pub quick_offset: usize,
    pub quick_drag: Option<(Vec<String>, String)>,
    pub offset: usize,
    pub navigated: bool,
    pub error: String,
    pub repeat: Option<(Keycode, isize, Instant)>,
}
impl Navigation {
    fn reveal(&mut self, index: usize, total: usize) {
        self.offset = self.offset.min(total.saturating_sub(ROWS));
        if index < self.offset {
            self.offset = index;
        } else if index >= self.offset + ROWS {
            self.offset = index + 1 - ROWS;
        }
    }
    fn move_by(&mut self, index: &mut usize, total: usize, step: isize) {
        *index = if total == 0 {
            0
        } else {
            (*index as isize + step).rem_euclid(total as isize) as usize
        };
        self.navigated = true;
        self.error.clear();
        self.reveal(*index, total);
    }
}

impl Dome {
    pub fn open_launcher(&mut self) {
        self.cancel_quick_drag();
        self.clock.open = false;
        self.apps = dome::service::apps();
        self.launcher = true;
        self.launch_query.clear();
        self.launch_index = 0;
        self.launch_navigation = Navigation::default();
        match dome::quick_launch::Pins::load(&dome::quick_launch::path()) {
            Ok(pins) => self.quick_launch = pins,
            Err(error) => self.launch_navigation.error = format!("Quick launch: {error}"),
        }
    }
    pub fn launcher_key(&mut self, code: Keycode, sym: u32, character: Option<char>, text: bool, shift: bool) {
        self.cancel_quick_drag();
        self.launch_navigation.repeat = None;
        match sym {
            0xff1b => self.launcher = false,
            0xff09 | 0xfe20 => {
                let count=self.quick_launch.desktop_ids.len();
                if count>0 {
                    let step=if shift||sym==0xfe20 {-1} else {1};
                    let index=self.launch_navigation.quick_focus.map_or(if step<0 {count-1}else{0},|i|(i as isize+step).rem_euclid(count as isize) as usize);
                    self.focus_quick(index);
                }
            }
            0xff08 => {
                self.launch_query.pop();
                self.launch_index = 0;
                self.launch_navigation = Navigation::default();
            }
            0xff52 | 0xff51 | 0xff54 | 0xff53 => {
                let step = if matches!(sym, 0xff52 | 0xff51) {
                    -1
                } else {
                    1
                };
                self.launch_navigation.quick_focus = None;
                let total = self.matching_apps().len();
                self.launch_navigation
                    .move_by(&mut self.launch_index, total, step);
                self.launch_navigation.repeat = Some((code, step, Instant::now() + REPEAT_DELAY));
            }
            0xff50 | 0xff57 => {
                self.launch_navigation.quick_focus = None;
                let total = self.matching_apps().len();
                self.launch_index = if sym == 0xff50 {
                    0
                } else {
                    total.saturating_sub(1)
                };
                self.launch_navigation.navigated = true;
                self.launch_navigation.error.clear();
                self.launch_navigation.reveal(self.launch_index, total);
            }
            0xff0d | 0xff8d => self.launch_selected(),
            _ if text => {
                if let Some(c) = character.filter(|c| !c.is_control()) {
                    if self.launch_query.len() + c.len_utf8() <= 256 {
                        self.launch_query.push(c);
                        self.launch_index = 0;
                        self.launch_navigation = Navigation::default();
                    }
                }
            }
            _ => (),
        }
    }
    pub fn repeat_launcher_key(&mut self) {
        let Some((code, step, next)) = self.launch_navigation.repeat else {
            return;
        };
        if !self.launcher || !self.native_session_active || !self.shell_keys.contains(&code) {
            self.launch_navigation.repeat = None;
            return;
        }
        let now = Instant::now();
        if now >= next {
            let total = self.matching_apps().len();
            self.launch_navigation
                .move_by(&mut self.launch_index, total, step);
            // Do not burst through queued repeats after a slow frame or session switch.
            self.launch_navigation.repeat = Some((code, step, now + REPEAT_INTERVAL));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_navigation_reaches_every_row_and_wraps_in_both_directions() {
        let mut navigation = Navigation::default();
        let mut index = 0;
        for expected in (1..25).chain([0]) {
            navigation.move_by(&mut index, 25, 1);
            assert_eq!(index, expected);
            assert!(index >= navigation.offset && index < navigation.offset + ROWS);
        }
        navigation.move_by(&mut index, 25, -1);
        assert_eq!((index, navigation.offset), (24, 17));
        navigation.move_by(&mut index, 0, 1);
        assert_eq!((index, navigation.offset), (0, 0));
        navigation.move_by(&mut index, 1, -1);
        assert_eq!((index, navigation.offset), (0, 0));
    }
}

impl Dome {
    fn focus_quick(&mut self,index:usize){
        self.launch_navigation.quick_focus=Some(index);self.launch_navigation.error.clear();
        dome::quick_launch::reveal(index,&mut self.launch_navigation.quick_offset,self.quick_launch.desktop_ids.len());
    }
    fn store_quick(&mut self,next:dome::quick_launch::Pins){
        match next.save(&dome::quick_launch::path()) {
            Ok(())=>{self.quick_launch=next;self.launch_navigation.quick_offset=self.launch_navigation.quick_offset.min(self.quick_launch.desktop_ids.len().saturating_sub(dome::quick_launch::COLUMNS));self.launch_navigation.error.clear();},
            Err(e)=>self.launch_navigation.error=format!("Cannot save quick launch: {e}"),
        }
    }
    pub fn launch_quick(&mut self,index:usize){
        let result=self.quick_launch.desktop_ids.get(index)
            .and_then(|id|self.apps.iter().find(|app|&app.desktop_id==id && app.visible))
            .ok_or_else(||"Pinned application is no longer installed".to_owned())
            .and_then(|app|dome::service::launch_focused(app,&self.launch_socket(),&self.scene));
        self.finish_launcher_launch(result);
    }
    pub fn finish_launcher_launch(&mut self,result:Result<(),String>){
        if let Err(error)=result {self.launch_navigation.error=error;self.launch_navigation.repeat=None;return;}
        self.launcher=false;self.launch_query.clear();self.launch_index=0;self.launch_navigation=Default::default();
    }
    /// Shared by desktop, physical XR mouse and synthetic controller-pointer input.
    pub fn launcher_button(&mut self,x:f64,y:f64,button:u32,pressed:bool)->bool{
        if !pressed && button==0x110 && self.launch_navigation.quick_drag.is_some(){
            self.finish_quick_drag(self.launcher&&(20. ..620.).contains(&x)&&(392. ..456.).contains(&y));return true;
        }
        if !self.launcher{return false;}
        if !pressed{return false;}
        if self.launch_navigation.quick_drag.is_some(){return true;}
        let shift=self.seat.get_keyboard().is_some_and(|k|k.modifier_state().shift);
        if let Some(index)=dome::quick_launch::hit(x,y,self.launch_navigation.quick_offset,self.quick_launch.desktop_ids.len()){
            if button==0x111 && shift {
                let mut next=self.quick_launch.clone();next.remove(index);self.store_quick(next);self.launch_navigation.quick_focus=None;
            }else if button==0x110{
                self.focus_quick(index);
                if shift {self.launch_navigation.quick_drag=Some((self.quick_launch.desktop_ids.clone(),self.quick_launch.desktop_ids[index].clone()));}
                else {self.launch_quick(index);}
            }
            return true;
        }
        if (20. ..620.).contains(&x)&&(104. ..392.).contains(&y){
            if button==0x110{
                let index=self.launch_navigation.offset+((y-104.)/36.) as usize;
                if let Some(app)=self.matching_apps().get(index){
                    if shift {let id=app.desktop_id.clone();let mut next=self.quick_launch.clone();match next.pin(&id){Ok(true)=>self.store_quick(next),Ok(false)=>(),Err(e)=>self.launch_navigation.error=e};if let Some(pin)=self.quick_launch.desktop_ids.iter().position(|p|p==&id){self.focus_quick(pin);}}
                    else {self.launch_navigation.quick_focus=None;self.launch_index=index;self.launch_navigation.navigated=true;self.launch_selected();}
                }
            }
            return true;
        }
        if !(20. ..620.).contains(&x)||!(56. ..456.).contains(&y){self.cancel_quick_drag();self.launcher=false;}
        true
    }
    pub fn launcher_motion(&mut self,x:f64,y:f64){
        use dome::quick_launch::{LEFT,CELL,COLUMNS};
        if !self.launcher{return;}
        let Some((_,id))=self.launch_navigation.quick_drag.as_ref()else{return;};
        if !(392. ..456.).contains(&y){return;}
        let Some(from)=self.quick_launch.desktop_ids.iter().position(|p|p==id)else{return;};
        let offset=&mut self.launch_navigation.quick_offset;
        if x>=LEFT+CELL*COLUMNS as f64 {*offset=(*offset+1).min(self.quick_launch.desktop_ids.len().saturating_sub(COLUMNS));}
        else if x<LEFT {*offset=offset.saturating_sub(1);}
        let slot=((x-LEFT)/CELL).floor().max(0.).min((COLUMNS-1) as f64) as usize;
        let to=self.quick_launch.move_to(from,*offset+slot);self.focus_quick(to);
    }
    pub fn finish_quick_drag(&mut self,commit:bool){
        if let Some((original,id))=self.launch_navigation.quick_drag.take(){
            let desired=self.quick_launch.clone();self.quick_launch.desktop_ids=original;
            if commit && desired!=self.quick_launch {self.store_quick(desired);}
            if let Some(index)=self.quick_launch.desktop_ids.iter().position(|p|p==&id){let error=self.launch_navigation.error.clone();self.focus_quick(index);self.launch_navigation.error=error;}
        }
    }
    pub fn cancel_quick_drag(&mut self){self.finish_quick_drag(false);}
    pub fn launcher_scroll(&mut self,x:f64,y:f64,delta:f64)->bool{
        if !self.launcher{return false;}
        if delta!=0. && (20. ..620.).contains(&x)&&(392. ..456.).contains(&y){
            let offset=&mut self.launch_navigation.quick_offset;
            *offset=if delta>0.{offset.saturating_add(1)}else{offset.saturating_sub(1)}.min(self.quick_launch.desktop_ids.len().saturating_sub(dome::quick_launch::COLUMNS));
            self.launch_navigation.quick_focus=None;
        }
        true
    }
    pub fn quick_launch_json(&self)->serde_json::Value{
        use dome::quick_launch::{LEFT,TOP,CELL,COLUMNS};
        let offset=self.launch_navigation.quick_offset.min(self.quick_launch.desktop_ids.len().saturating_sub(COLUMNS));
        let selected=self.launch_navigation.quick_focus;
        let items:Vec<_>=self.quick_launch.desktop_ids.iter().enumerate().skip(offset).take(COLUMNS).map(|(index,id)|{
            let app=self.apps.iter().find(|app|&app.desktop_id==id&&app.visible);
            serde_json::json!({"index":index,"desktop_id":id,"name":app.map(|a|a.name.as_str()).unwrap_or(id),"icon":app.map(|a|a.icon.as_str()).unwrap_or("application-x-executable"),"enabled":app.is_some()})
        }).collect();
        let name=selected.and_then(|i|self.quick_launch.desktop_ids.get(i)).map(|id|self.apps.iter().find(|a|&a.desktop_id==id).map(|a|a.name.as_str()).unwrap_or(id));
        serde_json::json!({"items":items,"selected":selected,"offset":offset,"total":self.quick_launch.desktop_ids.len(),"name":name,"x":LEFT,"y":TOP,"cell":CELL,"columns":COLUMNS,"dragging":self.launch_navigation.quick_drag.is_some()})
    }
}
