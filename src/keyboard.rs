//! Persistent XKB settings shared by the desktop and XR seat.
use serde::{Deserialize, Serialize};
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
