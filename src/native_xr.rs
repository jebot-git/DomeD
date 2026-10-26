//! Reversible native OpenXR handoff. Lease data is session-only, never persisted.
use crate::{Scene, WindowId, xr_presentation::Presentation};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::{Duration, Instant}};
#[derive(Clone, Debug)]
pub struct Snapshot {
    presentation: Presentation, workspace: usize, focus: Option<WindowId>,
    desktop: bool, xr: bool,
}
#[derive(Clone, Debug)]
pub struct Lease {
    pub owner: WindowId, pid: u32, token: String, pub active: bool,
    snapshot: Snapshot, heartbeat: Instant,
}
#[derive(Clone, Debug, Default)]
pub struct Coordinator {
    pub lease: Option<Lease>, launches: HashMap<WindowId, Snapshot>,
    pub desktop_requested: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub op: String, pub pid: u32, pub token: String,
    #[serde(default)] pub id: Option<WindowId>,
    #[serde(default)] pub desktop: bool,
}
#[derive(Debug, Serialize)]
pub struct Status { pub version: u32, pub compositor_pid: u32, pub active: bool, pub suspended: bool, pub owner: Option<WindowId> }
impl Scene {
    fn native_snapshot(&self, id: WindowId, focus: Option<WindowId>) -> Result<Snapshot,String> {
        let w=self.window(id).ok_or("Unknown native XR window")?;
        Ok(Snapshot{presentation:self.presentation,workspace:self.active_workspace,focus,
            desktop:w.desktop_visible,xr:w.xr_visible})
    }
    pub fn native_record_launch(&mut self,id:WindowId,previous:Option<WindowId>) {
        if let Ok(mut snapshot)=self.native_snapshot(id,previous) {
            if self.native_owner().is_some() {
                let (presentation,workspace,focus)=self.native_saved_context();
                snapshot.presentation=presentation;snapshot.workspace=workspace;snapshot.focus=focus;
            }
            self.native_xr.launches.insert(id,snapshot);
        }
    }
    pub fn native_owner(&self)->Option<WindowId> {
        self.native_xr.lease.as_ref().filter(|l|l.active).map(|l|l.owner)
    }
    pub fn native_saved_context(&self)->(Presentation,usize,Option<WindowId>) {
        self.native_xr.lease.as_ref().filter(|l|l.active).map_or(
            (self.presentation,self.active_workspace,self.focused),
            |l|(l.snapshot.presentation,l.snapshot.workspace,l.snapshot.focus))
    }
    pub fn native_saved_window(&self,w:&crate::Window)->crate::Window {
        let mut saved=w.clone();
        if let Some(l)=self.native_xr.lease.as_ref().filter(|l|l.active && l.owner==w.id) {
            saved.desktop_visible=l.snapshot.desktop;saved.xr_visible=l.snapshot.xr;
        }
        saved
    }
    pub fn native_status(&self)->Status {
        let l=self.native_xr.lease.as_ref();
        Status{version:1,compositor_pid:std::process::id(),active:l.is_some_and(|l|l.active),suspended:l.is_some_and(|l|!l.active),owner:l.map(|l|l.owner)}
    }
    fn native_restore(&mut self,lease:&Lease,desktop:bool) {
        self.presentation=lease.snapshot.presentation;
        self.active_workspace=lease.snapshot.workspace.min(self.workspaces.len().saturating_sub(1));
        if let Some(w)=self.windows.iter_mut().find(|w|w.id==lease.owner) {
            w.desktop_visible=if desktop {true}else{lease.snapshot.desktop};
            // Keep a minimized taskbar entry in XR so the user can return.
            w.xr_visible=!desktop;
            w.minimized=!desktop;
        }
        self.focused=if desktop {Some(lease.owner)}else{lease.snapshot.focus.filter(|id|self.window(*id).is_some_and(|w|!w.minimized&&w.workspace==self.active_workspace))};
        if self.focused.is_none() { self.focused=self.windows.iter().rev().find(|w|w.id!=lease.owner && !w.minimized && w.workspace==self.active_workspace).map(|w|w.id); }
        self.native_xr.desktop_requested|=desktop;
        self.revision+=1;
    }
    pub fn native_suspend(&mut self) {
        if let Some(mut lease)=self.native_xr.lease.take() {
            if lease.active { self.native_restore(&lease,false);lease.active=false; }
            self.native_xr.lease=Some(lease);
        }
    }
    pub fn native_request(&mut self,r:Request)->Result<Status,String> {
        if self.locked {return Err("Session is locked".into());}
        if r.token.len()<24 || r.token.len()>128 || !r.token.bytes().all(|b|b.is_ascii_hexdigit()) {
            return Err("Invalid native XR lease token".into());
        }
        if r.op=="begin" {
            let id=r.id.ok_or("Missing native XR window")?;
            let w=self.window(id).ok_or("Unknown native XR window")?;
            if w.client_pid!=r.pid || r.pid==0 {return Err("Native XR owner does not match Wayland peer".into());}
            let visibility=(w.desktop_visible,w.xr_visible);
            let other=self.native_xr.lease.as_ref().is_some_and(|l|l.pid!=r.pid || l.token!=r.token || l.owner!=id);
            if other && self.native_owner().is_some() {return Err("Another native XR experience owns the session".into());}
            if other {self.native_xr.lease=None;}
            if self.native_owner()==Some(id) {return Ok(self.native_status());}
            let snapshot=if let Some(old)=self.native_xr.lease.take() {
                let mut s=self.native_snapshot(id,old.snapshot.focus)?;
                s.desktop=old.snapshot.desktop;s.xr=old.snapshot.xr;s
            }else{
                let mut saved=self.native_xr.launches.remove(&id).unwrap_or(self.native_snapshot(id,self.focused.filter(|i|*i!=id))?);
                saved.desktop=visibility.0;saved.xr=visibility.1;saved
            };
            self.native_xr.lease=Some(Lease{owner:id,pid:r.pid,token:r.token,active:true,snapshot,heartbeat:Instant::now()});
            let w=self.windows.iter_mut().find(|w|w.id==id).unwrap();
            w.desktop_visible=true;w.xr_visible=false;w.minimized=false;
            self.active_workspace=w.workspace;self.focused=Some(id);
            self.presentation=Presentation{desktop:false,top:false,bottom:false};self.revision+=1;
        }else{
            let mut lease=self.native_xr.lease.take().ok_or("No native XR lease")?;
            if lease.pid!=r.pid || lease.token!=r.token {
                self.native_xr.lease=Some(lease);return Err("Native XR lease owner mismatch".into());
            }
            lease.heartbeat=Instant::now();
            match r.op.as_str() {
                "heartbeat"=>self.native_xr.lease=Some(lease),
                "suspend"=>{if lease.active {self.native_restore(&lease,false);lease.active=false;}self.native_xr.lease=Some(lease);},
                "end"=>{if lease.active || r.desktop {self.native_restore(&lease,r.desktop);}},
                _=>{self.native_xr.lease=Some(lease);return Err("Unknown native XR lease operation".into());}
            }
        }
        Ok(self.native_status())
    }
    pub fn native_tick(&mut self) {
        self.native_xr.launches.retain(|id,_|self.windows.iter().any(|w|w.id==*id));
        let expired=self.native_xr.lease.as_ref().is_some_and(|l| self.locked
            || l.heartbeat.elapsed()>Duration::from_secs(2)
            || !self.windows.iter().any(|w|w.id==l.owner && w.client_pid==l.pid)
            || !std::path::Path::new(&format!("/proc/{}",l.pid)).exists());
        if expired {if let Some(l)=self.native_xr.lease.take(){if l.active {self.native_restore(&l,false);}}}
        if let Some(owner)=self.native_owner() {
            if self.focused!=Some(owner) {self.focused=Some(owner);self.revision+=1;}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->(Scene,u64,u64) {
        let mut s=Scene::new("test");let old=s.add("Editor","editor");
        s.presentation=Presentation{top:false,bottom:true,desktop:true};
        let game=s.add("Tempest XR","org.de_xr.TempestXR");
        let w=s.windows.iter_mut().find(|w|w.id==game).unwrap();
        w.client_pid=std::process::id();w.desktop_visible=false;w.xr_visible=true;
        s.native_record_launch(game,Some(old));(s,old,game)
    }
    fn req(op:&str,id:u64)->Request {Request{op:op.into(),id:Some(id),pid:std::process::id(),
        token:"1234567890abcdef1234567890abcdef".into(),desktop:false}}
    #[test] fn alt_tab_restore_keeps_desktop_geometry_and_original_bar_visibility() {
        let (mut s,old,game)=fixture();let geometry=s.window(old).unwrap().desktop;let presentation=s.presentation;
        s.native_request(req("begin",game)).unwrap();assert_eq!(s.native_owner(),Some(game));
        assert!(!s.presentation.top&&!s.presentation.bottom&&!s.presentation.desktop);
        assert!(s.window(game).unwrap().desktop_visible&&!s.window(game).unwrap().xr_visible);
        s.native_suspend();assert_eq!(s.focused,Some(old));assert_eq!(s.presentation,presentation);
        assert_eq!(s.window(old).unwrap().desktop,geometry);assert!(s.window(game).unwrap().minimized);
        assert!(s.native_status().suspended);
    }
    #[test] fn resume_and_desktop_toggle_preserve_snapshot_and_release_capture() {
        let (mut s,old,game)=fixture();let presentation=s.presentation;
        s.native_request(req("begin",game)).unwrap();s.native_suspend();
        s.apply(crate::Action::Focus{id:game}).unwrap();s.native_request(req("begin",game)).unwrap();
        assert_eq!(s.native_owner(),Some(game));s.native_suspend();assert_eq!(s.focused,Some(old));
        s.native_request(req("begin",game)).unwrap();let mut end=req("end",game);end.desktop=true;
        s.native_request(end).unwrap();assert_eq!(s.presentation,presentation);assert!(s.native_owner().is_none());
        assert_eq!(s.focused,Some(game));assert!(s.native_xr.desktop_requested);
        let w=s.window(game).unwrap();assert!(w.desktop_visible&&!w.xr_visible&&!w.minimized);
    }
    #[test] fn expired_or_closed_client_cannot_leave_desktop_hidden() {
        let (mut s,old,game)=fixture();let p=s.presentation;s.native_request(req("begin",game)).unwrap();
        s.native_xr.lease.as_mut().unwrap().heartbeat=Instant::now()-Duration::from_secs(3);s.native_tick();
        assert_eq!(s.presentation,p);assert_eq!(s.focused,Some(old));assert!(s.native_xr.lease.is_none());
        s.native_request(req("begin",game)).unwrap();s.apply(crate::Action::Close{id:game}).unwrap();s.native_tick();
        assert_eq!(s.presentation,p);assert!(s.native_xr.lease.is_none());
    }
    #[test] fn peer_pid_and_token_are_checked_and_capture_is_not_persisted() {
        let (mut s,_,game)=fixture();let mut bad=req("begin",game);bad.pid+=1;assert!(s.native_request(bad).is_err());
        s.native_request(req("begin",game)).unwrap();let mut bad=req("end",game);bad.token="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
        assert!(s.native_request(bad).is_err());assert_eq!(s.native_owner(),Some(game));
        let json=serde_json::to_string(&s).unwrap();assert!(!json.contains("1234567890abcdef"));
        let restored:Scene=serde_json::from_str(&json).unwrap();assert!(restored.native_xr.lease.is_none());
        let saved=crate::session_state::Layout::capture(&s,&Default::default());
        assert!(saved.presentation.desktop&&saved.presentation.bottom&&!saved.presentation.top);
        let w=saved.windows.iter().find(|w|w.id==game).unwrap();assert!(!w.desktop_visible&&w.xr_visible);
        assert_ne!(saved.focused,Some(game));
    }
    #[test] fn background_mapping_does_not_steal_game_capture_but_explicit_focus_releases_it() {
        let (mut s,old,game)=fixture();s.native_request(req("begin",game)).unwrap();s.add("Background app","background");
        s.native_tick();assert_eq!(s.focused,Some(game));
        s.apply(crate::Action::Focus{id:old}).unwrap();assert!(s.native_status().suspended);assert!(s.presentation.desktop);
    }
    #[test] fn launching_another_experience_never_saves_the_hidden_native_presentation() {
        let (mut s,old,first)=fixture();let presentation=s.presentation;
        s.native_request(req("begin",first)).unwrap();let second=s.add("Second experience","experience");
        s.windows.iter_mut().find(|w|w.id==second).unwrap().client_pid=std::process::id();
        s.native_record_launch(second,Some(first));
        let mut r=req("begin",second);r.token="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        assert!(s.native_request(r).is_err());s.native_suspend();
        let mut r=req("begin",second);r.token="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        s.native_request(r).unwrap();s.native_suspend();assert_eq!(s.presentation,presentation);assert_eq!(s.focused,Some(old));
    }

}
