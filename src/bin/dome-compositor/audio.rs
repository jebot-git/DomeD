//! One audio pane and input policy for desktop and XR.
use crate::Dome;
use dome::audio::{Device, DeviceList, DeviceRow, Request, Snapshot, Target};
use dome::audio_mixer::{Action as MixerAction, Request as MixerRequest};
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
        self.clock.open = false;
        self.audio_drag = None;
        self.audio_drag_value = None;
        self.audio_open = !self.audio_open;
        self.audio_selected = 0;
        self.audio.request(Request::Open(self.audio_open));
        if self.audio_open {
            self.audio
                .request(Request::MixerPage(self.audio_mixer_page));
        }
        self.history_panel = None;
        self.menu = None;
        self.launcher = false;
        self.desktop_manager = false;
        self.task_popup = None;
        self.default_apps_open = false;
        self.appearance_open = false;
        self.xr_settings_open = false;
        self.binding_capture = None;
        self.tray.close();
    }
    fn audio_devices(snapshot: &Snapshot) -> Vec<&Device> {
        snapshot.outputs.iter().chain(&snapshot.inputs).collect()
    }
    pub fn audio_rect(&self, width: i32, height: i32) -> [f64; 4] {
        if self.audio_mixer_page {
            let count = self.audio.state().mixer.len();
            let w = 520.min((width - 24).max(1)) as f64;
            let h = (250 + count.clamp(1, 10) as i32 * 32).min((height - 108).max(1)) as f64;
            return [(width as f64 - w - 12.).max(0.), 42., w, h];
        }
        let count = DeviceList::new(&self.audio.state(), None, 0.).rows.len();
        let w = 440.min((width - 24).max(1)) as f64;
        let h = (250 + count.min(10) as i32 * 32).min((height - 108).max(1)) as f64;
        [(width as f64 - w - 12.).max(0.), 42., w, h]
    }
    pub fn audio_json(&self, width: i32, height: i32) -> serde_json::Value {
        if !self.audio_open {
            return serde_json::Value::Null;
        }
        let snapshot = self.audio.state();
        if self.audio_mixer_page {
            return self.mixer_json(&snapshot, width, height);
        }
        let devices = Self::audio_devices(&snapshot);
        let [x, y, w, h] = self.audio_rect(width, height);
        let selected = self.audio_selected.min(devices.len() + 1);
        let list = DeviceList::new(&snapshot, selected.checked_sub(2), h);
        let visible = list.visible;
        let first = list.first;
        let rows: Vec<_> = list.rows.iter().map(|row| match row {
            DeviceRow::Heading(target) => serde_json::json!({"kind":"heading","target":target,"label":if *target==Target::Output {"Output devices"}else{"Microphones"}}),
            DeviceRow::Device(index) => {let d=devices[*index];serde_json::json!({"kind":"device","index":index+2,"id":d.id,"serial":d.serial,"target":d.target,"label":d.label,"active":d.selected})},
        }).collect();
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
        if matches!(key, 0xff09 | 0xff55 | 0xff56) {
            self.set_audio_page(!self.audio_mixer_page);
            return true;
        }
        if self.audio_mixer_page {
            self.mixer_key(key);
            return true;
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
        if y >= top + 4. && y < top + 34. && x >= left + 96. && x < left + 330. {
            self.set_audio_page(x >= left + 216.);
            return true;
        }
        if self.audio_mixer_page {
            self.mixer_click(x - left, y - top, w, h);
            return true;
        }
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
        let list = DeviceList::new(&snapshot, self.audio_selected.checked_sub(2), h);
        if let Some(index) = list.device_at(y - top - 214.) {
            self.audio_selected = index + 2;
            self.audio_activate();
        }
        true
    }
    pub fn audio_motion(&mut self, x: f64, width: i32, height: i32) {
        if self.audio_mixer_page && self.audio_open && self.audio_drag.is_some() {
            let [left, _, w, _] = self.audio_rect(width, height);
            let value = (((x - left - 60.) / (w - 216.)).clamp(0., 1.) * 100.).round() as u8;
            if self.audio_drag_value != Some(value) {
                self.audio_drag_value = Some(value);
                self.mixer_request(MixerAction::Volume {
                    channel: self.audio_mixer_channel.checked_sub(1),
                    value: value as f32 / 100.,
                });
            }
            return;
        }
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
        if self.audio_mixer_page && self.audio_open && !on_icon {
            let [_, top, _, _] = self.audio_rect(width, height);
            if y >= top + 214. {
                self.mixer_select(if delta > 0. { 1 } else { -1 });
            } else {
                self.mixer_request(MixerAction::Step {
                    channel: self.audio_mixer_channel.checked_sub(1),
                    delta: if delta < 0. { 5 } else { -5 },
                });
            }
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
    fn set_audio_page(&mut self, mixer: bool) {
        self.audio_mixer_page = mixer;
        self.audio_drag = None;
        self.audio_drag_value = None;
        self.audio.request(Request::MixerPage(mixer));
    }
    fn mixer_json(&self, snapshot: &Snapshot, width: i32, height: i32) -> serde_json::Value {
        let [x, y, w, h] = self.audio_rect(width, height);
        let selected = self
            .audio_mixer_selected
            .min(snapshot.mixer.len().saturating_sub(1));
        let visible = ((h - 250.) / 32.).floor().max(1.) as usize;
        let first = selected
            .saturating_sub(visible - 1)
            .min(snapshot.mixer.len().saturating_sub(visible));
        let item = snapshot.mixer.get(selected);
        let channel = item.and_then(|i| {
            self.audio_mixer_channel
                .checked_sub(1)
                .filter(|n| *n < i.channels.len())
        });
        let channel_label = channel
            .and_then(|n| item?.channels.get(n).map(|c| c.name.as_str()))
            .unwrap_or("All channels");
        let level = item.and_then(|i| {
            i.volume(channel)
                .map(|v| serde_json::json!({"volume":v,"muted":i.muted}))
        });
        let choice = item
            .and_then(|i| i.choices.iter().find(|c| c.value == i.current))
            .map(|c| c.label.as_str())
            .unwrap_or("No selection");
        let rows:Vec<_>=snapshot.mixer.iter().enumerate().map(|(index,i)|serde_json::json!({"index":index,"label":format!("{} · {}",i.kind.label(),i.label)})).collect();
        serde_json::json!({"mixer_page":true,"x":x,"y":y,"w":w,"h":h,"selected":selected,"first":first,"visible":visible,"rows":rows,"item":item,"level":level,"channel":channel_label,"choice":choice,"error":snapshot.mixer_error,"hint":"Arrows: volume · Enter: mute · C: channel · R: route/profile · Tab: page"})
    }
    fn mixer_request(&self, mut action: MixerAction) {
        let snapshot = self.audio.state();
        if let Some(item) = snapshot.mixer.get(
            self.audio_mixer_selected
                .min(snapshot.mixer.len().saturating_sub(1)),
        ) {
            if !matches!(action, MixerAction::Select(_)) && item.channels.is_empty() {
                return;
            }
            match &mut action {
                MixerAction::Volume { channel, .. } | MixerAction::Step { channel, .. }
                    if channel.is_some_and(|n| n >= item.channels.len()) =>
                {
                    *channel = None
                }
                _ => (),
            }
            self.audio.request(Request::Mixer(MixerRequest {
                kind: item.kind,
                id: item.id,
                serial: item.serial,
                action,
            }));
        }
    }
    fn mixer_select(&mut self, delta: i32) {
        let last = self.audio.state().mixer.len().saturating_sub(1);
        self.audio_mixer_selected = self
            .audio_mixer_selected
            .saturating_add_signed(delta as isize)
            .min(last);
        self.audio_mixer_channel = 0;
    }
    fn mixer_channel(&mut self, delta: i32) {
        let snapshot = self.audio.state();
        if let Some(item) = snapshot.mixer.get(
            self.audio_mixer_selected
                .min(snapshot.mixer.len().saturating_sub(1)),
        ) {
            self.audio_mixer_channel = (self.audio_mixer_channel as i32 + delta)
                .rem_euclid(item.channels.len() as i32 + 1)
                as usize;
        }
    }
    fn mixer_choice(&self, delta: i32) {
        let snapshot = self.audio.state();
        if let Some(item) = snapshot.mixer.get(
            self.audio_mixer_selected
                .min(snapshot.mixer.len().saturating_sub(1)),
        ) {
            if item.choices.is_empty() {
                return;
            }
            let current = item
                .choices
                .iter()
                .position(|c| c.value == item.current)
                .map(|n| n as i32)
                .unwrap_or(if delta > 0 { -1 } else { 0 });
            let index = (current + delta).rem_euclid(item.choices.len() as i32) as usize;
            self.mixer_request(MixerAction::Select(item.choices[index].clone()));
        }
    }
    fn mixer_key(&mut self, key: u32) {
        match key {
            0xff1b => self.close_audio(),
            0xff52 => self.mixer_select(-1),
            0xff54 => self.mixer_select(1),
            0xff50 => {
                self.audio_mixer_selected = 0;
                self.audio_mixer_channel = 0;
            }
            0xff57 => {
                self.audio_mixer_selected = self.audio.state().mixer.len().saturating_sub(1);
                self.audio_mixer_channel = 0;
            }
            0xff51 | 0xff53 => self.mixer_request(MixerAction::Step {
                channel: self.audio_mixer_channel.checked_sub(1),
                delta: if key == 0xff53 { 5 } else { -5 },
            }),
            0xff0d | 0x20 => self.mixer_request(MixerAction::Mute),
            0x63 | 0x43 => self.mixer_channel(1),
            0x72 | 0x52 => self.mixer_choice(1),
            _ => (),
        }
    }
    fn mixer_click(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let snapshot = self.audio.state();
        self.audio_mixer_selected = self
            .audio_mixer_selected
            .min(snapshot.mixer.len().saturating_sub(1));
        if (64. ..94.).contains(&y) {
            if x >= w - 96. {
                self.mixer_request(MixerAction::Mute);
            } else if x >= w - 140. {
                self.mixer_request(MixerAction::Step {
                    channel: self.audio_mixer_channel.checked_sub(1),
                    delta: 5,
                });
            } else if x < 52. {
                self.mixer_request(MixerAction::Step {
                    channel: self.audio_mixer_channel.checked_sub(1),
                    delta: -5,
                });
            } else if x >= 60. && x < w - 156. {
                self.audio_drag = Some(Target::Output);
                let value = (((x - 60.) / (w - 216.)).clamp(0., 1.) * 100.).round() as u8;
                self.audio_drag_value = Some(value);
                self.mixer_request(MixerAction::Volume {
                    channel: self.audio_mixer_channel.checked_sub(1),
                    value: value as f32 / 100.,
                });
            }
        } else if (108. ..138.).contains(&y) {
            self.mixer_channel(if x < w / 2. { -1 } else { 1 });
        } else if (154. ..184.).contains(&y) {
            self.mixer_choice(if x < w / 2. { -1 } else { 1 });
        } else if y >= 214. {
            let visible = ((h - 250.) / 32.).floor().max(1.) as usize;
            let first = self
                .audio_mixer_selected
                .saturating_sub(visible - 1)
                .min(snapshot.mixer.len().saturating_sub(visible));
            let row = first + ((y - 214.) / 32.) as usize;
            if y < 214. + visible as f64 * 32. && row < snapshot.mixer.len() {
                self.audio_mixer_selected = row;
                self.audio_mixer_channel = 0;
            }
        }
    }
}
