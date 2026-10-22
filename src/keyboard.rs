//! Persistent XKB settings shared by the desktop and XR seat.
use serde::{Deserialize, Serialize};

/// Default input choices; Fcitx keyboard-rs uses XKB's Cyrillic Serbian default.
pub const INPUT_METHODS: &[(&str, &str)] = &[
    ("keyboard-us", "US"),
    ("anthy", "あ"),
    ("keyboard-cz", "CZ"),
    ("hangul", "한"),
    ("keyboard-rs", "СР"),
];

pub fn input_method_label(method: &str) -> Option<&'static str> {
    INPUT_METHODS
        .iter()
        .find(|(id, _)| *id == method)
        .map(|(_, label)| *label)
}
/// Only use a national flag for a known layout/engine; custom engines stay neutral.
pub fn input_method_flag(method: &str) -> &'static str {
    match method {
        "anthy" | "mozc" => "jp",
        "hangul" => "kr",
        _ => match method.strip_prefix("keyboard-").unwrap_or(method).split('-').next().unwrap_or("") {
            "us" => "us", "gb" => "gb", "jp" => "jp", "cz" => "cz",
            "kr" => "kr", "rs" => "rs", "de" => "de", "fr" => "fr",
            _ => "generic",
        },
    }
}

/// Preserve custom ordering/layout overrides; add only engines actually installed.
pub fn complete_input_methods(
    existing: &[(String, String)],
    available: &[String],
) -> Vec<(String, String)> {
    let mut result = existing.to_vec();
    for (id, _) in INPUT_METHODS {
        if available.iter().any(|name| name == id) && !result.iter().any(|(name, _)| name == id) {
            result.push(((*id).into(), String::new()));
        }
    }
    result
}

