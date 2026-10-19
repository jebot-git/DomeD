//! Ghostty keeps its independent XR overlay; Wayland mode is an explicit diagnostic option.
use std::{io, process::Command};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode { Overlay, Wayland }
impl Mode {
    pub fn configured() -> Self {
        if std::env::var("DOME_TERMINAL_MODE").as_deref() == Ok("wayland") { Self::Wayland } else { Self::Overlay }
    }
}
pub fn command(socket: &str) -> Command {
    let mode = Mode::configured();
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

pub fn launch(socket: &str) -> io::Result<()> {
    let mut command = command(socket);
    if Mode::configured() == Mode::Overlay {
        use std::os::unix::fs::FileTypeExt;
        let running = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from)
            .and_then(|dir| std::fs::symlink_metadata(dir.join("ghostty-xr/control.sock")).ok())
            .is_some_and(|meta| meta.file_type().is_socket());
        if running {
            let sibling = std::path::Path::new(command.get_program()).parent().unwrap_or(std::path::Path::new(".")).join("ghostty-xrctl");
            let controller = if sibling.is_file() { sibling.into_os_string() } else { "ghostty-xrctl".into() };
            if Command::new(controller).arg("show").status()?.success() { return Ok(()); }
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
