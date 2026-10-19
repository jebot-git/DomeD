//! Per-socket launch realm, independent of headset visibility and input ownership.
use crate::Realm;
use serde::{Deserialize, Serialize};
use std::{fs, io, os::unix::fs::OpenOptionsExt, path::PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct Context {
    pub version: u32,
    pub compositor_pid: u32,
    pub realm: Realm,
}
pub struct Registration(PathBuf);
impl Registration {
    pub fn publish(socket: &std::ffi::OsStr, realm: Realm) -> io::Result<Self> {
        use std::io::Write;
        let socket = PathBuf::from(socket);
        let socket = if socket.is_absolute() { socket } else {
            PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR missing"))?).join(socket)
        };
        let path = PathBuf::from(format!("{}.dome-session.json", socket.display()));
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        let context = Context { version: 1, compositor_pid: std::process::id(), realm };
        let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary)?;
        let result = (|| {
            file.write_all(&serde_json::to_vec(&context)?)?;
            fs::rename(&temporary, &path)
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result?;
        Ok(Self(path))
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        // A newly started compositor may already have reused the socket name.
        let ours = fs::read(&self.0).ok().and_then(|bytes| serde_json::from_slice::<Context>(&bytes).ok())
            .is_some_and(|context| context.compositor_pid == std::process::id());
        if ours { let _ = fs::remove_file(&self.0); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn socket_records_are_private_versioned_and_cleaned_up() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("dome-context-{}-{unique}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        for realm in [Realm::Desktop, Realm::Xr] {
            let socket = directory.join(if realm == Realm::Xr { "wayland-xr" } else { "wayland-desktop" });
            let registration = Registration::publish(socket.as_os_str(), realm).unwrap();
            let context: Context = serde_json::from_slice(&fs::read(&registration.0).unwrap()).unwrap();
            assert_eq!(context.version, 1);
            assert_eq!(context.compositor_pid, std::process::id());
            assert_eq!(context.realm, realm);
            assert_eq!(fs::metadata(&registration.0).unwrap().permissions().mode() & 0o777, 0o600);
            let path = registration.0.clone(); drop(registration);
            assert!(!path.exists());
        }
        fs::remove_dir(directory).unwrap();
    }
}
