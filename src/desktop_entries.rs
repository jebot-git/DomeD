//! XDG application catalog. Execution is delegated to GIO, never a shell parser.
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize)]
pub struct DesktopAction {
    pub id: String,
    pub name: String,
    pub icon: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct App {
    pub id: usize,
    pub desktop_id: String,
    pub name: String,
    pub generic_name: String,
    pub comment: String,
    pub icon: String,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub startup_wm_class: String,
    pub actions: Vec<DesktopAction>,
    pub visible: bool,
    #[serde(skip)]
    pub path: PathBuf,
}
impl App {
    pub fn matches(&self, app_id: &str) -> bool {
        let id = app_id.strip_suffix(".desktop").unwrap_or(app_id);
        self.desktop_id.trim_end_matches(".desktop") == id
            || (!self.startup_wm_class.is_empty() && self.startup_wm_class.eq_ignore_ascii_case(id))
    }
    pub fn search(&self, query: &str) -> bool {
        let text = format!(
            "{} {} {} {} {}",
            self.name,
            self.generic_name,
            self.comment,
            self.desktop_id,
            self.keywords.join(" ")
        )
        .to_lowercase();
        self.visible
            && query
                .to_lowercase()
                .split_whitespace()
                .all(|word| text.contains(word))
    }
}
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(c) => out.push(c),
                None => out.push('\\'),
            }
        } else {
            out.push(c)
        }
    }
    out
}
fn list(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut value = String::new();
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            value.push('\\');
            value.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ';' {
            if !value.is_empty() {
                out.push(unescape(&value));
                value.clear();
            }
        } else {
            value.push(c)
        }
    }
    if !value.is_empty() {
        out.push(unescape(&value));
    }
    out
}
fn localized(group: &BTreeMap<String, String>, key: &str, locale: &str) -> String {
    let parts: Vec<_> = locale.split('@').collect();
    let base = parts[0].split('.').next().unwrap_or("");
    let language = base.split('_').next().unwrap_or("");
    let mut candidates = vec![];
    if let Some(modifier) = parts.get(1) {
        candidates.push(format!("{base}@{modifier}"));
    }
    candidates.push(base.to_string());
    if let Some(modifier) = parts.get(1) {
        candidates.push(format!("{language}@{modifier}"));
    }
    candidates.push(language.into());
    for l in candidates {
        if let Some(value) = group.get(&format!("{key}[{l}]")) {
            return unescape(value);
        }
    }
    unescape(group.get(key).map(String::as_str).unwrap_or(""))
}
fn executable(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let valid = |p: &Path| {
        p.is_file()
            && p.metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    };
    if name.contains('/') {
        valid(Path::new(name))
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .any(|p| valid(&p.join(name)))
    }
}
pub fn parse(
    text: &str,
    path: PathBuf,
    desktop_id: String,
    locale: &str,
    desktops: &[&str],
) -> Option<App> {
    let mut groups: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].into();
        } else if let Some((key, value)) = line.split_once('=') {
            groups
                .entry(section.clone())
                .or_default()
                .insert(key.trim().into(), value.trim().into());
        }
    }
    let g = groups.get("Desktop Entry")?;
    let get = |key| g.get(key).map(String::as_str).unwrap_or("");
    if get("Type") != "Application"
        || get("Hidden") == "true"
        || (!g.contains_key("Exec") && get("DBusActivatable") != "true")
    {
        return None;
    }
    let name = localized(g, "Name", locale);
    if name.is_empty() {
        return None;
    }
    let only = list(get("OnlyShowIn"));
    let not = list(get("NotShowIn"));
    let visible = get("NoDisplay") != "true"
        && (only.is_empty() || only.iter().any(|v| desktops.contains(&v.as_str())))
        && !not.iter().any(|v| desktops.contains(&v.as_str()))
        && (get("TryExec").is_empty() || executable(&unescape(get("TryExec"))));
    let actions = list(get("Actions"))
        .iter()
        .filter_map(|id| {
            let group = groups.get(&format!("Desktop Action {id}"))?;
            let name = localized(group, "Name", locale);
            if name.is_empty() || !group.contains_key("Exec") {
                return None;
            }
            Some(DesktopAction {
                id: id.clone(),
                name,
                icon: localized(group, "Icon", locale),
            })
        })
        .collect();
    Some(App {
        id: 0,
        desktop_id,
        name,
        generic_name: localized(g, "GenericName", locale),
        comment: localized(g, "Comment", locale),
        icon: localized(g, "Icon", locale),
        categories: list(get("Categories")),
        keywords: list(&localized(g, "Keywords", locale)),
        startup_wm_class: unescape(get("StartupWMClass")),
        actions,
        visible,
        path,
    })
}
fn files(root: &Path, dir: &Path, depth: u8, out: &mut Vec<(String, PathBuf)>) {
    if depth > 16 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() && !path.is_symlink() {
            files(root, &path, depth + 1, out);
        } else if path.extension().is_some_and(|s| s == "desktop") && path.is_file() {
            if let Ok(relative) = path.strip_prefix(root) {
                out.push((relative.to_string_lossy().replace('/', "-"), path));
            }
        }
    }
}
pub fn catalog(dirs: &[PathBuf], locale: &str, desktops: &[&str]) -> Vec<App> {
    let mut seen = HashSet::new();
    let mut result = vec![];
    for dir in dirs {
        let root = dir.join("applications");
        let mut entries = vec![];
        files(&root, &root, 0, &mut entries);
        for (id, path) in entries {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&path) {
                if let Some(app) = parse(&text, path, id, locale, desktops) {
                    result.push(app);
                }
            }
        }
    }
    result.sort_by_key(|a| a.name.to_lowercase());
    for (id, a) in result.iter_mut().enumerate() {
        a.id = id;
    }
    result
}
pub fn apps() -> Vec<App> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs = vec![PathBuf::from(
        std::env::var("XDG_DATA_HOME").unwrap_or(format!("{home}/.local/share")),
    )];
    dirs.extend(
        std::env::split_paths(
            &std::env::var_os("XDG_DATA_DIRS").unwrap_or("/usr/local/share:/usr/share".into()),
        )
        .filter(|p| p.is_absolute()),
    );
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
        .unwrap_or("C".into());
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or("Dome".into());
    catalog(&dirs, &locale, &desktop.split(':').collect::<Vec<_>>())
}
pub fn launch(app: &App, socket: &str) -> Result<(), String> {
    std::process::Command::new("gio")
        .arg("launch")
        .arg(&app.path)
        .env("WAYLAND_DISPLAY", socket)
        .env_remove("WAYLAND_SOCKET")
        .env("XDG_CURRENT_DESKTOP", "Dome")
        .env("GDK_BACKEND", "wayland")
        .env("QT_QPA_PLATFORM", "wayland")
        .env("SDL_VIDEODRIVER", "wayland")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot start gio: {e}"))
}
/// GDesktopAppInfo owns desktop-action Exec field expansion and activation semantics.
pub fn launch_action(app: &App, action: &str, socket: &str) -> Result<(), String> {
    if !app.actions.iter().any(|a| a.id == action) {
        return Err("Unknown desktop action".into());
    }
    use std::ffi::{c_char, c_void, CString};
    let path = CString::new(app.path.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
    let action = CString::new(action).map_err(|e| e.to_string())?;
    unsafe {
        let gio = libloading::Library::new("libgio-2.0.so.0").map_err(|e| e.to_string())?;
        let object = libloading::Library::new("libgobject-2.0.so.0").map_err(|e| e.to_string())?;
        let new: libloading::Symbol<unsafe extern "C" fn(*const c_char) -> *mut c_void> = gio
            .get(b"g_desktop_app_info_new_from_filename\0")
            .map_err(|e| e.to_string())?;
        let context: libloading::Symbol<unsafe extern "C" fn() -> *mut c_void> = gio
            .get(b"g_app_launch_context_new\0")
            .map_err(|e| e.to_string())?;
        let set: libloading::Symbol<
            unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
        > = gio
            .get(b"g_app_launch_context_setenv\0")
            .map_err(|e| e.to_string())?;
        let launch: libloading::Symbol<
            unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void),
        > = gio
            .get(b"g_desktop_app_info_launch_action\0")
            .map_err(|e| e.to_string())?;
        let unref: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> =
            object.get(b"g_object_unref\0").map_err(|e| e.to_string())?;
        let info = new(path.as_ptr());
        if info.is_null() {
            return Err("Invalid desktop entry".into());
        }
        let ctx = context();
        for (key, value) in [
            ("WAYLAND_DISPLAY", socket),
            ("XDG_CURRENT_DESKTOP", "Dome"),
            ("GDK_BACKEND", "wayland"),
            ("QT_QPA_PLATFORM", "wayland"),
            ("SDL_VIDEODRIVER", "wayland"),
        ] {
            let k = CString::new(key).unwrap();
            let v = CString::new(value).map_err(|e| e.to_string())?;
            set(ctx, k.as_ptr(), v.as_ptr());
        }
        let unset: libloading::Symbol<unsafe extern "C" fn(*mut c_void, *const c_char)> = gio
            .get(b"g_app_launch_context_unsetenv\0").map_err(|e| e.to_string())?;
        unset(ctx, c"WAYLAND_SOCKET".as_ptr());
        launch(info, action.as_ptr(), ctx);
        unref(ctx);
        unref(info);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_catalog_metadata_visibility_and_actions() {
        let text="[Desktop Entry]\nType=Application\nName=Editor\nName[cs]=Editor CS\nExec=edit %U\nIcon=editor\nCategories=Utility;TextEditor;\nKeywords=write;code;\nStartupWMClass=Edit\nTerminal=true\nActions=new;\n[Desktop Action new]\nName=New Window\nName[cs]=Nové okno\nExec=edit --new\n";
        let a = parse(
            text,
            "/tmp/edit.desktop".into(),
            "org.example.Edit.desktop".into(),
            "cs_CZ.UTF-8",
            &["Dome"],
        )
        .unwrap();
        assert_eq!(a.name, "Editor CS");
        assert_eq!(a.actions[0].name, "Nové okno");
        assert!(a.matches("org.example.Edit"));
        assert!(a.matches("Edit"));
        assert!(a.search("code"));
        assert!(a.visible);
        let hidden = parse(
            &text.replace("Terminal=true", "NoDisplay=true"),
            a.path.clone(),
            a.desktop_id.clone(),
            "C",
            &["Dome"],
        )
        .unwrap();
        assert!(!hidden.search(""));
        assert!(parse(
            &text.replace("Terminal=true", "Hidden=true"),
            a.path,
            a.desktop_id,
            "C",
            &["Dome"]
        )
        .is_none());
    }
    #[test]
    fn nested_ids_and_user_hidden_tombstones() {
        let path = std::env::temp_dir().join(format!("dome-xdg-test-{}", std::process::id()));
        let local = path.join("local");
        let system = path.join("system");
        fs::create_dir_all(local.join("applications")).unwrap();
        fs::create_dir_all(system.join("applications/foo")).unwrap();
        let text = "[Desktop Entry]\nType=Application\nName=Test\nExec=true\n";
        fs::write(system.join("applications/foo/bar.desktop"), text).unwrap();
        fs::write(
            local.join("applications/foo-bar.desktop"),
            format!("{text}Hidden=true\n"),
        )
        .unwrap();
        assert!(catalog(&[local.clone(), system.clone()], "C", &["Dome"]).is_empty());
        fs::remove_file(local.join("applications/foo-bar.desktop")).unwrap();
        assert_eq!(
            catalog(&[local, system], "C", &["Dome"])[0].desktop_id,
            "foo-bar.desktop"
        );
        fs::remove_dir_all(path).unwrap();
    }
}
