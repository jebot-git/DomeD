//! Family/face choices over physical BDF strikes. Saved identities remain
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
    id: String, family: String, face: String, pixels: u16, strike: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Description {
    pub family: String, pub face: String, pub size: String,
}
#[derive(Clone, Copy)]
pub enum Control { Family, Face, Size }
fn pixel_family(name: &str) -> Option<&str> {
    let (width, rest) = name.strip_prefix('x')?.split_once('y')?;
    let (height, family) = rest.split_once("px")?;
    if !width.is_empty() && !height.is_empty() && !family.is_empty()
        && width.bytes().chain(height.bytes()).all(|c| c.is_ascii_digit()) {
        Some(family)
    } else { None }
}
fn choices(fonts: &[BitmapFont]) -> Vec<Choice> {
    let mut choices = Vec::new();
    for font in fonts {
        let (raw, style) = font.id.split_once(" / ").unwrap_or((&font.id, "Regular"));
        let raw = if font.family.is_empty() { raw } else { &font.family };
        let raw = raw.split(" [").next().unwrap_or(raw);
        let style = if font.style.is_empty() { style.split(" [").next().unwrap_or(style) } else { &font.style };
        let (mut family, mut face) = (raw.to_owned(), style.to_owned());
        let mut strike = String::new();
        if raw.starts_with("Galmuri") {
            family = "Galmuri".into(); strike = raw.into();
            if raw.starts_with("GalmuriMono") { face = format!("Monospaced {style}"); }
            else if font.file.contains("Condensed") { face = "Condensed".into(); }
        } else if ["Fusion Pixel ", "Ark Pixel ", "Jelly Pixel "].iter().any(|prefix| raw.starts_with(prefix)) {
            family = raw.split(" Pixel ").next().unwrap().to_owned() + " Pixel";
            face = if raw.contains(" Mono") { "Monospaced" } else { "Proportional" }.into();
        } else if let Some(variant) = raw.strip_prefix("Retro Pixel ") {
            family = "Retro Pixel".into(); face = variant.into();
        } else if let Some(name) = pixel_family(raw) { family = name.into(); }
        else if raw == "5thElement" { family = "5th Element".into(); }
        else if raw.eq_ignore_ascii_case("qub") { family = "QUB".into(); }
        for &pixels in &font.sizes {
            choices.push(Choice { id: font.id.clone(), family: family.clone(), face: face.clone(),
                pixels, strike: strike.clone() });
        }
    }
    choices.sort_by(|a,b| (&a.family,&a.face,a.pixels,a.id.len(),&a.id)
        .cmp(&(&b.family,&b.face,b.pixels,b.id.len(),&b.id)));
    choices
}
/// Migrate physical Fusion Pixel language variants to the combined strike.
pub fn migrated_id(id: &str) -> String {
    if let Some((family, style)) = id.split_once(" / ") {
        if family.starts_with("Fusion Pixel ") {
            if let Some((base, lang)) = family.rsplit_once(' ') {
                if ["latin", "ja", "ko", "zh_Hans", "zh_Hant", "zh_HK", "zh_TW"].contains(&lang) {
                    return format!("{base} / {style}");
                }
            }
        }
    }
    id.to_owned()
}
fn current<'a>(choices: &'a [Choice], appearance: &Appearance) -> Option<&'a Choice> {
    choices.iter().filter(|c| c.id == migrated_id(&appearance.font))
        .min_by_key(|c| c.pixels.abs_diff(appearance.font_size))
}
pub fn describe(fonts: &[BitmapFont], appearance: &Appearance) -> Description {
    let choices = choices(fonts);
    if let Some(c) = current(&choices, appearance) {
        Description { family:c.family.clone(), face:c.face.clone(),
            size: if c.strike.is_empty() {format!("{} px · native",c.pixels)} else {format!("{} · {} px",c.strike,c.pixels)} }
    } else {
        Description { family:appearance.font.clone(), face:String::new(),
            size:format!("{} px · native",appearance.font_size) }
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
    let mut family=c.family.clone();let mut face=c.face.clone();let mut size=c.pixels;
    match control {
        Control::Family => { if let Some(next)=cycle(choices.iter().map(|v|v.family.clone()),&family,direction) {family=next;} }
        Control::Face => { if let Some(next)=cycle(choices.iter().filter(|v|v.family==family).map(|v|v.face.clone()),&face,direction) {face=next;} }
        Control::Size => { if let Some(next)=cycle(choices.iter().filter(|v|v.family==family&&v.face==face).map(|v|v.pixels),&size,direction) {size=next;} }
    }
    let Some(selected)=choices.iter().filter(|v|v.family==family).min_by_key(|v| (
        v.face!=face,
        !matches!(v.face.as_str(),"Regular"|"Sans-Serif"|"Monospaced"|"Half-width"),
        v.pixels.abs_diff(size), v.id.len())) else { return; };
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
    fn fusion_migration_sizes_and_mixed_script_choice() {
        let fonts:Vec<_>=[8,10,12].into_iter().map(|size|font(&format!("Fusion Pixel {size}px Mono"),"Sans-Serif",size)).collect();
        let mut a=Appearance::default();a.font="Fusion Pixel 10px Mono ja / Sans-Serif".into();a.font_size=10;
        adjust(&fonts,&mut a,Control::Size,1);assert_eq!(a.font,"Fusion Pixel 12px Mono / Sans-Serif");
        let saved:Appearance=serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(describe(&fonts,&saved).family,"Fusion Pixel");
        for lang in ["ja","ko","latin","zh_Hans","zh_Hant","zh_HK","zh_TW"] {
            assert_eq!(migrated_id(&format!("Fusion Pixel 12px Mono {lang} / Sans-Serif")), fonts[2].id);
        }
    }
    #[test]
    fn new_collections_group_sizes_and_preserve_face() {
        for family in ["Fusion Pixel", "Ark Pixel", "Jelly Pixel"] {
            let fonts = [font(&format!("{family} 10px Mono"), "Sans-Serif", 10),
                         font(&format!("{family} 12px Mono"), "Sans-Serif", 12),
                         font(&format!("{family} 12px Prop"), "Sans-Serif", 12)];
            let mut a = Appearance { font: fonts[0].id.clone(), font_size: 10, ..Default::default() };
            adjust(&fonts, &mut a, Control::Size, 1);
            assert_eq!(a.font, fonts[1].id);
            adjust(&fonts, &mut a, Control::Face, 1);
            assert_eq!(a.font, fonts[2].id);
            assert_eq!(describe(&fonts, &a).family, family);
        }
        assert_eq!(describe(&[font("Retro Pixel Arcade", "Sans-Serif", 8)],
            &Appearance { font:"Retro Pixel Arcade / Sans-Serif".into(), font_size:8, ..Default::default() }).face, "Arcade");
    }
    #[test]
    fn pixel_design_names_keep_saved_ids_and_tekuplus_faces_cycle() {
        for raw in ["x12y12pxMaruMinyaHangul", "x5y8pxNegaTape", "x5y8pxNegaClip",
                    "x14y24pxHeadUpDaisy", "x10y12pxDenkiChipHangul"] {
            let f = font(raw, "Regular", 12);
            let a = Appearance { font: f.id.clone(), font_size: 12, ..Default::default() };
            assert_eq!(describe(&[f], &a).family, raw.split_once("px").unwrap().1);
            assert_eq!(a.font, format!("{raw} / Regular"));
        }
        assert_eq!(pixel_family("xylophone"), None);
        let fonts = [font("Tekuplus", "Regular", 12), font("Tekuplus", "Bold", 12)];
        let mut a = Appearance { font: fonts[0].id.clone(), font_size: 12, ..Default::default() };
        adjust(&fonts, &mut a, Control::Face, 1);
        assert_eq!(a.font, "Tekuplus / Bold");
        assert_eq!(describe(&fonts, &a).family, "Tekuplus");
    }
}
