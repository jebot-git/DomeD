//! One audio pane and input policy for desktop and XR.
use crate::Dome;
use dome::audio::{Device, Request, Snapshot, Target};
impl Dome {
    pub fn close_audio(&mut self) {
        self.audio_drag = None;
        self.audio_drag_value = None;
        if self.audio_open {
            self.audio_open = false;
            self.audio.request(Request::Open(false));
        }
    }
    pub fn toggle_audio(&mut self) {
        self.audio_drag = None;
        self.audio_drag_value = None;
        self.audio_open = !self.audio_open;
        self.audio_selected = 0;
        self.audio.request(Request::Open(self.audio_open));
        self.history_panel = None;
        self.menu = None;
        self.launcher = false;
        self.desktop_manager = false;
        self.task_popup = None;
        self.appearance_open = false;
        self.xr_settings_open = false;
        self.binding_capture = None;
        self.tray.close();
    }
    fn audio_devices(snapshot: &Snapshot) -> Vec<&Device> {
        snapshot.outputs.iter().chain(&snapshot.inputs).collect()
    }
    pub fn audio_rect(&self, width: i32, height: i32) -> [f64; 4] {
        let count = Self::audio_devices(&self.audio.state()).len();
        let w = 440.min((width - 24).max(1)) as f64;
        let h = (250 + count.min(10) as i32 * 32).min((height - 108).max(1)) as f64;
        [(width as f64 - w - 12.).max(0.), 42., w, h]
    }
    pub fn audio_json(&self, width: i32, height: i32) -> serde_json::Value {
        if !self.audio_open {
            return serde_json::Value::Null;
        }
        let snapshot = self.audio.state();
        let devices = Self::audio_devices(&snapshot);
        let [x, y, w, h] = self.audio_rect(width, height);
        let visible = ((h - 250.) / 32.).floor().max(1.) as usize;
        let selected = self.audio_selected.min(devices.len() + 1);
        let first = selected
            .saturating_sub(2)
            .saturating_sub(visible - 1)
            .min(devices.len().saturating_sub(visible));
        let rows:Vec<_>=devices.into_iter().map(|d|serde_json::json!({"id":d.id,"serial":d.serial,"target":d.target,"label":d.label,"active":d.selected})).collect();
        serde_json::json!({"x":x,"y":y,"w":w,"h":h,"output":snapshot.output,"input":snapshot.input,"output_name":snapshot.outputs.iter().find(|d|d.selected).map(|d|d.label.as_str()).unwrap_or("No output device"),"input_name":snapshot.inputs.iter().find(|d|d.selected).map(|d|d.label.as_str()).unwrap_or("No microphone"),"error":snapshot.error,"rows":rows,"visible":visible,"first":first,"selected":selected})
    }
    fn audio_activate(&self) {
        let snapshot = self.audio.state();
        match self.audio_selected {
            0 if snapshot.output.is_some() => self.audio.request(Request::Mute(Target::Output)),
            1 if snapshot.input.is_some() => self.audio.request(Request::Mute(Target::Input)),
            n if n >= 2 => {
                if let Some(d) = Self::audio_devices(&snapshot).get(n - 2) {
                    self.audio.request(Request::Default {
                        target: d.target,
                        id: d.id,
                        serial: d.serial,
                    });
                }
            }
            _ => (),
        }
    }
    pub fn audio_key(&mut self, key: u32) -> bool {
        if !self.audio_open {
            return false;
        }
        let snapshot = self.audio.state();
        let last = Self::audio_devices(&snapshot).len() + 1;
        self.audio_selected = self.audio_selected.min(last);
        match key {
            0xff1b => self.close_audio(),
            0xff52 => self.audio_selected = self.audio_selected.saturating_sub(1),
            0xff54 => self.audio_selected = (self.audio_selected + 1).min(last),
            0xff50 => self.audio_selected = 0,
            0xff57 => self.audio_selected = last,
            0xff51 | 0xff53 => {
                let target = if self.audio_selected == 1 {
                    Target::Input
                } else {
                    Target::Output
                };
                let level = if target == Target::Input {
                    snapshot.input
                } else {
                    snapshot.output
                };
                if level.is_some() && self.audio_selected < 2 {
                    self.audio
                        .request(Request::Step(target, if key == 0xff53 { 5 } else { -5 }));
                }
            }
            0xff0d | 0x20 => self.audio_activate(),
            _ => (),
        }
        true
    }
    pub fn audio_click(&mut self, x: f64, y: f64, width: i32, height: i32) -> bool {
        if !self.audio_open {
            return false;
        }
        let [left, top, w, h] = self.audio_rect(width, height);
        if x < left
            || x >= left + w
            || y < top
            || y >= top + h
            || (x >= left + w - 36. && y < top + 36.)
        {
            self.close_audio();
            return true;
        }
        self.audio_drag = None;
        self.audio_drag_value = None;
        let snapshot = self.audio.state();
        for (target, offset, index, level) in [
            (Target::Output, 64., 0, snapshot.output),
            (Target::Input, 136., 1, snapshot.input),
        ] {
            if y >= top + offset && y < top + offset + 30. {
                self.audio_selected = index;
                if level.is_some() {
                    if x >= left + w - 96. {
                        self.audio.request(Request::Mute(target));
                    } else if x >= left + w - 140. {
                        self.audio.request(Request::Step(target, 5));
                    } else if x < left + 52. {
                        self.audio.request(Request::Step(target, -5));
                    } else if x >= left + 60. && x < left + w - 156. {
                        self.audio_drag = Some(target);
                        self.audio_motion(x, width, height);
                    }
                }
                return true;
            }
        }
        let visible = ((h - 250.) / 32.).floor().max(1.) as usize;
        let devices = Self::audio_devices(&snapshot);
        let first = self
            .audio_selected
            .saturating_sub(2)
            .saturating_sub(visible - 1)
            .min(devices.len().saturating_sub(visible));
        if y >= top + 214. && y < top + 214. + visible as f64 * 32. {
            let index = first + ((y - top - 214.) / 32.) as usize;
            if index < devices.len() {
                self.audio_selected = index + 2;
                self.audio_activate();
            }
        }
        true
    }
    pub fn audio_motion(&mut self, x: f64, width: i32, height: i32) {
        let Some(target) = self.audio_drag.filter(|_| self.audio_open) else {
            return;
        };
        let [left, _, w, _] = self.audio_rect(width, height);
        let value = (((x - left - 60.) / (w - 216.)).clamp(0., 1.) * 100.).round() as u8;
        if self.audio_drag_value != Some(value) {
            self.audio_drag_value = Some(value);
            self.audio
                .request(Request::Volume(target, value as f32 / 100.));
        }
    }
    /// Wheel over the speaker controls output; over the microphone row it controls input.
    pub fn audio_scroll(&mut self, x: f64, y: f64, delta: f64, width: i32, height: i32) -> bool {
        let tray = self.visible_tray(width);
        let left = width as f64 - 220. - tray.items.len() as f64 * 28.;
        let on_icon = tray
            .items
            .first()
            .is_some_and(|item| item.key == "dome.audio")
            && (0. ..36.).contains(&y)
            && x >= left
            && x < left + 28.;
        if !self.audio_open && !on_icon {
            return false;
        }
        if !delta.is_finite() || delta == 0. {
            return true;
        }
        let snapshot = self.audio.state();
        let [_, top, _, _] = self.audio_rect(width, height);
        if self.audio_open && y >= top + 214. {
            let last = Self::audio_devices(&snapshot).len() + 1;
            self.audio_selected = if delta > 0. {
                (self.audio_selected + 1).min(last)
            } else {
                self.audio_selected.saturating_sub(1)
            };
        } else {
            let target = if self.audio_open && y >= top + 108. && y < top + 176. {
                Target::Input
            } else {
                Target::Output
            };
            let level = if target == Target::Input {
                snapshot.input
            } else {
                snapshot.output
            };
            if level.is_some() {
                self.audio
                    .request(Request::Step(target, if delta < 0. { 5 } else { -5 }));
            }
        }
        true
    }
}
