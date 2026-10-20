//! XR launches reuse the overlay; desktop launches have independent Wayland windows.
use std::{io, process::Command};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode { Overlay, Wayland }
impl Mode {
    pub fn for_realm(self, realm: crate::Realm) -> Self {
        if realm == crate::Realm::Desktop { Self::Wayland } else { self }
    }
    pub fn configured() -> Self {
        if std::env::var("DOME_TERMINAL_MODE").as_deref() == Ok("wayland") { Self::Wayland } else { Self::Overlay }
    }
}
pub fn command(socket: &str, realm: crate::Realm) -> Command {
    let mode = Mode::configured().for_realm(realm);
    let binary = std::env::var_os("DOME_TERMINAL").unwrap_or_else(||
        if mode == Mode::Wayland { "ghostty-xr".into() } else { "ghostty-xr-launch".into() });
    command_for(socket, binary, mode)
}
pub fn command_for(socket: &str, binary: std::ffi::OsString, mode: Mode) -> Command {
    let mut command = Command::new(binary);
    if mode == Mode::Wayland { command.arg("--wayland"); }
    else { command.args(["--placement", "20"]); }
    command.env("WAYLAND_DISPLAY", socket);
    command
}

/// F12 belongs to the overlay in XR; desktop applications retain bare F12.
pub fn show_shortcut(key: u32, mods: crate::shortcuts::Mods, realm: crate::Realm, mode: Mode) -> bool {
    key == 0xffc9 && !mods.ctrl && !mods.alt && !mods.shift
        && (mods.logo || (realm == crate::Realm::Xr && mode == Mode::Overlay))
}

/// Reopen the existing overlay without creating another shell tab.
pub fn show(socket: &str, realm: crate::Realm) -> io::Result<()> {
    if Mode::configured().for_realm(realm) == Mode::Overlay {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            if show_overlay(&std::path::PathBuf::from(dir).join("ghostty-xr/control.sock")).is_ok() {
                return Ok(());
            }
        }
    }
    let mut child = command(socket, realm).spawn()?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(())
}

fn show_overlay(path: &std::path::Path) -> io::Result<()> {
    let socket = std::os::unix::net::UnixDatagram::unbound()?;
    socket.connect(path)?;
    socket.send(b"show")?;
    Ok(())
}

// Send both requests through one socket so the new tab is selected before focus.
fn reuse_overlay(path: &std::path::Path) -> io::Result<()> {
    let socket = std::os::unix::net::UnixDatagram::unbound()?;
    socket.connect(path)?;
    socket.send(b"tab-new")?;
    socket.send(b"show")?;
    Ok(())
}

pub fn launch(socket: &str, realm: crate::Realm) -> io::Result<()> {
    let mut command = command(socket, realm);
    if Mode::configured().for_realm(realm) == Mode::Overlay {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            if reuse_overlay(&std::path::PathBuf::from(dir).join("ghostty-xr/control.sock")).is_ok() {
                return Ok(());
            }
        }
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn f12_show_is_realm_and_modifier_aware() {
        use super::*;
        use crate::{Realm, shortcuts::Mods};
        for realm in [Realm::Desktop, Realm::Xr] {
            for mode in [Mode::Overlay, Mode::Wayland] {
                assert_eq!(show_shortcut(0xffc9, Mods::default(), realm, mode),
                    realm == Realm::Xr && mode == Mode::Overlay);
                assert!(show_shortcut(0xffc9, Mods { logo: true, ..Default::default() }, realm, mode));
                for mods in [Mods { ctrl: true, ..Default::default() },
                    Mods { shift: true, ..Default::default() }, Mods { alt: true, ..Default::default() }] {
                    assert!(!show_shortcut(0xffc9, mods, realm, mode));
                }
            }
        }
    }
    #[test]
    fn desktop_is_wayland_even_with_an_overlay_configured() {
        use super::Mode;
        use crate::Realm;
        assert_eq!(Mode::Overlay.for_realm(Realm::Desktop), Mode::Wayland);
        assert_eq!(Mode::Overlay.for_realm(Realm::Xr), Mode::Overlay);
        assert_eq!(Mode::Wayland.for_realm(Realm::Xr), Mode::Wayland);
    }
    #[test]
    fn repeated_overlay_launch_creates_tab_then_shows_it() {
        use std::os::unix::net::UnixDatagram;
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("dome-terminal-{}-{unique}.sock", std::process::id()));
        let receiver = UnixDatagram::bind(&path).unwrap();
        receiver.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
        super::reuse_overlay(&path).unwrap();
        let mut message = [0; 32];
        for expected in [b"tab-new".as_slice(), b"show".as_slice()] {
            let size = receiver.recv(&mut message).unwrap();
            assert_eq!(&message[..size], expected);
        }
        super::show_overlay(&path).unwrap();
        let size = receiver.recv(&mut message).unwrap();
        assert_eq!(&message[..size], b"show");
        assert!(receiver.recv(&mut message).is_err(), "show must not create a new tab");
        drop(receiver);
        assert!(super::reuse_overlay(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn overlay_keeps_native_input_layout_and_independent_layer_submission() {
        let command = super::command_for("dome-xr-test", "ghostty-xr-launch".into(), super::Mode::Overlay);
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--placement", "20"]);
    }
    #[test]
    fn terminal_uses_selected_wayland_realm_without_shell_expansion() {
        let command = super::command_for("dome-xr-test", "ghostty-xr".into(), super::Mode::Wayland);
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--wayland"]);
        assert!(command.get_envs().any(|(key, value)| {
            key == "WAYLAND_DISPLAY" && value == Some("dome-xr-test".as_ref())
        }));
    }
}
