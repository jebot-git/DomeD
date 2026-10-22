//! XR launches reuse the overlay; desktop launches have independent Wayland windows.
use std::{io, process::Command};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Backend { Ghostty, FlTerm }
impl Backend {
    pub fn configured() -> Self {
        let configured = std::env::var("DOME_TERMINAL_BACKEND").ok();
        let custom = std::env::var_os("DOME_TERMINAL");
        Self::select(configured.as_deref(), custom.as_deref(), program_exists("flterm-xr"))
    }
    fn select(configured: Option<&str>, custom: Option<&std::ffi::OsStr>, flterm_available: bool) -> Self {
        match configured {
            Some("flterm") => Self::FlTerm,
            Some("ghostty") => Self::Ghostty,
            _ if custom.is_some() => {
                if std::path::Path::new(custom.unwrap()).file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("flterm-xr")) { Self::FlTerm } else { Self::Ghostty }
            }
            _ if flterm_available => Self::FlTerm,
            _ => Self::Ghostty,
        }
    }
    pub fn socket_name(self) -> &'static str {
        match self { Self::FlTerm => "flterm-xr/control.sock", Self::Ghostty => "ghostty-xr/control.sock" }
    }
    fn binary(self, mode: Mode) -> &'static str {
        match (self, mode) {
            (Self::FlTerm, Mode::Wayland) => "flterm-xr",
            (Self::FlTerm, Mode::Overlay) => "flterm-xr-launch",
            (Self::Ghostty, Mode::Wayland) => "ghostty-xr",
            (Self::Ghostty, Mode::Overlay) => "ghostty-xr-launch",
        }
    }
}
fn program_exists(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join(name)).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }))
}

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
    command_for_mode(socket, Mode::configured().for_realm(realm))
}
fn command_for_mode(socket: &str, mode: Mode) -> Command {
    let binary = std::env::var_os("DOME_TERMINAL")
        .unwrap_or_else(|| Backend::configured().binary(mode).into());
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

/// Hide only the selected backend, including during seat/VT deactivation.
/// A missing overlay is never started by this operation.
pub fn hide() -> io::Result<()> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is unset"))?;
    overlay_socket(&std::path::PathBuf::from(runtime).join(Backend::configured().socket_name()))?.send(b"hide")?;
    Ok(())
}

/// Show or start the independent overlay in either realm, without adding a tab.
/// DOME_TERMINAL_MODE controls new terminals, never the overlay shortcut.
pub fn show(socket: &str) -> io::Result<()> {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        if show_overlay(&std::path::PathBuf::from(dir).join(Backend::configured().socket_name())).is_ok() {
            return Ok(());
        }
    }
    let mut child = command_for_mode(socket, Mode::Overlay).spawn()?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(())
}

fn overlay_socket(path: &std::path::Path) -> io::Result<std::os::unix::net::UnixDatagram> {
    let socket = std::os::unix::net::UnixDatagram::unbound()?;
    if let Err(error) = socket.connect(path) {
        // A killed session leaves the pathname behind. Only ECONNREFUSED
        // establishes that the datagram endpoint has no live owner.
        if error.kind() == io::ErrorKind::ConnectionRefused {
            let _ = std::fs::remove_file(path);
        }
        return Err(error);
    }
    Ok(socket)
}

fn show_overlay(path: &std::path::Path) -> io::Result<()> {
    overlay_socket(path)?.send(b"show")?;
    Ok(())
}

// Send both requests through one socket so the new tab is selected before focus.
fn reuse_overlay(path: &std::path::Path) -> io::Result<()> {
    let socket = overlay_socket(path)?;
    socket.send(b"tab-new")?;
    socket.send(b"show")?;
    Ok(())
}

pub fn launch(socket: &str, realm: crate::Realm) -> io::Result<()> {
    let mut command = command(socket, realm);
    if Mode::configured().for_realm(realm) == Mode::Overlay {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            if reuse_overlay(&std::path::PathBuf::from(dir).join(Backend::configured().socket_name())).is_ok() {
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
    fn backend_selection_preserves_custom_terminals_and_separate_control_sockets() {
        use super::*;
        assert_eq!(Backend::select(None, None, true), Backend::FlTerm);
        assert_eq!(Backend::select(None, None, false), Backend::Ghostty);
        assert_eq!(Backend::select(Some("ghostty"), None, true), Backend::Ghostty);
        assert_eq!(Backend::select(None, Some("/opt/bin/ghostty-xr".as_ref()), true), Backend::Ghostty);
        assert_eq!(Backend::select(None, Some("/opt/bin/flterm-xr".as_ref()), false), Backend::FlTerm);
        assert_eq!(Backend::select(Some("flterm"), Some("/opt/bin/custom-terminal".as_ref()), false), Backend::FlTerm);
        assert_ne!(Backend::FlTerm.socket_name(), Backend::Ghostty.socket_name());
        assert_eq!(Backend::FlTerm.binary(Mode::Overlay), "flterm-xr-launch");
        assert_eq!(Backend::FlTerm.binary(Mode::Wayland), "flterm-xr");
    }

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
        assert!(!path.exists(), "dead overlay socket must be removed before respawning");
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
