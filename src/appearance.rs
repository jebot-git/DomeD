//! Persistent shell colours and font preferences.
use serde::{Deserialize, Serialize};
/// Alpha bytes nearest to the four percentage presets.
pub const CHROME_OPACITIES: [u8; 4] = [64, 128, 191, 255];
fn deserialize_opacity<'de, D: serde::Deserializer<'de>>(deserializer:D) -> Result<u8,D::Error> {
    let value=u8::deserialize(deserializer)?;
    if value < CHROME_OPACITIES[0] { return Ok(value); }
    Ok(*CHROME_OPACITIES.iter().min_by_key(|preset| preset.abs_diff(value)).unwrap())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub light: bool,
    pub accent: String,
    #[serde(rename = "background", deserialize_with = "crate::scene::discard_retired_setting", skip_serializing)]
    pub retired_background: (),
    #[serde(deserialize_with = "deserialize_opacity")]
    pub opacity: u8,
    pub font: String,
    pub font_size: u16,
    pub font_scale: u8,
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
        assert_eq!(appearance.font, "Unifont / Sans-Serif");
        assert_eq!(appearance.font_size, 16);
        assert!(appearance.validate().is_ok());
        let restored: Appearance = serde_json::from_slice(&serde_json::to_vec(&appearance).unwrap()).unwrap();
        assert_eq!(restored.font, appearance.font);
        assert_eq!(restored.font_size, appearance.font_size);
        assert_eq!(restored.desktop_background(), [208. / 255.; 3]);
        assert!(serde_json::to_value(&appearance).unwrap().get("background").is_none());
    }
    #[test]
    fn desktop_background_follows_the_colour_scheme() {
        let mut appearance = Appearance::default();
        assert_eq!(appearance.desktop_background(), [48. / 255.; 3]);
        appearance.light = true;
        assert_eq!(appearance.desktop_background(), [208. / 255.; 3]);
        appearance.light = false;
        assert_eq!(appearance.desktop_background(), Appearance::default().desktop_background());
    }
    #[test]
    fn retired_strikes_migrate_without_resetting_other_preferences() {
        for (name, size) in [("Galmuri7Bitmap / Regular", 10), ("GNU Unifont / Sans-Serif", 16)] {
            let mut appearance = Appearance::default();
            appearance.font = name.into();
            appearance.font_size = size;
            appearance.light = true;
            appearance.accent = "#123456".into();
            appearance.migrate_font();
            assert_eq!(appearance.font, Appearance::default().font);
            assert_eq!(appearance.font_size, 16);
            assert!(appearance.light);
            assert_eq!(appearance.accent, "#123456");
            assert!(appearance.validate().is_ok());
        }
    }
    #[test]
    fn invalid_font_settings_are_rejected() {
        let mut appearance = Appearance::default();
        appearance.font_size = 0;
        assert!(appearance.validate().is_err());
        appearance.font_size = 3;
        assert!(appearance.validate().is_err());
        appearance.font_size = 12;
        appearance.font_scale = 2;
        assert!(appearance.validate().is_ok());
        appearance.font_scale = 3;
        assert!(appearance.validate().is_err());
        appearance.font_scale = 1;
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
            retired_background: (),
            opacity: 255,
            font: "Unifont / Sans-Serif".into(),
            font_size: 16,
            font_scale: 1,
            retired_pointer_delay: (),
        }
    }
}
impl Appearance {
    pub fn caption_height(&self) -> i32 {
        30.max(self.font_size as i32 * self.font_scale as i32 + 8)
    }
    pub fn desktop_background(&self) -> [f32; 3] {
        [if self.light { 208. / 255. } else { 48. / 255. }; 3]
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.accent.len() != 7
            || !self.accent.starts_with('#')
            || !self.accent[1..].bytes().all(|b| b.is_ascii_hexdigit())
            || !CHROME_OPACITIES.contains(&self.opacity)
            || self.font.is_empty()
            || self.font.len() > 256
            || self.font.chars().any(char::is_control)
            || !(4..=64).contains(&self.font_size)
            || !matches!(self.font_scale, 1 | 2)
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
    fn migrate_font(&mut self) {
        // Earlier versions used the project name rather than the font's real family.
        if self.font == "GNU Unifont / Sans-Serif" {
            self.font = Self::default().font;
        }
        if self.font.starts_with("Galmuri") && self.font.contains("Bitmap / ") {
            self.font = Self::default().font;
            self.font_size = Self::default().font_size;
        }
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .map(|mut settings| { settings.migrate_font(); settings })
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

#[cfg(test)]
mod opacity_tests {
    use super::*;
    #[test]
    fn percentage_presets_round_trip_and_old_alpha_values_migrate() {
        for alpha in CHROME_OPACITIES {
            let appearance:Appearance=serde_json::from_value(serde_json::json!({"opacity":alpha})).unwrap();
            assert_eq!(appearance.opacity,alpha);assert!(appearance.validate().is_ok());
        }
        for (old,expected) in [(208,191),(248,255)] {
            let appearance:Appearance=serde_json::from_value(serde_json::json!({"opacity":old,"light":true})).unwrap();
            assert_eq!(appearance.opacity,expected);assert!(appearance.light);
        }
        let mut appearance=Appearance::default();appearance.opacity=0;assert!(appearance.validate().is_err());
    }
}
