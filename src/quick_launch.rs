//! Ordered launcher pins use stable XDG desktop IDs, never catalog indices or commands.
use serde::{Deserialize, Serialize};
use std::{io::Write, path::{Path, PathBuf}};
pub const CAPACITY: usize = 128;
pub const COLUMNS: usize = 11;
pub const LEFT: f64 = 36.;
pub const TOP: f64 = 400.;
pub const CELL: f64 = 48.;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pins { pub desktop_ids: Vec<String> }
impl Pins {
    fn validate(&self) -> Result<(), String> {
        if self.desktop_ids.len() > CAPACITY { return Err("Too many quick-launch items".into()); }
        let mut seen = std::collections::HashSet::new();
        for id in &self.desktop_ids {
            if id.is_empty() || id.len() > 512 || id.chars().any(|c| c.is_control() || c == '/' || c == '\\') || !id.ends_with(".desktop") || !seen.insert(id) {
                return Err("Invalid or duplicate quick-launch desktop ID".into());
            }
        }
        Ok(())
    }
    pub fn pin(&mut self, id: &str) -> Result<bool, String> {
        if self.desktop_ids.iter().any(|old| old == id) { return Ok(false); }
        let mut next = self.clone();next.desktop_ids.push(id.into());next.validate()?;*self=next;Ok(true)
    }
    pub fn remove(&mut self, index: usize) { if index < self.desktop_ids.len() { self.desktop_ids.remove(index); } }
    pub fn move_to(&mut self, from: usize, to: usize) -> usize {
        if from >= self.desktop_ids.len() { return from; }
        let to=to.min(self.desktop_ids.len()-1);let id=self.desktop_ids.remove(from);self.desktop_ids.insert(to,id);to
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let file=match std::fs::File::open(path) { Ok(f)=>f,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(Self::default()),Err(e)=>return Err(e.to_string()) };
        if file.metadata().map_err(|e| e.to_string())?.len()>128*1024 { return Err("Quick-launch settings exceed 128 KiB".into()); }
        let pins:Self=serde_json::from_reader(file).map_err(|e| e.to_string())?;pins.validate()?;Ok(pins)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        use std::os::unix::fs::OpenOptionsExt;
        self.validate()?;let parent=path.parent().ok_or("No settings directory")?;std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
        let temp=path.with_extension(format!("{}.tmp",std::process::id()));
        let result=(|| -> Result<(), String> {
            let mut file=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp).map_err(|e|e.to_string())?;
            file.write_all(&serde_json::to_vec_pretty(self).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            file.sync_all().map_err(|e|e.to_string())?;std::fs::rename(&temp,path).map_err(|e|e.to_string())?;Ok(())
        })();if result.is_err(){let _=std::fs::remove_file(temp);}result
    }
}
pub fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(||PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")).join("dome/launcher.json")
}
pub fn hit(x:f64,y:f64,offset:usize,count:usize)->Option<usize>{
    if !(LEFT..LEFT+CELL*COLUMNS as f64).contains(&x)||!(TOP..TOP+40.).contains(&y){return None;}
    let index=offset+((x-LEFT)/CELL).floor() as usize;(index<count).then_some(index)
}
pub fn reveal(index:usize,offset:&mut usize,count:usize){
    *offset=(*offset).min(count.saturating_sub(COLUMNS));if index<*offset{*offset=index;}else if index>=*offset+COLUMNS{*offset=index+1-COLUMNS;}
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn stable_ids_survive_ordering_removal_and_atomic_reload(){
        let root=std::env::temp_dir().join(format!("dome-pins-{}",std::process::id()));let file=root.join("launcher.json");let mut p=Pins::default();
        assert!(p.pin("a.desktop").unwrap());assert!(!p.pin("a.desktop").unwrap());p.pin("b.desktop").unwrap();p.pin("c.desktop").unwrap();
        p.move_to(2,0);p.remove(1);p.save(&file).unwrap();assert_eq!(Pins::load(&file).unwrap().desktop_ids,["c.desktop","b.desktop"]);
        let old=std::fs::read(&file).unwrap();assert!(p.pin("/bin/invalid.desktop").is_err());assert_eq!(std::fs::read(&file).unwrap(),old);
        std::fs::write(&file,b"{broken").unwrap();assert!(Pins::load(&file).is_err());std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn grid_hit_testing_and_keyboard_reveal_agree_for_overflow(){
        let mut offset=0;for index in 0..40 {reveal(index,&mut offset,40);let x=LEFT+(index-offset) as f64*CELL+CELL/2.;assert_eq!(hit(x,TOP+20.,offset,40),Some(index));}
        reveal(0,&mut offset,40);assert_eq!(offset,0);assert_eq!(hit(LEFT-1.,TOP,0,40),None);assert_eq!(hit(LEFT,TOP+40.,0,40),None);
    }
}
