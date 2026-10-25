//! Persistent DRM GPU choices, with stable PCI paths where available.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    pub path: String,
    pub node: String,
    pub label: String,
}

pub fn valid_path(value: &str) -> bool {
    let path = Path::new(value);
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else { return false; };
    if path.parent() == Some(Path::new("/dev/dri")) {
        return name.strip_prefix("card").is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
    }
    path.parent() == Some(Path::new("/dev/dri/by-path"))
        && name.starts_with("pci-") && name.ends_with("-card")
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"-:._".contains(&c))
}

pub fn same_device(a: &str, b: &str) -> bool {
    a == b || std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok()).is_some_and(|(a,b)| a == b)
}

fn product_name(database: &str, vendor: &str, device: &str) -> Option<String> {
    let mut in_vendor = false;
    for line in database.lines() {
        if line.starts_with('#') || line.is_empty() { continue; }
        if !line.starts_with('\t') {
            in_vendor = line.get(..4).is_some_and(|s| s.eq_ignore_ascii_case(vendor));
        } else if in_vendor && !line.starts_with("\t\t")
            && line.get(1..5).is_some_and(|s| s.eq_ignore_ascii_case(device)) {
            let name = line.get(5..)?.trim();
            return Some(name.rsplit_once('[').and_then(|(_, s)| s.strip_suffix(']')).unwrap_or(name).to_owned());
        }
    }
    None
}

/// No graphics contexts or device nodes are opened while listing choices.
pub fn discover() -> Vec<Gpu> {
    let database = std::fs::read_to_string("/usr/share/hwdata/pci.ids")
        .or_else(|_| std::fs::read_to_string("/usr/share/misc/pci.ids")).unwrap_or_default();
    discover_at(Path::new("/sys/class/drm"), Path::new("/dev/dri"), &database)
}

fn discover_at(sys: &Path, dev: &Path, database: &str) -> Vec<Gpu> {
    let mut choices = Vec::new();
    let Ok(entries) = std::fs::read_dir(sys) else { return choices; };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.strip_prefix("card").is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())) { continue; }
        let event = std::fs::read_to_string(entry.path().join("device/uevent")).unwrap_or_default();
        let value = |key: &str| event.lines().find_map(|s| s.strip_prefix(key));
        let Some((vendor, device)) = value("PCI_ID=").and_then(|s| s.split_once(':')) else { continue; };
        let node = dev.join(&name);
        if !node.exists() { continue; }
        let stable = value("PCI_SLOT_NAME=").map(|pci| dev.join("by-path").join(format!("pci-{pci}-card")))
            .filter(|path| path.exists()).unwrap_or_else(|| node.clone());
        let product = product_name(database, vendor, device).unwrap_or_else(|| format!("GPU {vendor}:{device}"));
        choices.push(Gpu { path: stable.to_string_lossy().into_owned(), node: node.to_string_lossy().into_owned(),
            label: format!("{product} ({name})") });
    }
    choices.sort_by(|a,b| a.node.cmp(&b.node));
    choices
}

pub fn cycle(current: Option<&str>, choices: &[Gpu], direction: i32) -> Option<String> {
    let index = match current {
        None => 0,
        Some(path) => match choices.iter().position(|gpu| same_device(path, &gpu.path)) {
            Some(index) => index + 1,
            None => return None,
        },
    };
    let next = (index as i32 + direction.signum()).rem_euclid(choices.len() as i32 + 1) as usize;
    next.checked_sub(1).map(|index| choices[index].path.clone())
}

pub fn label(current: Option<&str>, active: Option<&str>, choices: &[Gpu]) -> String {
    let label = |path: &str| choices.iter().find(|gpu| same_device(path, &gpu.path))
        .map(|gpu| gpu.label.clone()).unwrap_or_else(|| format!("Unavailable: {}", Path::new(path).file_name().unwrap_or_default().to_string_lossy()));
    match current {
        None => active.map_or_else(|| "Automatic".into(), |path| format!("Automatic · {}", label(path))),
        Some(path) => format!("{}{}", label(path), if active.is_some_and(|a| same_device(a,path)) { " · active" } else { "" }),
    }
}

