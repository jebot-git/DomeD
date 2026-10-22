//! Real backend isolation: FLTerm and Ghostty may both own live overlays.
use std::{fs,os::unix::{fs::PermissionsExt,net::UnixDatagram},time::Duration};
#[test]
fn flterm_reuses_only_its_overlay_and_keeps_desktop_launches_independent() {
    let unique=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let runtime=std::env::temp_dir().join(format!("dome-flterm-{}-{unique}",std::process::id()));
    for backend in ["flterm-xr","ghostty-xr"] { fs::create_dir_all(runtime.join(backend)).unwrap(); }
    let binary=runtime.join("terminal-fixture");
    fs::write(&binary,"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$XDG_RUNTIME_DIR/desktop-args\"\n").unwrap();
    fs::set_permissions(&binary,fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("XDG_RUNTIME_DIR",&runtime);
    std::env::set_var("DOME_TERMINAL",&binary);
    std::env::set_var("DOME_TERMINAL_BACKEND","flterm");
    std::env::set_var("DOME_TERMINAL_MODE","overlay");
    let flterm=UnixDatagram::bind(runtime.join("flterm-xr/control.sock")).unwrap();
    let ghostty=UnixDatagram::bind(runtime.join("ghostty-xr/control.sock")).unwrap();
    for socket in [&flterm,&ghostty] { socket.set_read_timeout(Some(Duration::from_millis(100))).unwrap(); }
    dome::terminal::launch("dome-xr",dome::Realm::Xr).unwrap();
    let mut message=[0;64];
    for expected in [b"tab-new".as_slice(),b"show".as_slice()] {
        let size=flterm.recv(&mut message).unwrap();assert_eq!(&message[..size],expected);
    }
    dome::terminal::show("dome-desktop").unwrap();
    let size=flterm.recv(&mut message).unwrap();assert_eq!(&message[..size],b"show");
    assert!(flterm.recv(&mut message).is_err());assert!(ghostty.recv(&mut message).is_err());
    dome::terminal::hide().unwrap();
    let size=flterm.recv(&mut message).unwrap();assert_eq!(&message[..size],b"hide");
    assert!(ghostty.recv(&mut message).is_err());
    dome::terminal::launch("dome-desktop",dome::Realm::Desktop).unwrap();
    let deadline=std::time::Instant::now()+Duration::from_secs(3);
    loop {
        if fs::read_to_string(runtime.join("desktop-args")).ok().as_deref()==Some("--wayland\n") { break; }
        assert!(std::time::Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));
    }
    assert!(flterm.recv(&mut message).is_err());assert!(ghostty.recv(&mut message).is_err());
    fs::remove_dir_all(runtime).unwrap();
}
