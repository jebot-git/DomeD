//! Session-scoped GTK/Qt theme and desktop Settings portal backend.
use dome::appearance::Appearance;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
};
use zbus::zvariant::{OwnedValue, Str, Value};

const BUS: &str = "org.freedesktop.impl.portal.desktop.dome";
const OBJECT: &str = "/org/freedesktop/portal/desktop";
type Settings = HashMap<String, HashMap<String, OwnedValue>>;

#[derive(Clone)]
struct Snapshot {
    appearance: Appearance,
    theme: String,
}
struct Portal(Arc<Mutex<Snapshot>>);
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.portal.Error")]
enum PortalError {
    NotFound(String),
}

fn string(value: impl Into<String>) -> OwnedValue {
    OwnedValue::from(Str::from(value.into()))
}
fn family(appearance: &Appearance) -> &str {
    appearance
        .font
        .split_once(" / ")
        .map_or(appearance.font.as_str(), |(family, _)| family)
}
fn font_name(appearance: &Appearance) -> String {
    // Dome stores physical pixels; toolkit font descriptions use points at 96 dpi.
    format!(
        "{} {}",
        family(appearance),
        appearance.font_size as f64 * 0.75
    )
}
fn settings(snapshot: &Snapshot) -> Settings {
    let a = &snapshot.appearance;
    let rgb: Vec<_> = [1, 3, 5]
        .iter()
        .map(|i| u8::from_str_radix(&a.accent[*i..*i + 2], 16).unwrap_or(0) as f64 / 255.)
        .collect();
    let accent = OwnedValue::try_from(Value::from((rgb[0], rgb[1], rgb[2]))).unwrap();
    HashMap::from([
        (
            "org.freedesktop.appearance".into(),
            HashMap::from([
                (
                    "color-scheme".into(),
                    OwnedValue::from(if a.light { 2u32 } else { 1u32 }),
                ),
                ("accent-color".into(), accent),
                ("contrast".into(), OwnedValue::from(0u32)),
            ]),
        ),
        (
            "org.gnome.desktop.interface".into(),
            HashMap::from([
                ("gtk-theme".into(), string(&snapshot.theme)),
                ("font-name".into(), string(font_name(a))),
                ("monospace-font-name".into(), string(font_name(a))),
                (
                    "color-scheme".into(),
                    string(if a.light {
                        "prefer-light"
                    } else {
                        "prefer-dark"
                    }),
                ),
                ("icon-theme".into(), string("breeze")),
                ("text-scaling-factor".into(), OwnedValue::from(1f64)),
            ]),
        ),
    ])
}
fn matches(namespace: &str, patterns: &[String]) -> bool {
    patterns.is_empty()
        || patterns.iter().any(|p| {
            p.is_empty()
                || p == namespace
                || p.strip_suffix(".*")
                    .is_some_and(|base| namespace.starts_with(&format!("{base}.")))
        })
}
#[zbus::interface(name = "org.freedesktop.impl.portal.Settings")]
impl Portal {
    fn read_all(&self, namespaces: Vec<String>) -> Settings {
        settings(&self.0.lock().unwrap())
            .into_iter()
            .filter(|(ns, _)| matches(ns, &namespaces))
            .collect()
    }
    fn read(&self, namespace: &str, key: &str) -> Result<OwnedValue, PortalError> {
        settings(&self.0.lock().unwrap())
            .remove(namespace)
            .and_then(|mut values| values.remove(key))
            .ok_or_else(|| PortalError::NotFound(format!("Unknown setting {namespace}.{key}")))
    }
    #[zbus(property)]
    fn version(&self) -> u32 {
        2
    }
}
#[cfg(test)]
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn css_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
fn atomic_write(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes)?;
    std::fs::rename(temp, path)
}
fn write_theme(root: &Path, appearance: Appearance) -> std::io::Result<Snapshot> {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&appearance).unwrap().hash(&mut hash);
    let theme = format!("Dome-{:016x}", hash.finish());
    let (bg, fg, base, muted) = if appearance.light {
        ("#e5e9ef", "#1e252e", "#ffffff", "#687484")
    } else {
        ("#1f252d", "#e7ebf1", "#171c23", "#929aa5")
    };
    let dark = if appearance.light { "" } else { "-dark" };
    let accent = &appearance.accent;
    let colors = format!("@define-color theme_bg_color {bg};\n@define-color theme_fg_color {fg};\n@define-color theme_base_color {base};\n@define-color theme_text_color {fg};\n@define-color theme_selected_bg_color {accent};\n@define-color theme_selected_fg_color #101820;\n@define-color insensitive_fg_color {muted};\n@define-color insensitive_bg_color {bg};\n@define-color insensitive_base_color {base};\n@define-color theme_unfocused_bg_color {bg};\n@define-color theme_unfocused_fg_color {fg};\n@define-color theme_unfocused_text_color {fg};\n@define-color theme_unfocused_selected_bg_color {accent};\n@define-color theme_unfocused_selected_fg_color #101820;\n@define-color accent_bg_color {accent};\n@define-color accent_fg_color #101820;\n");
    let font = format!(
        "* {{ font-family: {}; font-size: {}px; }}\n",
        css_string(family(&appearance)),
        appearance.font_size
    );
    let gtk3 = format!("@import url(\"resource:///org/gtk/libgtk/theme/Adwaita/gtk-contained{dark}.css\");\n{colors}{font}window, .background {{ background-color: @theme_bg_color; color: @theme_fg_color; }}\nentry, textview text {{ background-color: @theme_base_color; color: @theme_text_color; }}\n*:selected, selection {{ background-color: @theme_selected_bg_color; color: @theme_selected_fg_color; }}\n");
    atomic_write(
        &root.join("themes").join(&theme).join("gtk-3.0/gtk.css"),
        gtk3,
    )?;
    atomic_write(
        &root.join("themes").join(&theme).join("gtk-4.0/gtk.css"),
        format!("@import url(\"resource:///org/gtk/libgtk/theme/Default/gtk{dark}.css\");\n{colors}{font}"),
    )?;
    Ok(Snapshot { appearance, theme })
}
fn font_families() -> Vec<String> {
    crate::qt::bitmap_fonts().iter().map(|font| {
        font.id.split_once(" / ").map_or(font.id.as_str(), |(family, _)| family).to_owned()
    }).collect()
}
fn inherited_fontconfig(root: &Path) -> String {
    std::env::var("FONTCONFIG_FILE").ok()
        .filter(|file| Path::new(file).is_file() && Path::new(file) != root.join("fonts.conf")
            && !file.contains("/dome-appearance-") && !file.contains("/dome-fonts/"))
        .unwrap_or_else(|| "/etc/fonts/fonts.conf".into())
}
pub(crate) fn write_fonts(root: &Path) -> std::io::Result<PathBuf> {
    dome::font_export::export(root, &inherited_fontconfig(root), &font_families())
}
pub(crate) fn refresh_session_fonts(runtime: &Path) -> std::io::Result<PathBuf> {
    dome::font_export::refresh_session(runtime, &inherited_fontconfig(runtime), &font_families())
}