/// A removed saved GPU must not prevent the next desktop login.
pub fn available_choice(current: Option<&str>, choices: &[Gpu]) -> Option<PathBuf> {
    current.and_then(|path| choices.iter().find(|gpu| same_device(path, &gpu.path)))
        .map(|gpu| PathBuf::from(&gpu.node))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_drm_card_paths_can_be_saved() {
        for path in ["/dev/dri/card0", "/dev/dri/card12", "/dev/dri/by-path/pci-0000:03:00.0-card"] { assert!(valid_path(path)); }
        for path in ["", "card0", "/dev/dri/card", "/dev/dri/renderD128", "/dev/dri/card1/../card2", "/tmp/card0", "/dev/dri/by-path/../../etc/passwd"] { assert!(!valid_path(path)); }
    }
    #[test]
    fn selector_wraps_through_automatic_and_recovers_removed_devices() {
        let choices = vec![Gpu {path:"/dev/dri/card8".into(),node:"/dev/dri/card8".into(),label:"First".into()},
            Gpu {path:"/dev/dri/card9".into(),node:"/dev/dri/card9".into(),label:"Second".into()}];
        assert_eq!(cycle(None,&choices,1),Some(choices[0].path.clone()));
        assert_eq!(cycle(None,&choices,-1),Some(choices[1].path.clone()));
        assert_eq!(cycle(Some(&choices[1].path),&choices,1),None);
        assert_eq!(cycle(Some(&choices[0].path),&choices,-1),None);
        assert_eq!(cycle(Some("/dev/dri/card100"),&choices,1),None);
        assert!(available_choice(Some("/dev/dri/card100"),&choices).is_none());
        assert_eq!(cycle(None,&[],1),None);
    }
    #[test]
    fn pci_product_names_are_scoped_to_the_correct_vendor() {
        let ids="1234  Other\n\t56a0  Wrong GPU\n8086  Intel\n\t56a0  DG2 [Arc A770]\n\t\t8086 1020  Board\n";
        assert_eq!(product_name(ids,"8086","56A0"),Some("Arc A770".into()));
        assert_eq!(product_name(ids,"1234","56a0"),Some("Wrong GPU".into()));
        assert_eq!(product_name(ids,"8086","ffff"),None);
    }
    #[test]
    fn discovery_ignores_connectors_and_uses_persistent_pci_aliases() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("dome-gpu-inventory-{}",std::process::id()));
        let sys = root.join("sys"); let dev = root.join("dev");
        std::fs::create_dir_all(sys.join("card2/device")).unwrap();
        std::fs::create_dir_all(sys.join("card2-DP-1/device")).unwrap();
        std::fs::create_dir_all(dev.join("by-path")).unwrap();
        std::fs::write(sys.join("card2/device/uevent"),"PCI_ID=8086:56A0\nPCI_SLOT_NAME=0000:03:00.0\n").unwrap();
        std::fs::write(dev.join("card2"),b"").unwrap();
        let stable = dev.join("by-path/pci-0000:03:00.0-card");
        symlink(dev.join("card2"),&stable).unwrap();
        let gpus = discover_at(&sys,&dev,"8086  Intel\n\t56a0  DG2 [Arc A770]\n");
        assert_eq!(gpus.len(),1);
        assert_eq!(gpus[0].path,stable.to_string_lossy());
        assert_eq!(gpus[0].label,"Arc A770 (card2)");
        assert!(same_device(&gpus[0].node,&gpus[0].path));
        assert_eq!(available_choice(Some(&gpus[0].path),&gpus),Some(dev.join("card2")));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn old_profiles_migrate_and_gpu_selection_roundtrips_without_altering_displays() {
        let mut config: crate::Config = serde_json::from_str("{}").unwrap();
        assert_eq!(config.gpu_device,None);
        config.displays.push(crate::DisplayConfig { name:"DP-1".into(), ..Default::default() });
        config.gpu_device=Some("/dev/dri/by-path/pci-0000:03:00.0-card".into());
        config.validate().unwrap();
        let loaded: crate::Config=serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        assert_eq!(config,loaded);
        config.gpu_device=Some("/tmp/not-a-gpu".into());
        assert!(config.validate().is_err());
    }
}
