//! FileManager1 belongs to the Dome login session, not a nested host desktop.
use dome::xr_transport::Shared;
use std::os::unix::ffi::OsStrExt;
use zbus::{message::Header, Connection};
struct Server { desktop: String, xr: String, bridge: Shared }
impl Server {
    async fn socket(&self, connection: &Connection, header: &Header<'_>) -> String {
        // A background desktop application must not open an XR-only window just
        // because the headset owns input. Prefer the requesting process's realm.
        if let Some(sender) = header.sender() {
            if let Ok(reply) = connection.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"), "GetConnectionUnixProcessID", &(sender.as_str(),)).await {
                if let Ok(pid) = reply.body().deserialize::<u32>() {
                    if let Ok(environment) = std::fs::read(format!("/proc/{pid}/environ")) {
                        for entry in environment.split(|b| *b == 0) {
                            if let Some(display) = entry.strip_prefix(b"WAYLAND_DISPLAY=") {
                                let display = std::path::Path::new(std::ffi::OsStr::from_bytes(display));
                                if display.file_name() == std::path::Path::new(&self.xr).file_name() { return self.xr.clone(); }
                                if display.file_name() == std::path::Path::new(&self.desktop).file_name() { return self.desktop.clone(); }
                            }
                        }
                    }
                }
            }
        }
        if self.bridge.lock().unwrap().input_realm == dome::Realm::Xr { self.xr.clone() } else { self.desktop.clone() }
    }
    async fn open(&self, urls: Vec<String>, startup: String, option: Option<&str>, connection: &Connection, header: &Header<'_>) -> zbus::fdo::Result<()> {
        if urls.len() > 32 || urls.iter().map(String::len).sum::<usize>() > 65536 {
            return Err(zbus::fdo::Error::LimitsExceeded("At most 32 local locations per request".into()));
        }
        // fsv-xr owns URL decoding, file validation and error presentation.
        if urls.is_empty() { return Ok(()); }
        if urls.iter().any(|url| !(url.starts_with("file://") || url == "trash:/" || url == "trash:///")) {
            return Err(zbus::fdo::Error::InvalidArgs("Only local file URLs and trash:/ are supported".into()));
        }
        let socket = self.socket(connection, header).await;
        let mut arguments: Vec<String> = option.into_iter().map(str::to_owned).collect();
        arguments.push("--".into()); arguments.extend(urls);
        let mut command = dome::file_manager::command(&socket, &arguments)
            .map_err(|e|zbus::fdo::Error::Failed(e.to_string()))?;
        if !startup.is_empty() { command.env("DESKTOP_STARTUP_ID", startup); }
        let mut child = command.spawn().map_err(|e| zbus::fdo::Error::Failed(format!("Cannot launch file manager: {e}")))?;
        std::thread::spawn(move || { let _ = child.wait(); });
        Ok(())
    }
}
#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl Server {
    async fn show_folders(&self, uris: Vec<String>, startup_id: String, #[zbus(connection)] connection: &Connection, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.open(uris, startup_id, None, connection, &header).await
    }
    async fn show_items(&self, uris: Vec<String>, startup_id: String, #[zbus(connection)] connection: &Connection, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.open(uris, startup_id, Some("--select"), connection, &header).await
    }
    async fn show_item_properties(&self, uris: Vec<String>, startup_id: String, #[zbus(connection)] connection: &Connection, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.open(uris, startup_id, Some("--properties"), connection, &header).await
    }
}
pub fn start(desktop: String, xr: String, bridge: Shared) -> Option<zbus::blocking::Connection> {
    let configured = std::env::var("DOME_FILE_MANAGER_SERVICE").ok();
    if configured.as_deref() == Some("0") || (configured.as_deref() != Some("1") && std::env::var("DOME_LOGIN_SESSION").as_deref() != Ok("1")) { return None; }
    match zbus::blocking::connection::Builder::session().and_then(|b| b.name("org.freedesktop.FileManager1"))
        .and_then(|b| b.serve_at("/org/freedesktop/FileManager1", Server { desktop, xr, bridge }))
        .and_then(|b| b.build()) {
        Ok(connection) => Some(connection),
        Err(error) => { eprintln!("Dome FileManager1 unavailable (an existing owner is preserved): {error}"); None }
    }
}
