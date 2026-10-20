use serde_json::Value;
use std::ffi::CString;
#[derive(Clone, Debug, serde::Deserialize)]
pub struct BitmapFont {
    pub id: String,
    pub sizes: Vec<u16>,
}
unsafe extern "C" {
    fn dome_qt_queue_bitmap_font(data: *const u8, len: i32);
    fn dome_qt_font_catalog(data: *const u8, len: i32) -> *const std::ffi::c_char;
    fn dome_qt_paint(json: *const std::ffi::c_char, pixels: *mut u8, len: i32) -> i32;
    fn dome_qt_hit(w: i32, h: i32, x: i32, y: i32) -> i32;
}
pub fn bitmap_fonts() -> &'static [BitmapFont] {
    static FONTS: std::sync::OnceLock<Vec<BitmapFont>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        let extras: &[&[u8]] = &[
            include_bytes!("../../../assets/fonts/cozette-v1.30.0/cozette.otb"),
            include_bytes!("../../../assets/fonts/denkichip-v1.213/DenkiChipHangul.bdf"),
            include_bytes!("../../../assets/fonts/galmuri-v2.40.4/Galmuri11Bitmap-Regular-2.40.4.ttf"),
            include_bytes!("../../../assets/fonts/galmuri-v2.40.4/Galmuri14Bitmap-Regular-2.40.4.ttf"),
            include_bytes!("../../../assets/fonts/galmuri-v2.40.4/Galmuri9Bitmap-Regular-2.40.4.ttf"),
            include_bytes!("../../../assets/fonts/galmuri-v2.40.4/Galmuri7Bitmap-Regular-2.40.4.ttf"),
        ];
        for font in extras {
            unsafe { dome_qt_queue_bitmap_font(font.as_ptr(), font.len() as i32); }
        }
        let font = include_bytes!("../../../assets/fonts/unifont-17.0.04.pcf");
        let json = unsafe { std::ffi::CStr::from_ptr(dome_qt_font_catalog(font.as_ptr(), font.len() as i32)) };
        serde_json::from_slice(json.to_bytes()).expect("Qt bitmap font catalog")
    })
}
pub fn paint(value: Value) -> Vec<u8> {
    bitmap_fonts();
    let w = value["width"].as_u64().unwrap();
    let h = value["height"].as_u64().unwrap();
    assert!(w * h <= 32_000_000);
    let json = CString::new(value.to_string()).unwrap();
    let mut out = vec![0; w as usize * h as usize * 4];
    assert_eq!(
        unsafe { dome_qt_paint(json.as_ptr(), out.as_mut_ptr(), out.len() as i32) },
        1
    );
    out
}
pub fn hit(w: i32, h: i32, x: i32, y: i32) -> i32 {
    unsafe { dome_qt_hit(w, h, x, y) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    #[test]
    fn tray_pixmaps_keep_network_argb_colours_and_alpha() {
        let pixel = [255u8, 12, 93, 208];
        let raw = pixel.repeat(22 * 22);
        let value = serde_json::json!({"kind":"shell","width":1024,"height":768,"tray":{"items":[{"key":"test","title":"Colour","width":22,"height":22,"pixmap":base64::engine::general_purpose::STANDARD.encode(raw)}]}});
        let image = paint(value.clone());
        let index = (18 * 1024 + 790) * 4;
        assert_eq!(&image[index..index + 4], &[208, 93, 12, 255]);
        let mut transparent = value;
        transparent["tray"]["items"][0]["pixmap"] = serde_json::json!(
            base64::engine::general_purpose::STANDARD.encode([0u8, 255, 0, 0].repeat(22 * 22))
        );
        let image = paint(transparent);
        assert!(
            image[index + 2] < 50,
            "Transparent red must not become opaque or tinted"
        );
    }
}
