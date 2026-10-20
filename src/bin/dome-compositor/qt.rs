use serde_json::Value;
use std::ffi::CString;
unsafe extern "C" {
    fn dome_qt_paint(json: *const std::ffi::c_char, pixels: *mut u8, len: i32) -> i32;
    fn dome_qt_hit(w: i32, h: i32, x: i32, y: i32) -> i32;
}
pub fn paint(value: Value) -> Vec<u8> {
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
