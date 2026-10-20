//! Persistent shell colours and physical mouse / hand arbitration preferences.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub light: bool,
    pub accent: String,
    pub background: [f32; 3],
    pub opacity: u8,
    pub hand_resume_ms: u64,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            light: false,
            accent: "#6babff".into(),
            background: [0.025, 0.045, 0.06],
            opacity: 248,
            hand_resume_ms: 800,
        }
    }
}
impl Appearance {
    pub fn validate(&self) -> Result<(), String> {
        if self.accent.len() != 7
            || !self.accent.starts_with('#')
            || !self.accent[1..].bytes().all(|b| b.is_ascii_hexdigit())
            || self.opacity < 160
            || !(300..=2000).contains(&self.hand_resume_ms)
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
