//! Folder launches always inherit the caller's Wayland realm; never start a second XR runtime.
use std::{ffi::OsString, io, process::Command};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn preferred_in(paths: &[PathBuf]) -> Option<PathBuf> {
    for name in ["fsvr", "fsx-vr", "fsv-xr"] {
        for directory in paths {
            let file = directory.join(name);
            if std::fs::metadata(&file)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            {
                return Some(file);
            }
        }
    }
    None
}
fn preferred() -> Option<PathBuf> {
    let mut paths: Vec<_> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".local/bin"));
    }
    preferred_in(&paths)
}
pub fn configured_in(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut defaults = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            defaults = line == "[Default Applications]";
            continue;
        }
        if defaults {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "inode/directory" {
                    return value
                        .split(';')
                        .map(str::trim)
                        .find(|s| !s.is_empty())
                        .map(str::to_owned);
                }
            }
        }
    }
    None
}
pub fn command(socket: &str, arguments: &[String]) -> io::Result<Command> {
    if let Some(binary) = std::env::var_os("DOME_FILE_MANAGER") {
        return Ok(command_for(socket, binary, arguments));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    if let Some(id) = configured_in(&config.join("dome-mimeapps.list")) {
        let native =
            id == "org.fsvxr.fsvr.desktop" || id.contains("fsx-vr") || id.contains("fsv-xr");
        if native {
            if let Some(binary) = preferred() {
                return Ok(command_for(socket, binary.into(), arguments));
            }
        }
        let app = crate::desktop_entries::apps()
            .into_iter()
            .find(|a| a.desktop_id == id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("The default file manager {id} is not installed"),
                )
            })?;
        let reveal = arguments
            .iter()
            .any(|a| a == "--select" || a == "--properties");
        let mut args = vec!["launch".into(), app.path.to_string_lossy().into_owned()];
        for url in arguments
            .iter()
            .filter(|a| !matches!(a.as_str(), "--" | "--select" | "--properties"))
        {
            args.push(if reveal && url.starts_with("file://") {
                url.trim_end_matches('/')
                    .rsplit_once('/')
                    .map(|(parent, _)| format!("{parent}/"))
                    .unwrap_or_else(|| url.clone())
            } else {
                url.clone()
            });
        }
        return Ok(command_for(socket, "gio".into(), &args));
    }
    if let Some(binary) = preferred() {
        return Ok(command_for(socket, binary.into(), arguments));
    }
    let mut args = vec!["open".into()];
    args.extend(arguments.iter().filter(|a| !a.starts_with("--")).cloned());
    if args.len() == 1 {
        args.push(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    }
    Ok(command_for(socket, "gio".into(), &args))
}

pub fn command_for(socket: &str, binary: OsString, arguments: &[String]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .env("WAYLAND_DISPLAY", socket)
        .env_remove("WAYLAND_SOCKET")
        .env("QT_QPA_PLATFORM", "wayland")
        .env("GDK_BACKEND", "wayland")
        .env("XDG_CURRENT_DESKTOP", "Dome");
    command
}
pub fn launch(socket: &str, arguments: &[String]) -> io::Result<()> {
    let mut child = command(socket, arguments)?.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn installed_fsv_aliases_are_preferred_and_non_executables_are_skipped() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let root = std::env::temp_dir().join(format!("dome-file-manager-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        for name in ["fsx-vr", "fsv-xr"] {
            let path = root.join(name);
            fs::write(&path, b"#!/bin/sh\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(root.join("fsvr"), b"not executable").unwrap();
        assert_eq!(
            super::preferred_in(&[root.clone()]),
            Some(root.join("fsx-vr"))
        );
        fs::set_permissions(root.join("fsvr"), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            super::preferred_in(&[root.clone()]),
            Some(root.join("fsvr"))
        );
        let config = root.join("dome-mimeapps.list");
        fs::write(&config,"# preserved\n[Default Applications]\ninode/directory=custom.desktop;\n[Added Associations]\ninode/directory=other.desktop;\n").unwrap();
        assert_eq!(
            super::configured_in(&config).as_deref(),
            Some("custom.desktop")
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn folder_launch_keeps_selected_realm_and_literal_urls() {
        for socket in ["dome-desktop-test", "dome-xr-test"] {
            let args = vec![
                "--select".into(),
                "--".into(),
                "file:///tmp/a%20%23%20file".into(),
            ];
            let command = super::command_for(socket, "fsv-xr".into(), &args);
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                ["--select", "--", "file:///tmp/a%20%23%20file"]
            );
            assert!(command
                .get_envs()
                .any(|(key, value)| key == "WAYLAND_DISPLAY" && value == Some(socket.as_ref())));
            assert!(command
                .get_envs()
                .any(|(key, value)| key == "WAYLAND_SOCKET" && value.is_none()));
            assert!(!command
                .get_args()
                .any(|a| a == "--xr" || a == "--desktop" || a == "--domed"));
        }
    }
}
