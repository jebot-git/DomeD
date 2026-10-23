//! Start optional desktop services after the native session's Wayland socket exists.
fn supervise_tray() {
    let _ = supervise_tray_until(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));
}
fn supervise_tray_until(stop: std::sync::Arc<std::sync::atomic::AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use std::time::{Duration, Instant};
        let mut child: Option<std::process::Child> = None;
        let mut started = Instant::now();
        let mut retry = Instant::now();
        let mut delay = Duration::from_millis(250);
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(process) = child.as_mut() {
                match process.try_wait() {
                    Ok(Some(status)) => {
                        eprintln!("Native tray manager exited ({status}); restarting");
                        if started.elapsed() > Duration::from_secs(30) { delay = Duration::from_millis(250); }
                        retry = Instant::now() + delay;
                        delay = (delay * 2).min(Duration::from_secs(8));
                        child = None;
                    }
                    Ok(None) => (),
                    Err(error) => { eprintln!("Native tray manager supervision: {error}"); child = None; }
                }
            }
            if child.is_none() && Instant::now() >= retry {
                let owned = zbus::blocking::Connection::session().ok().and_then(|connection|
                    zbus::blocking::Proxy::new(&connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus")
                        .ok().and_then(|bus| bus.call::<_, _, bool>("NameHasOwner", &("org.domed.Tray",)).ok())).unwrap_or(true);
                if !owned {
                    if let Some(program) = dome::desktop_entries::find_executable("dome-tray") {
                        match std::process::Command::new(program).arg("--daemon").spawn() {
                            Ok(process) => { child = Some(process); started = Instant::now(); }
                            Err(error) => { eprintln!("Native tray manager startup: {error}"); retry = Instant::now() + delay; delay = (delay * 2).min(Duration::from_secs(8)); }
                        }
                    } else { retry = Instant::now() + Duration::from_secs(8); }
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if let Some(mut process) = child { let _ = process.kill(); let _ = process.wait(); }
    })
}
pub fn start() {
    // Diagnostic sessions use private buses and must never touch a real PAM login socket.
    if std::env::var_os("DOME_DRM_TEST_DEVICE").is_some()
        || std::env::var_os("DOME_DRM_TEST_DEVICES").is_some()
    {
        return;
    }
    std::thread::spawn(|| {
        // A standalone DomeD login has no Plasma autostart phase. Start the
        // retained authentication agent on this session's bus and Wayland socket.
        if std::env::var("DOME_START_POLKIT").as_deref() != Ok("0") {
            if std::path::Path::new("/usr/local/libexec/domed/dome-polkit-agent").is_file() {
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "start", "dome-polkit-agent.service"]).status();
            } else {
                eprintln!("Native FLTK Polkit agent is not installed: /usr/local/libexec/domed/dome-polkit-agent");
            }
        }

        if std::env::var("DOME_START_KDECONNECT").as_deref() != Ok("0") {
            if let Ok(connection) = zbus::blocking::Connection::session() {
                if let Ok(bus) = zbus::blocking::Proxy::new(&connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus") {
                    let _ = bus.call::<_, _, u32>("StartServiceByName", &("org.kde.kdeconnect", 0u32));
                }
            }
        }

        if std::env::var("DOME_START_TRAY").as_deref() != Ok("0") {
            supervise_tray();
        }
        if std::env::var_os("PAM_KWALLET5_LOGIN").is_some() {
            let candidates = [
                "/usr/libexec/pam_kwallet_init",
                "/usr/share/libpam-kwallet-common/pam_kwallet_init",
            ];
            if let Some(program) = candidates.iter().find(|path| std::path::Path::new(path).is_file()) {
                match std::process::Command::new(program).status() {
                    Ok(status) if status.success() => (),
                    other => eprintln!("KWallet PAM environment handoff: {other:?}"),
                }
            }
        }
        // Current KWallet uses KSecretService; older versions activate kwalletd on demand.
        // Preserve an already running Secret Service provider on the session bus.
        let owned = zbus::blocking::Connection::session()
            .ok()
            .and_then(|connection| {
                zbus::blocking::Proxy::new(
                    &connection,
                    "org.freedesktop.DBus",
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                )
                .ok()
                .and_then(|proxy| {
                    proxy
                        .call::<_, _, bool>("NameHasOwner", &("org.freedesktop.secrets",))
                        .ok()
                })
            })
            .unwrap_or(true);
        if !owned && std::path::Path::new("/usr/bin/ksecretd").is_file() {
            if let Err(error) = std::process::Command::new("/usr/bin/ksecretd").spawn() {
                eprintln!("KWallet Secret Service startup: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires private D-Bus and Xvfb; run tests/tray/test_supervision.py"]
    fn restarts_manager_after_process_exit() {
        assert_eq!(std::env::var("DOME_PRIVATE_TRAY_SUPERVISION").as_deref(), Ok("1"));
        let connection = zbus::blocking::Connection::session().unwrap();
        let bus = zbus::blocking::Proxy::new(&connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").unwrap();
        assert!(!bus.call::<_, _, bool>("NameHasOwner", &("org.domed.Tray",)).unwrap());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let supervisor = supervise_tray_until(stop.clone());
        let wait_pid = |previous: u32| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if let Ok(pid) = bus.call::<_, _, u32>("GetConnectionUnixProcessID", &("org.domed.Tray",)) {
                    if pid != previous { return pid; }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            panic!("Tray manager did not recover");
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let first = wait_pid(0);
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(first as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
            let second = wait_pid(first);
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(second as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
            let third = wait_pid(second);
            assert_ne!(first, third);
            println!("PASS permanent tray manager recovers after two forced crashes");
        }));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        supervisor.join().unwrap();
        assert!(!bus.call::<_, _, bool>("NameHasOwner", &("org.domed.Tray",)).unwrap());
        if let Err(error) = result { std::panic::resume_unwind(error); }
    }
}