pub fn next_input_method<'a>(current: &str, methods: &'a [(String, String)]) -> Option<&'a str> {
    if methods.is_empty() {
        return None;
    }
    let index = methods
        .iter()
        .position(|(id, _)| id == current)
        .map_or(0, |i| (i + 1) % methods.len());
    Some(&methods[index].0)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keyboard {
    pub ime: bool,
    pub layout: String,
    pub variant: String,
    pub options: String,
    pub switch_shortcut: SwitchShortcut,
    pub active: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchShortcut {
    SuperSpace,
    SuperShiftSpace,
    SuperAltSpace,
    CtrlAltSpace,
}
impl SwitchShortcut {
    pub fn label(self) -> &'static str {
        match self {
            Self::SuperSpace => "Super+Space",
            Self::SuperShiftSpace => "Super+Shift+Space",
            Self::SuperAltSpace => "Super+Alt+Space",
            Self::CtrlAltSpace => "Ctrl+Alt+Space",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::SuperSpace => Self::SuperAltSpace,
            Self::SuperShiftSpace => Self::SuperAltSpace,
            Self::SuperAltSpace => Self::CtrlAltSpace,
            Self::CtrlAltSpace => Self::SuperSpace,
        }
    }
    pub fn matches(self, logo: bool, ctrl: bool, alt: bool, shift: bool) -> bool {
        match self {
            Self::SuperSpace => logo && !ctrl && !alt && !shift,
            Self::SuperShiftSpace => logo && shift && !ctrl && !alt,
            Self::SuperAltSpace => logo && alt && !ctrl && !shift,
            Self::CtrlAltSpace => ctrl && alt && !logo && !shift,
        }
    }
}
impl Default for Keyboard {
    fn default() -> Self {
        Self {
            ime: true,
            layout: "us".into(),
            variant: String::new(),
            options: String::new(),
            switch_shortcut: SwitchShortcut::SuperSpace,
            active: 0,
        }
    }
}
impl Keyboard {
    pub fn validate(&self) -> Result<(), String> {
        if self.layout.is_empty()
            || self.layout.split(',').count() > 4
            || self
                .layout
                .split(',')
                .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            || [&self.variant, &self.options].iter().any(|s| {
                s.len() > 256
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_,:-".contains(&b))
            })
            || self.active >= self.layout.split(',').count()
        {
            return Err(
                "Use 1–4 comma-separated XKB layouts; variants and options must be XKB names"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn label(&self) -> String {
        self.layout
            .split(',')
            .nth(self.active)
            .unwrap_or("us")
            .to_uppercase()
    }
    fn path() -> std::path::PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".config")
            })
            .join("dome/keyboard.json")
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|v| serde_json::from_slice::<Self>(&v).ok())
            .filter(|v| v.validate().is_ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap())
            .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, path).map_err(|e| e.to_string())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_flags_cover_default_methods_and_preserve_unknown_engines() {
        for (method, country) in [("keyboard-us", "us"), ("anthy", "jp"),
            ("keyboard-cz", "cz"), ("hangul", "kr"), ("keyboard-rs-latin", "rs"),
            ("custom-engine", "generic")] {
            assert_eq!(input_method_flag(method), country);
        }
    }
    #[test]
    fn input_choices_preserve_custom_methods_and_skip_uninstalled_engines() {
        let existing = vec![
            ("keyboard-us".into(), "us".into()),
            ("custom".into(), "cz".into()),
        ];
        let available = vec![
            "keyboard-us".into(),
            "keyboard-cz".into(),
            "keyboard-rs".into(),
        ];
        let methods = complete_input_methods(&existing, &available);
        assert_eq!(&methods[..2], &existing);
        assert_eq!(methods[2].0, "keyboard-cz");
        assert_eq!(methods[3].0, "keyboard-rs");
        assert_eq!(complete_input_methods(&methods, &available), methods);
        assert_eq!(next_input_method("keyboard-us", &methods), Some("custom"));
        assert_eq!(
            next_input_method("keyboard-rs", &methods),
            Some("keyboard-us")
        );
        assert_eq!(next_input_method("unknown", &methods), Some("keyboard-us"));
        assert_eq!(next_input_method("keyboard-us", &[]), None);
        assert_eq!(input_method_label("hangul"), Some("한"));
        assert_eq!(input_method_label("keyboard-rs"), Some("СР"));
    }
    #[test]
    fn shortcuts_do_not_conflict_with_launcher_or_extra_modifiers() {
        for shortcut in [
            SwitchShortcut::SuperShiftSpace,
            SwitchShortcut::SuperAltSpace,
            SwitchShortcut::CtrlAltSpace,
        ] {
            assert!(!shortcut.matches(true, false, false, false));
            assert!(!shortcut.matches(true, true, true, true));
        }
        assert!(SwitchShortcut::SuperShiftSpace.matches(true, false, false, true));
    }
    #[test]
    fn xkb_settings_validate_groups_and_names() {
        let mut c = Keyboard::default();
        assert!(c.validate().is_ok());
        c.active = 2;
        assert!(c.validate().is_err());
        c.active = 0;
        c.layout = "us,,cz".into();
        assert!(c.validate().is_err());
        c.layout = "us,cz".into();
        c.options = "grp:alt_shift_toggle,caps:escape".into();
        assert!(c.validate().is_ok());
        c.variant = "qwerty;bad".into();
        assert!(c.validate().is_err());
    }
}

/// KDE's kcminputrc uses 0=on, 1=off and 2=leave unchanged.
pub fn numlock_setting(config: &str) -> Option<bool> {
    let mut keyboard = false;
    let mut result = None;
    for line in config.lines().map(str::trim) {
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            keyboard = line == "[Keyboard]";
            continue;
        }
        if keyboard {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "NumLock" {
                    result = match value.trim() {
                        "0" => Some(true),
                        "1" => Some(false),
                        _ => None,
                    };
                }
            }
        }
    }
    result
}
pub fn startup_numlock() -> Option<bool> {
    let directory = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    std::fs::read_to_string(directory.join("kcminputrc"))
        .ok()
        .and_then(|s| numlock_setting(&s))
}
#[cfg(test)]
mod numlock_tests {
    use super::*;
    #[test]
    fn kde_numlock_preferences_and_other_groups() {
        assert_eq!(
            numlock_setting("[Keyboard]\nNumLock=0\n[Mouse]\nNumLock=1"),
            Some(true)
        );
        assert_eq!(numlock_setting("[Keyboard]\nNumLock = 1"), Some(false));
        for value in ["2", "", "garbage"] {
            assert_eq!(
                numlock_setting(&format!("[Keyboard]\nNumLock={value}")),
                None
            );
        }
        assert_eq!(numlock_setting("[Mouse]\nNumLock=0"), None);
    }
}
