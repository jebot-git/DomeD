//! Family/face/script choices over physical BDF strikes. Saved identities remain
//! the real font IDs, so every client resolves the same bitmap without scaling.
use crate::appearance::Appearance;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BitmapFont {
    pub id: String,
    pub sizes: Vec<u16>,
    #[serde(default)] pub family: String,
    #[serde(default)] pub style: String,
    #[serde(default)] pub file: String,
}
#[derive(Clone, Debug)]
struct Choice {
    id: String, family: String, face: String, script: String, pixels: u16, strike: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Description {
    pub family: String, pub face: String, pub script: String, pub size: String, pub has_scripts: bool,
}
#[derive(Clone, Copy)]
pub enum Control { Family, Face, Size, Script }
fn choices(fonts: &[BitmapFont]) -> Vec<Choice> {
    let mut choices = Vec::new();
    for font in fonts {
        let (raw, style) = font.id.split_once(" / ").unwrap_or((&font.id, "Regular"));
        let raw = if font.family.is_empty() { raw } else { &font.family };
        let raw = raw.split(" [").next().unwrap_or(raw);
        let style = if font.style.is_empty() { style.split(" [").next().unwrap_or(style) } else { &font.style };
        let (mut family, mut face, mut script) = (raw.to_owned(), style.to_owned(), String::new());
        let mut strike = String::new();
        if raw.starts_with("Galmuri") {
            family = "Galmuri".into(); strike = raw.into();
            if raw.starts_with("GalmuriMono") { face = format!("Monospaced {style}"); }
            else if font.file.contains("Condensed") { face = "Condensed".into(); }
        } else if raw.starts_with("Fusion Pixel ") {
            family = "Fusion Pixel".into();
            face = if raw.contains(" Mono ") { "Monospaced" } else { "Proportional" }.into();
            script = match raw.split_whitespace().last().unwrap_or("latin") {
                "ja" => "Japanese", "ko" => "Korean", "zh_Hans" => "Chinese (Simplified)",
                "zh_Hant" => "Chinese (Traditional)", "zh_HK" => "Chinese (Hong Kong)",
                "zh_TW" => "Chinese (Taiwan)", _ => "Latin",
            }.into();
        } else if raw.starts_with("Neep") {
            family = "Neep".into();
            let source = format!("{} {}", font.file, font.id);
            script = if source.contains("iso8859-15-") { "Latin (Western + Euro)" }
                else if source.contains("iso8859-2-") { "Latin (Central European)" }
                else if source.contains("iso8859-9-") { "Latin (Turkish)" }
                else if source.contains("iso8859-1-") { "Latin (Western)" } else { "Unicode" }.into();
            let variant = if raw == "Neep Alt" { "Alternate · " }
                else if source.contains("pre-ampersand") { "Pre-ampersand · " }
                else if source.contains("post-ampersand") { "Post-ampersand · " } else { "" };
            face = format!("{variant}{style}{}", if source.contains("-part-") { " (partial)" } else { "" });
        } else if raw == "x10y12pxDenkiChipHangul" { family = "DenkiChipHangul".into(); }
        else if raw == "5thElement" { family = "5th Element".into(); }
        else if raw.eq_ignore_ascii_case("qub") { family = "QUB".into(); }
        else if raw.starts_with("Kissinger 2") {
            family = "Kissinger2".into();
            face = if raw.ends_with('f') { "Full-width" } else { "Half-width" }.into();
        }
        for &pixels in &font.sizes {
            choices.push(Choice { id: font.id.clone(), family: family.clone(), face: face.clone(),
                script: script.clone(), pixels, strike: strike.clone() });
        }
    }
    choices.sort_by(|a,b| (&a.family,&a.face,&a.script,a.pixels,a.id.len(),&a.id)
        .cmp(&(&b.family,&b.face,&b.script,b.pixels,b.id.len(),&b.id)));
    choices
}
fn current<'a>(choices: &'a [Choice], appearance: &Appearance) -> Option<&'a Choice> {
    choices.iter().filter(|c| c.id == appearance.font)
        .min_by_key(|c| c.pixels.abs_diff(appearance.font_size))
}
pub fn describe(fonts: &[BitmapFont], appearance: &Appearance) -> Description {
    let choices = choices(fonts);
    if let Some(c) = current(&choices, appearance) {
        let scripts: BTreeSet<_> = choices.iter().filter(|v|v.family==c.family).map(|v|&v.script).collect();
        Description { family:c.family.clone(), face:c.face.clone(), script:c.script.clone(),
            size: if c.strike.is_empty() {format!("{} px · native",c.pixels)} else {format!("{} · {} px",c.strike,c.pixels)},
            has_scripts:scripts.len()>1 }
    } else {
        Description { family:appearance.font.clone(), face:String::new(), script:String::new(),
            size:format!("{} px · native",appearance.font_size), has_scripts:false }
    }
}
fn cycle<T: Ord + Clone>(values: impl Iterator<Item=T>, value: &T, direction:i32) -> Option<T> {
    let values:Vec<_>=values.collect::<BTreeSet<_>>().into_iter().collect();
    if values.is_empty() { return None; }
    let index=values.iter().position(|v|v==value).unwrap_or(0);
    Some(values[(index as i32+direction).rem_euclid(values.len()as i32)as usize].clone())
}
pub fn adjust(fonts: &[BitmapFont], appearance: &mut Appearance, control: Control, direction:i32) {
    let choices=choices(fonts);
    let Some(c)=current(&choices,appearance) else { return; };
    let mut family=c.family.clone();let mut face=c.face.clone();let mut script=c.script.clone();let mut size=c.pixels;
    match control {
        Control::Family => { if let Some(next)=cycle(choices.iter().map(|v|v.family.clone()),&family,direction) {family=next;} }
        Control::Face => { if let Some(next)=cycle(choices.iter().filter(|v|v.family==family).map(|v|v.face.clone()),&face,direction) {face=next;} }
        Control::Script => { if let Some(next)=cycle(choices.iter().filter(|v|v.family==family&&v.face==face).map(|v|v.script.clone()),&script,direction) {script=next;} }
        Control::Size => { if let Some(next)=cycle(choices.iter().filter(|v|v.family==family&&v.face==face&&v.script==script).map(|v|v.pixels),&size,direction) {size=next;} }
    }
    let Some(selected)=choices.iter().filter(|v|v.family==family).min_by_key(|v| (
        v.face!=face, v.script!=script,
        !matches!(v.face.as_str(),"Regular"|"Sans-Serif"|"Monospaced"|"Half-width"),
        v.script!="Latin", v.pixels.abs_diff(size), v.id.len())) else { return; };
    appearance.font=selected.id.clone();appearance.font_size=selected.pixels;
}
#[cfg(test)]
mod tests {
    use super::*;
    fn font(family:&str,style:&str,size:u16)->BitmapFont {
        BitmapFont{id:format!("{family} / {style}"),family:family.into(),style:style.into(),sizes:vec![size],file:String::new()}
    }
    #[test]
    fn galmuri_sizes_cross_physical_faces_and_wrap_without_changing_style() {
        let fonts=vec![font("Galmuri9","Regular",12),BitmapFont{family:"Galmuri11 [quiple Galmuri11]".into(),..font("Galmuri11","Regular",17)},font("Galmuri14","Regular",22),font("Galmuri11","Bold",17)];
        let mut a=Appearance::default();a.font=fonts[0].id.clone();a.font_size=12;
        for expected in ["Galmuri11 / Regular","Galmuri14 / Regular","Galmuri9 / Regular"] {
            adjust(&fonts,&mut a,Control::Size,1);assert_eq!(a.font,expected);
        }
        adjust(&fonts,&mut a,Control::Size,-1);assert_eq!(a.font,"Galmuri14 / Regular");
        assert_eq!(describe(&fonts,&a).family,"Galmuri");
        assert!(describe(&fonts,&a).size.starts_with("Galmuri14"));
    }
    #[test]
    fn fusion_scripts_and_sizes_preserve_each_other_and_round_trip() {
        let fonts:Vec<_>=[8,10,12].into_iter().flat_map(|size|["ja","ko","latin"].map(move|script|font(&format!("Fusion Pixel {size}px Mono {script}"),"Sans-Serif",size))).collect();
        let mut a=Appearance::default();a.font="Fusion Pixel 10px Mono ja / Sans-Serif".into();a.font_size=10;
        adjust(&fonts,&mut a,Control::Size,1);assert_eq!(a.font,"Fusion Pixel 12px Mono ja / Sans-Serif");
        adjust(&fonts,&mut a,Control::Script,1);assert_eq!(a.font,"Fusion Pixel 12px Mono ko / Sans-Serif");
        let saved:Appearance=serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        let description=describe(&fonts,&saved);assert_eq!(description.family,"Fusion Pixel");assert_eq!(description.script,"Korean");assert!(description.has_scripts);
        assert!(!describe(&[font("Cozette","Regular",13)],&Appearance{font:"Cozette / Regular".into(),font_size:13,..Default::default()}).has_scripts);
    }
}
