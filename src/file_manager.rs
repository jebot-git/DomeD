//! Folder launches always inherit the caller's Wayland realm; never start a second XR runtime.
use std::{ffi::OsString, io, process::Command};

pub fn command_for(socket: &str, binary: OsString, arguments: &[String]) -> Command {
    let mut command = Command::new(binary);
    command.args(arguments)
        .env("WAYLAND_DISPLAY", socket)
        .env_remove("WAYLAND_SOCKET")
        .env("QT_QPA_PLATFORM", "wayland")
        .env("GDK_BACKEND", "wayland")
        .env("XDG_CURRENT_DESKTOP", "Dome");
    command
}
pub fn launch(socket: &str, arguments: &[String]) -> io::Result<()> {
    let binary = std::env::var_os("DOME_FILE_MANAGER").unwrap_or_else(|| "fsv-xr".into());
    let mut child = command_for(socket, binary, arguments).spawn()?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn folder_launch_keeps_selected_realm_and_literal_urls() {
        for socket in ["dome-desktop-test", "dome-xr-test"] {
            let args = vec!["--select".into(), "--".into(), "file:///tmp/a%20%23%20file".into()];
            let command = super::command_for(socket, "fsv-xr".into(), &args);
            assert_eq!(command.get_args().collect::<Vec<_>>(), ["--select", "--", "file:///tmp/a%20%23%20file"]);
            assert!(command.get_envs().any(|(key, value)| key == "WAYLAND_DISPLAY" && value == Some(socket.as_ref())));
            assert!(command.get_envs().any(|(key, value)| key == "WAYLAND_SOCKET" && value.is_none()));
            assert!(!command.get_args().any(|a| a == "--xr" || a == "--desktop" || a == "--domed"));
        }
    }
}
