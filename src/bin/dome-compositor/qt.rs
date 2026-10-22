use serde_json::Value;
use std::ffi::CString;
pub use dome::font_picker::BitmapFont;
unsafe extern "C" {
    fn dome_bdf_font_resolve(id: *const std::ffi::c_char, pixels: i32, scale: i32) -> *const std::ffi::c_char;
    fn dome_qt_queue_bitmap_font(data: *const u8, len: i32);
    fn dome_qt_font_catalog(data: *const u8, len: i32) -> *const std::ffi::c_char;
    fn dome_qt_paint(json: *const std::ffi::c_char, pixels: *mut u8, len: i32) -> i32;
    fn dome_qt_hit(w: i32, h: i32, x: i32, y: i32, caption: i32) -> i32;
}
pub fn resolved_font(appearance: &dome::appearance::Appearance) -> Result<Value, String> {
    bitmap_fonts();
    let id = CString::new(appearance.font.as_str()).map_err(|e| e.to_string())?;
    let data = unsafe { std::ffi::CStr::from_ptr(dome_bdf_font_resolve(id.as_ptr(), appearance.font_size as i32, appearance.font_scale as i32)) };
    let value: Value = serde_json::from_slice(data.to_bytes()).map_err(|e| e.to_string())?;
    if let Some(error) = value["error"].as_str() { return Err(error.to_owned()); }
    Ok(value)
}
pub fn normalize_appearance(appearance: &mut dome::appearance::Appearance) {
    let fonts = bitmap_fonts();
    let chosen = fonts.iter().find(|font| font.id == appearance.font)
        .or_else(|| fonts.iter().find(|font| font.id == "Unifont / Sans-Serif"))
        .or_else(|| fonts.first());
    if let Some(font) = chosen {
        appearance.font = font.id.clone();
        if let Some(size) = font.sizes.iter().min_by_key(|size| size.abs_diff(appearance.font_size)) {
            appearance.font_size = *size;
        }
    }
}
pub fn bitmap_fonts() -> Vec<BitmapFont> {
    static INITIALIZED: std::sync::Once = std::sync::Once::new();
    INITIALIZED.call_once(|| {
        let cache = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
        let inherited = std::env::var("DE_XR_FONTCONFIG_BASE").or_else(|_| std::env::var("FONTCONFIG_FILE"))
            .unwrap_or_else(|_| "/etc/fonts/fonts.conf".into());
        std::env::set_var("DE_XR_FONTCONFIG_BASE", &inherited);
        let configuration = dome::font_export::export(&cache.join("de-xr/bundled"), &inherited, &[])
            .expect("Export bundled BDF fonts");
        std::env::set_var("FONTCONFIG_FILE", configuration);
        // Keep the default available even when no installed font directories exist.
        let font = dome::bundled_fonts::FONTS[0].1;
        unsafe { dome_qt_queue_bitmap_font(font.as_ptr(), font.len() as i32); }

    });
    let json = unsafe { std::ffi::CStr::from_ptr(dome_qt_font_catalog(std::ptr::null(), 0)) };
    serde_json::from_slice(json.to_bytes()).expect("BDF font catalog")
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
pub fn hit(w: i32, h: i32, x: i32, y: i32, caption: i32) -> i32 {
    unsafe { dome_qt_hit(w, h, x, y, caption) }
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
