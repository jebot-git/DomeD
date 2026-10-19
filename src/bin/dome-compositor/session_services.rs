//! Start optional wallet services only after the native session's Wayland socket exists.
pub fn start() {
    // Diagnostic sessions use private buses and must never touch a real PAM login socket.
    if std::env::var_os("DOME_DRM_TEST_DEVICE").is_some()
        || std::env::var_os("DOME_DRM_TEST_DEVICES").is_some()
    {
        return;
    }
    std::thread::spawn(|| {
        if std::env::var_os("PAM_KWALLET5_LOGIN").is_some() {
            match std::process::Command::new("/usr/libexec/pam_kwallet_init").status() {
                Ok(status) if status.success() => (),
                other => eprintln!("KWallet PAM environment handoff: {other:?}"),
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