pub struct ToolkitAppearance {
    send: mpsc::Sender<Appearance>,
}
impl ToolkitAppearance {
    pub fn start(appearance: &Appearance) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let enabled = std::env::var("DOME_APPEARANCE_SERVICE").ok();
        if enabled.as_deref() != Some("1")
            && (std::env::var_os("DOME_DRM_TEST_DEVICE").is_some()
                || std::env::var_os("DOME_DRM_TEST_DEVICES").is_some())
        {
            return Ok(None);
        }
        if enabled.as_deref() == Some("0")
            || (enabled.as_deref() != Some("1")
                && std::env::var("DOME_LOGIN_SESSION").as_deref() != Ok("1"))
        {
            return Ok(None);
        }
        let runtime = PathBuf::from(
            std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is required")?,
        );
        let root = runtime.join(format!("dome-appearance-{}", std::process::id()));
        let snapshot = write_theme(&root, appearance.clone())?;
        let fonts = refresh_session_fonts(&runtime)?;
        let font_root = runtime.join("dome-fonts");
        let shared = Arc::new(Mutex::new(snapshot));
        let connection = zbus::blocking::connection::Builder::session()?
            .name(BUS)?
            .serve_at(OBJECT, Portal(shared.clone()))?
            .build()?;
        // Only this session and its descendants inherit these settings. Keep application configs intact.
        let mut data_dirs = vec![root.clone()];
        let previous = std::env::var_os("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        data_dirs.extend(std::env::split_paths(&previous));
        std::env::set_var("XDG_DATA_DIRS", std::env::join_paths(data_dirs)?);
        std::env::set_var("FONTCONFIG_FILE", fonts);
        std::env::set_var("GTK_USE_PORTAL", "1");
        let module = root.join("libdome-appearance.so");
        atomic_write(
            &module,
            include_bytes!(concat!(env!("OUT_DIR"), "/libdome-appearance.so")),
        )?;
        let mut modules: Vec<_> = std::env::var("GTK_MODULES")
            .unwrap_or_default()
            .split(':')
            .filter(|name| !name.is_empty() && !name.ends_with("libdome-appearance.so"))
            .map(str::to_owned)
            .collect();
        modules.push(module.to_string_lossy().into_owned());
        std::env::set_var("GTK_MODULES", modules.join(":"));
        std::env::remove_var("GTK_THEME");
        std::env::set_var("QT_QPA_PLATFORMTHEME", "gtk3");
        std::env::set_var("QT_STYLE_OVERRIDE", "Fusion");
        std::env::set_var("QT_QUICK_CONTROLS_STYLE", "Fusion");
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            for appearance in receive {
                // Repair missing or outdated font exports before telling clients
                // to resolve the newly selected face.
                if let Err(error) = write_fonts(&font_root) {
                    eprintln!("Toolkit font export: {error}");
                    continue;
                }
                match write_theme(&root, appearance) {
                    Ok(next) => {
                        let new = settings(&next);
                        let old = settings(&shared.lock().unwrap());
                        *shared.lock().unwrap() = next;
                        let mut changed = Vec::new();
                        for (namespace, values) in new {
                            for (key, value) in values {
                                if old.get(&namespace).and_then(|v| v.get(&key)) == Some(&value) {
                                    continue;
                                }
                                changed.push((namespace.clone(), key, value));
                            }
                        }
                        // GTK's Qt adapter clears its font cache on the theme-name change.
                        changed.sort_by_key(|(_, key, _)| key == "gtk-theme");
                        for (namespace, key, value) in changed {
                            if let Err(error) = connection.emit_signal(
                                None::<&str>,
                                OBJECT,
                                "org.freedesktop.impl.portal.Settings",
                                "SettingChanged",
                                &(&namespace, key, value),
                            ) {
                                eprintln!("Toolkit appearance notification: {error}");
                            }
                        }
                    }
                    Err(error) => eprintln!("Toolkit appearance update: {error}"),
                }
            }
        });
        Ok(Some(Self { send }))
    }
    pub fn update(&self, appearance: &Appearance) {
        let _ = self.send.send(appearance.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portal_values_follow_light_dark_accent_and_pixel_font_size() {
        let mut appearance = Appearance::default();
        let dark = settings(&Snapshot {
            appearance: appearance.clone(),
            theme: "Dome-dark".into(),
        });
        assert_eq!(
            u32::try_from(
                dark["org.freedesktop.appearance"]["color-scheme"]
                    .try_clone()
                    .unwrap()
            )
            .unwrap(),
            1
        );
        assert_eq!(font_name(&appearance), "Unifont 12");
        appearance.light = true;
        appearance.font_size = 24;
        let light = settings(&Snapshot {
            appearance,
            theme: "Dome-light".into(),
        });
        assert_eq!(
            u32::try_from(
                light["org.freedesktop.appearance"]["color-scheme"]
                    .try_clone()
                    .unwrap()
            )
            .unwrap(),
            2
        );
        assert!(matches(
            "org.gnome.desktop.interface",
            &["org.gnome.*".into()]
        ));
        assert!(!matches(
            "org.gnome.desktop.interface",
            &["org.gnom.*".into()]
        ));
        assert!(matches("anything", &["".into()]));
        assert_eq!(xml("<x&\""), "&lt;x&amp;&quot;");
    }
}
