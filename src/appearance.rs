//! Persistent shell colours and font preferences.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub light: bool,
    pub accent: String,
    pub background: [f32; 3],
    pub opacity: u8,
    pub font: String,
    pub font_size: u16,
    #[serde(rename = "hand_resume_ms", deserialize_with = "crate::scene::discard_retired_setting", skip_serializing)]
    pub retired_pointer_delay: (),
}
#[cfg(test)]
mod font_tests {
    use super::Appearance;
    #[test]
    fn old_settings_receive_the_unicode_bitmap_default() {
        let old = r##"{"light":true,"accent":"#61cfa5","background":[0.1,0.2,0.3],"opacity":208,"hand_resume_ms":800}"##;
        let appearance: Appearance = serde_json::from_str(old).unwrap();
        assert!(appearance.light);
        assert_eq!(appearance.font, "GNU Unifont / Sans-Serif");
        assert_eq!(appearance.font_size, 16);
        assert!(appearance.validate().is_ok());
        let restored: Appearance = serde_json::from_slice(&serde_json::to_vec(&appearance).unwrap()).unwrap();
        assert_eq!(restored.font, appearance.font);
        assert_eq!(restored.font_size, appearance.font_size);
    }
    #[test]
    fn invalid_font_settings_are_rejected() {
        let mut appearance = Appearance::default();
        appearance.font_size = 0;
        assert!(appearance.validate().is_err());
        appearance.font_size = 16;
        appearance.font = "bad\nfont".into();
        assert!(appearance.validate().is_err());
    }
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            light: false,
            accent: "#6babff".into(),
            background: [0.025, 0.045, 0.06],
            opacity: 248,
            font: "GNU Unifont / Sans-Serif".into(),
            font_size: 16,
            retired_pointer_delay: (),
        }
    }
}
impl Appearance {
    pub fn validate(&self) -> Result<(), String> {
        if self.accent.len() != 7
            || !self.accent.starts_with('#')
            || !self.accent[1..].bytes().all(|b| b.is_ascii_hexdigit())
            || self.opacity < 160
            || self.font.is_empty()
            || self.font.len() > 256
            || self.font.chars().any(char::is_control)
            || !(8..=24).contains(&self.font_size)
            || self
                .background
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("Invalid appearance settings".into());
        }
        Ok(())
    }
    fn path() -> std::path::PathBuf {
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".config")
            });
        root.join("dome/appearance.json")
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|s| s.validate().is_ok())
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionAction {
    Lock,
    SwitchUser,
    Suspend,
    Logout,
    Restart,
    Shutdown,
}
impl SessionAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lock => "Lock Screen",
            Self::SwitchUser => "Switch User",
            Self::Suspend => "Sleep",
            Self::Logout => "Log Out of Dome",
            Self::Restart => "Restart",
            Self::Shutdown => "Shut Down",
        }
    }
}
