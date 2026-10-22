//! Local-time clock preferences and the calendar shared by desktop and XR chrome.
use chrono::{Datelike, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub twelve_hour: bool,
    pub seconds: bool,
    pub weekday: bool,
    pub date: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            twelve_hour: false,
            seconds: false,
            weekday: true,
            date: false,
        }
    }
}
impl Settings {
    fn path() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            })
            .join("dome/clock.json")
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&temporary, serde_json::to_vec_pretty(self).unwrap())
            .map_err(|e| e.to_string())?;
        std::fs::rename(temporary, path).map_err(|e| e.to_string())
    }
    pub fn toggle(&mut self, row: usize) {
        match row {
            0 => self.twelve_hour = !self.twelve_hour,
            1 => self.seconds = !self.seconds,
            2 => self.weekday = !self.weekday,
            3 => self.date = !self.date,
            _ => (),
        }
    }
    pub fn time_format(&self) -> String {
        let mut f = if self.twelve_hour { "%I:%M" } else { "%H:%M" }.to_string();
        if self.seconds {
            f.push_str(":%S");
        }
        if self.twelve_hour {
            f.push_str(" %p");
        }
        f
    }
}
#[derive(Clone, Debug)]
pub struct Calendar {
    pub open: bool,
    pub month: NaiveDate,
    pub selected: NaiveDate,
    pub settings_open: bool,
    pub settings_row: usize,
    pub settings: Settings,
    pub error: String,
}
impl Default for Calendar {
    fn default() -> Self {
        let today = Local::now().date_naive();
        Self {
            open: false,
            month: today.with_day(1).unwrap(),
            selected: today,
            settings_open: false,
            settings_row: 0,
            settings: Settings::load(),
            error: String::new(),
        }
    }
}
impl Calendar {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.today();
            self.settings_open = false;
            self.error.clear();
        }
    }
    pub fn today(&mut self) {
        self.selected = Local::now().date_naive();
        self.month = self.selected.with_day(1).unwrap();
    }
    pub fn move_month(&mut self, step: i32) {
        let total = self.month.year() * 12 + self.month.month0() as i32 + step;
        if let Some(next) =
            NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
        {
            self.month = next;
            self.selected = next;
        }
    }
    pub fn move_day(&mut self, step: i64) {
        if let Some(date) = self.selected.checked_add_signed(Duration::days(step)) {
            self.selected = date;
            self.month = date.with_day(1).unwrap();
        }
    }
    pub fn days(&self) -> Vec<NaiveDate> {
        let first = self.month - Duration::days(self.month.weekday().num_days_from_monday().into());
        (0..42).map(|n| first + Duration::days(n)).collect()
    }
    pub fn rect(width: i32, height: i32, settings: bool) -> [f64; 4] {
        let w = 392.min((width - 16).max(1)) as f64;
        let h = (if settings { 514 } else { 370 }).min((height - 100).max(1)) as f64;
        [(width as f64 - w - 8.).max(0.), 42., w, h]
    }
    pub fn label(&self) -> String {
        let now = Local::now();
        let time = now.format(&self.settings.time_format()).to_string();
        let date = if self.settings.date {
            now.format(if self.settings.weekday {
                "%a %Y-%m-%d"
            } else {
                "%Y-%m-%d"
            })
            .to_string()
        } else if self.settings.weekday {
            now.format("%a").to_string()
        } else {
            String::new()
        };
        if date.is_empty() {
            time
        } else {
            format!("{date} {time}")
        }
    }
    pub fn bar_width(&self, font_size: u16) -> i32 {
        (self.label().chars().count() as i32 * ((font_size as i32 + 1) / 2) + 24).clamp(176, 480)
    }
    pub fn json(&self, width: i32, height: i32, available: bool) -> serde_json::Value {
        let now = Local::now();
        let primary = self.label();
        let popup = if self.open {
            let [x, y, w, h] = Self::rect(width, height, self.settings_open);
            let today = now.date_naive();
            serde_json::json!({"x":x,"y":y,"w":w,"h":h,"month":self.month.format("%B %Y").to_string(),"selected":self.selected.to_string(),"available":available,"settings_open":self.settings_open,"settings_row":self.settings_row,"options":[["12-hour clock",self.settings.twelve_hour],["Show seconds",self.settings.seconds],["Show weekday",self.settings.weekday],["Show date",self.settings.date]],"error":self.error,"days":self.days().iter().map(|d|serde_json::json!({"date":d.to_string(),"day":d.day(),"today":*d==today,"selected":*d==self.selected,"in_month":d.month()==self.month.month()})).collect::<Vec<_>>()})
        } else {
            serde_json::Value::Null
        };
        serde_json::json!({"text":primary,"secondary":"","popup":popup})
    }
}
/// The date argument is passed directly to the process, never to a shell.
pub fn mail_program() -> Option<PathBuf> {
    if let Some(program) = std::env::var_os("DOME_PENGUIN_MAIL") {
        if let Some(path) = crate::desktop_entries::find_executable(&program.to_string_lossy()) {
            return Some(path);
        }
    }
    for program in ["penguin-mail", "penguin-mail-fltk"] {
        if let Some(path) = crate::desktop_entries::find_executable(program) {
            return Some(path);
        }
    }
    for path in [
        "/opt/fltk-vr/bin/penguin-mail",
        "/usr/local/bin/penguin-mail",
    ] {
        if let Some(path) = crate::desktop_entries::find_executable(path) {
            return Some(path);
        }
    }
    None
}
pub fn launch_date(path: &std::path::Path, date: NaiveDate, socket: &str) -> Result<(), String> {
    let mut child = std::process::Command::new(path)
        .arg("--calendar")
        .arg(date.to_string())
        .env("WAYLAND_DISPLAY", socket)
        .env_remove("WAYLAND_SOCKET")
        .envs(
            crate::desktop_entries::launch_environment(path)
                .iter()
                .copied(),
        )
        .spawn()
        .map_err(|e| format!("Cannot open Penguin Mail: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leap_day_and_year_boundaries_keep_every_date() {
        let mut c = Calendar::default();
        c.month = NaiveDate::from_ymd_opt(2024, 2, 1).unwrap();
        let days = c.days();
        assert_eq!(days.len(), 42);
        assert!(days.contains(&NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()));
        assert_eq!(days[0].weekday(), chrono::Weekday::Mon);
        c.month = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        c.move_month(1);
        assert_eq!(c.month.to_string(), "2027-01-01");
        c.move_month(-1);
        assert_eq!(c.month.to_string(), "2026-12-01");
        c.selected = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        c.move_day(1);
        assert_eq!(c.selected.to_string(), "2027-01-01");
    }
    #[test]
    fn options_round_trip_and_time_formats_are_bounded() {
        let mut s = Settings::default();
        assert_eq!(s.time_format(), "%H:%M");
        for row in 0..4 {
            s.toggle(row);
        }
        assert_eq!(s.time_format(), "%I:%M:%S %p");
        let copy: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(copy, s);
        assert!(copy.date);
        assert!(!copy.weekday);
        assert_eq!(
            serde_json::from_str::<Settings>("{}").unwrap(),
            Settings::default()
        );
    }
    #[test]
    fn a_long_clock_label_reserves_room_for_the_native_font() {
        let mut clock = Calendar::default();
        clock.settings = Settings {
            twelve_hour: true,
            seconds: true,
            weekday: true,
            date: true,
        };
        assert!(clock.bar_width(24) >= clock.label().chars().count() as i32 * 12 + 24);
        assert!(clock.bar_width(24) > clock.bar_width(16));
    }
    #[test]
    fn rendered_geometry_and_hit_cells_share_the_same_six_weeks() {
        let mut c = Calendar::default();
        c.open = true;
        c.settings_open = true;
        let j = c.json(1024, 768, true);
        assert_eq!(j["popup"]["days"].as_array().unwrap().len(), 42);
        assert_eq!(j["popup"]["options"].as_array().unwrap().len(), 4);
        let [x, y, w, h] = Calendar::rect(1024, 768, true);
        assert!(x >= 0. && y >= 36. && x + w <= 1024. && y + h <= 708.);
    }
}
