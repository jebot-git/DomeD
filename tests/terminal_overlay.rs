//! A separate test process isolates the terminal environment from other tests.
use std::{
    fs,
    os::unix::{fs::PermissionsExt, net::UnixDatagram},
    time::{Duration, Instant},
};

#[test]
fn wayland_launch_mode_keeps_overlay_shortcut_and_cold_start_independent() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let runtime =
        std::env::temp_dir().join(format!("dome-overlay-{}-{unique}", std::process::id()));
    fs::create_dir_all(runtime.join("ghostty-xr")).unwrap();
    let binary = runtime.join("terminal-fixture");
    fs::write(
        &binary,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$XDG_RUNTIME_DIR/overlay-args\"\n",
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", &runtime);
    std::env::set_var("DOME_TERMINAL", &binary);
    std::env::set_var("DOME_TERMINAL_MODE", "wayland");
    std::env::set_var("DOME_TERMINAL_BACKEND", "ghostty");
    let command = dome::terminal::command("test-desktop", dome::Realm::Desktop);
    assert_eq!(command.get_args().collect::<Vec<_>>(), ["--wayland"]);
    let path = runtime.join("ghostty-xr/control.sock");
    let receiver = UnixDatagram::bind(&path).unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    dome::terminal::show("test-desktop").unwrap();
    let mut message = [0; 64];
    let len = receiver.recv(&mut message).unwrap();
    assert_eq!(&message[..len], b"show");
    dome::terminal::toggle("test-desktop").unwrap();
    let len=receiver.recv(&mut message).unwrap();
    assert_eq!(&message[..len], b"toggle");
    assert!(
        receiver.recv(&mut message).is_err(),
        "shortcut must not create a tab"
    );
    assert!(
        !runtime.join("overlay-args").exists(),
        "existing overlay must be reused"
    );
    drop(receiver); // Leave a stale socket to exercise cleanup and native overlay cold start.
    dome::terminal::show("test-desktop").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let args = loop {
        if let Ok(args) = fs::read_to_string(runtime.join("overlay-args")) {
            if args.ends_with("20\n") {
                break args;
            }
        }
        assert!(Instant::now() < deadline, "overlay launcher did not run");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(args, "--placement\n20\n");
    assert!(!path.exists());
    fs::remove_dir_all(runtime).unwrap();
}
