//! Exercise GIO desktop entries/actions and PATH execution without a browser profile.
use dome::desktop_entries::{launch, launch_action, launch_executable, parse};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::{Duration, Instant}};

fn record(path: &Path) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = fs::read_to_string(path) {
            if text.ends_with("done\n") {
                fs::remove_file(path).unwrap();
                return text.lines().map(str::to_owned).collect();
            }
        }
        assert!(Instant::now() < deadline, "application did not record its environment");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn vivaldi_launches_select_kwallet6_without_adding_arguments() {
    let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("dome-vivaldi-launch-{}-{unique}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let journal = dir.join("environment");
    let executable = dir.join("vivaldi-snapshot");
    fs::write(&executable, format!(
        "#!/bin/sh\n{{\nprintf '%s\\n' \"$XDG_CURRENT_DESKTOP\" \"$KDE_SESSION_VERSION\" \"$WAYLAND_DISPLAY\" \"$#\"\nprintf '%s\\n' \"$@\"\nprintf 'done\\n'\n}} > '{}'\n",
        journal.display(),
    )).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let entry = dir.join("vivaldi-stable.desktop");
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=Vivaldi\nExec={} normal\nActions=new;\n[Desktop Action new]\nName=New Window\nExec={} action\n",
        executable.display(), executable.display(),
    );
    fs::write(&entry, &text).unwrap();
    let app = parse(&text, entry, "vivaldi-stable.desktop".into(), "C", &["Dome"]).unwrap();
    launch(&app, "wayland-test").unwrap();
    assert_eq!(record(&journal), ["Dome:KDE", "6", "wayland-test", "1", "normal", "done"]);
    launch_action(&app, "new", "wayland-test").unwrap();
    assert_eq!(record(&journal), ["Dome:KDE", "6", "wayland-test", "1", "action", "done"]);
    launch_executable(&executable, "wayland-xr-test").unwrap();
    assert_eq!(record(&journal), ["Dome:KDE", "6", "wayland-xr-test", "0", "", "done"]);

    let other = dir.join("other-browser");
    fs::copy(&executable, &other).unwrap();
    launch_executable(&other, "wayland-test").unwrap();
    assert_eq!(record(&journal)[0], "Dome");
    fs::remove_dir_all(dir).unwrap();
}
