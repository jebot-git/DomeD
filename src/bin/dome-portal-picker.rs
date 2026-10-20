//! Separate Qt6 process: the compositor never blocks its GUI thread for consent.
use dome as _;
use std::ffi::CString;
unsafe extern "C" {
    fn dome_portal_picker(argc: i32, argv: *mut *mut std::ffi::c_char) -> i32;
}
fn main() {
    let args: Vec<_> = std::env::args().map(|s| CString::new(s).unwrap()).collect();
    let mut pointers: Vec<_> = args.iter().map(|s| s.as_ptr() as *mut _).collect();
    pointers.push(std::ptr::null_mut());
    std::process::exit(unsafe { dome_portal_picker(args.len() as i32, pointers.as_mut_ptr()) });
}
