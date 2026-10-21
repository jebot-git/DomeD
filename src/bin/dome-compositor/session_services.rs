//! Start optional desktop services after the native session's Wayland socket exists.
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
            let candidates = [
                "/usr/libexec/polkit-kde-authentication-agent-1",
                "/usr/lib/x86_64-linux-gnu/libexec/polkit-kde-authentication-agent-1",
                "/usr/lib/aarch64-linux-gnu/libexec/polkit-kde-authentication-agent-1",
                "/usr/lib/polkit-kde-authentication-agent-1",
            ];
            if let Some(program) = candidates.iter().find(|path| std::path::Path::new(path).is_file()) {
                if let Err(error) = std::process::Command::new(program).spawn() {
                    eprintln!("Polkit authentication agent startup: {error}");
                }
            }
        }
        if std::env::var("DOME_START_KDECONNECT").as_deref() != Ok("0") {
            if std::env::var("DOME_TRAY_FRONTEND").as_deref() == Ok("qt6") {
                let _ = std::process::Command::new("kdeconnect-indicator").spawn();
            } else if let Ok(connection) = zbus::blocking::Connection::session() {
                if let Ok(bus) = zbus::blocking::Proxy::new(&connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus") {
                    let _ = bus.call::<_, _, u32>("StartServiceByName", &("org.kde.kdeconnect", 0u32));
                }
            }
        }

        if std::env::var("DOME_START_TRAY").as_deref() != Ok("0")
            && std::env::var("DOME_TRAY_FRONTEND").as_deref() != Ok("qt6") {
            if let Some(program) = dome::desktop_entries::find_executable("dome-tray") {
                if let Err(error) = std::process::Command::new(program).arg("--daemon").spawn() {
                    eprintln!("Native FLTK tray manager startup: {error}");
                }
            } else {
                eprintln!("Native tray manager is missing; build and install dome-tray");
            }
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
