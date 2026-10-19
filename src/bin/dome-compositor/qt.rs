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
