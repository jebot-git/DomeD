//! Export the same bitmap assets used by the shell for client toolkits.
use std::path::{Path, PathBuf};

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, path)
}

/// Export every registered face and a Fontconfig allowlist for distributions
/// that disable bitmap fonts. Re-running this repairs an older session's export
/// without replacing its themes or restarting the compositor.
pub fn export(root: &Path, inherited: &str, _families: &[String]) -> std::io::Result<PathBuf> {
    let directory = root.join("fonts");
    let mut accepted = String::new();
    for (name, bytes) in crate::bundled_fonts::FONTS {
        let path = directory.join(name);
        write_if_changed(&path, bytes)?;
        accepted.push_str(&format!(
            "<pattern><patelt name=\"file\"><string>{}</string></patelt></pattern>",
            xml(&path.to_string_lossy())
        ));
    }
    let index = include_bytes!("../assets/fonts/fonts.index");
    write_if_changed(&directory.join("fonts.index"), index)?;
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"))
        .join("de-xr/bdf2x-v1");
    std::fs::create_dir_all(&cache)?;
    accepted.push_str("<pattern><patelt name=\"fontformat\"><string>BDF</string></patelt></pattern>");
    let path = root.join("fonts.conf");
    let configuration = format!("<?xml version=\"1.0\"?><!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\"><fontconfig><include ignore_missing=\"yes\">{}</include><dir>{}</dir><dir>{}</dir><cachedir>{}</cachedir><selectfont><rejectfont><glob>*</glob></rejectfont><acceptfont><glob>*.bdf</glob><glob>*.bdf.gz</glob><glob>*.BDF</glob><glob>*.BDF.gz</glob>{accepted}</acceptfont></selectfont></fontconfig>", xml(inherited), xml(&directory.to_string_lossy()), xml(&cache.to_string_lossy()), xml(&root.join("font-cache").to_string_lossy()));
    write_if_changed(&path, configuration.as_bytes())?;
    Ok(path)
}

/// A stable path lets launchers which outlive a compositor keep the current
/// bundle. Refresh legacy exports too, so an upgrade does not strand clients
/// still inheriting a previous compositor's FONTCONFIG_FILE.
pub fn refresh_session(
    runtime: &Path,
    inherited: &str,
    families: &[String],
) -> std::io::Result<PathBuf> {
    let configuration = export(&runtime.join("dome-fonts"), inherited, families)?;
    for entry in std::fs::read_dir(runtime)? {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("dome-appearance-")
            && entry.path().join("fonts.conf").is_file()
        {
            if let Err(error) = export(&entry.path(), inherited, families) {
                eprintln!(
                    "Could not refresh legacy font export {}: {error}",
                    entry.path().display()
                );
            }
        }
    }
    Ok(configuration)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refreshing_an_old_export_adds_every_registered_face_and_keeps_unchanged_files() {
        let root = std::env::temp_dir().join(format!(
            "dome-font-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("fonts.conf"), "old session").unwrap();
        let config = export(&root, "/etc/fonts/fonts.conf", &["A&B".into()]).unwrap();
        let data = std::fs::read_to_string(&config).unwrap();
        assert!(!data.contains("A&amp;B"), "Family aliases must not admit outline fonts with the same name");
        assert!(data.contains("dunggeunmo-v1.3/DungGeunMo.bdf"));
        for (name, bytes) in crate::bundled_fonts::FONTS {
            assert_eq!(
                std::fs::read(root.join("fonts").join(name)).unwrap(),
                *bytes
            );
        }
        let modified = std::fs::metadata(&config).unwrap().modified().unwrap();
        export(&root, "/etc/fonts/fonts.conf", &["A&B".into()]).unwrap();
        assert_eq!(
            std::fs::metadata(&config).unwrap().modified().unwrap(),
            modified
        );
        let legacy = root.join("dome-appearance-123");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("fonts.conf"), "old parent export").unwrap();
        let stable = refresh_session(&root, "/etc/fonts/fonts.conf", &[]).unwrap();
        assert_eq!(stable, root.join("dome-fonts/fonts.conf"));
        assert!(std::fs::read_to_string(legacy.join("fonts.conf"))
            .unwrap()
            .contains("DungGeunMo.bdf"));
        assert_eq!(
            refresh_session(&root, "/etc/fonts/fonts.conf", &[]).unwrap(),
            stable
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
