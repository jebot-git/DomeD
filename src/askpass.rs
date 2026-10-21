//! Select the native sudo password helper for children of a DomeD session.
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn select(existing: Option<OsString>, compositor: &Path) -> Option<OsString> {
    // An explicit override (including an empty value) belongs to the caller.
    if existing.is_some() {
        return existing;
    }
    let sibling = compositor.parent()?.join("dome-askpass");
    let selected = [sibling.as_path(), Path::new("/usr/local/bin/dome-askpass")]
        .into_iter()
        .find(|path| {
            path.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
        .map(|path| path.as_os_str().to_owned());
    selected
}

/// Set before publishing the ready client environment and starting services.
pub fn configure() {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(helper) = select(std::env::var_os("SUDO_ASKPASS"), &executable) {
            std::env::set_var("SUDO_ASKPASS", helper);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_executable_sibling() {
        let temporary = std::env::temp_dir().join(format!("dome-askpass-test-{}", std::process::id()));
        std::fs::create_dir_all(&temporary).unwrap();
        let helper = temporary.join("dome-askpass");
        std::fs::write(&helper, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(select(None, &temporary.join("dome-compositor")), Some(helper.into_os_string()));
        std::fs::remove_dir_all(temporary).unwrap();
    }
    #[test]
    fn preserves_explicit_helper_and_opt_out() {
        let executable = Path::new("/unused/dome-compositor");
        for value in ["/custom/askpass", ""] {
            assert_eq!(select(Some(value.into()), executable), Some(value.into()));
        }
    }
}
