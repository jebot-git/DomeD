//! Looking Glass overview and dock input. Client input resumes after Open/Escape.
use crate::Dome;
use dome::{looking_glass as lg, Action, Rect};
impl Dome {
    pub fn glass_json(&self, width: i32, height: i32) -> serde_json::Value {
        let scene = self.shell_scene();
        let state = &scene.looking_glass;
        if !state.enabled {
            return serde_json::json!({"enabled":false});
        }
        let cards: Vec<_> = lg::cards(&scene, self.input_realm, width, height)
            .into_iter()
            .map(|card| {
                let mut value = serde_json::to_value(&card).unwrap();
                value["corners"] = serde_json::json!(lg::corners(
                    card.rect,
                    if card.back { 0. } else { card.yaw_deg }
                ));
                value["preview"] = self.task_previews.json(card.id);
                value
            })
            .collect();
        let (_, page, pages) = lg::page(&scene, self.input_realm);
        serde_json::json!({"enabled":true,"overview":state.overview,"page":page,"pages":pages,"cards":cards})
    }
    pub fn glass_open(&mut self, id: u64) {
        self.action(Action::GlassWindow {
            id,
            yaw_deg: Some(0.),
            parked: Some(false),
            back: Some(false),
            note: None,
        });
        self.action(Action::Focus { id });
        self.action(Action::GlassOverview { open: false });
    }
    pub fn glass_click(&mut self, x: f64, y: f64, width: i32, height: i32) -> bool {
        let scene = self.shell_scene();
        if !scene.looking_glass.enabled
            || self.menu.is_some()
            || self.launcher
            || self.desktop_manager
            || self.appearance_open
            || self.xr_settings_open
            || self.about
            || self.audio_open
            || self.history_panel.is_some()
        {
            return false;
        }
        let (x, y) = (x as f32, y as f32);
        let (ids, page, pages) = lg::page(&scene, self.input_realm);
        if y >= height as f32 - 60. {
            if x >= width as f32 - 250. && x < width as f32 - 170. {
                self.action(Action::GlassOverview {
                    open: !scene.looking_glass.overview,
                });
                return true;
            }
            if x >= 110. && x < width as f32 - 250. {
                let capacity = ((width - 360) / 130).max(1) as usize;
                if let Some(id) = ids.iter().take(capacity).nth(((x - 110.) / 130.) as usize) {
                    self.glass_open(*id);
                }
                return true;
            }
            return false;
        }
        if !scene.looking_glass.overview || y < 36. {
            return false;
        }
        if y < 86. {
            if x >= width as f32 - 110. {
                self.action(Action::LookingGlass { enabled: false });
            } else if x >= width as f32 - 220. {
                self.action(Action::GlassOverview { open: false });
            } else if x >= width as f32 - 310. {
                self.action(Action::GlassPage {
                    page: (page + 1).min(pages - 1),
                });
            } else if x >= width as f32 - 400. {
                self.action(Action::GlassPage {
                    page: page.saturating_sub(1),
                });
            }
            return true;
        }
        for card in lg::cards(&scene, self.input_realm, width, height) {
            let state = scene
                .looking_glass
                .cards
                .get(&card.id)
                .cloned()
                .unwrap_or_default();
            let controls = Rect {
                x: card.rect.x,
                y: card.rect.y + card.rect.h + 4.,
                w: card.rect.w,
                h: 24.,
            };
            if lg::contains(controls, x, y) {
                match ((x - controls.x) / (controls.w / 5.)) as usize {
                    0 => self.glass_open(card.id),
                    1 => self.action(Action::GlassWindow {
                        id: card.id,
                        yaw_deg: None,
                        parked: Some(!state.parked),
                        back: None,
                        note: None,
                    }),
                    2 => self.action(Action::GlassWindow {
                        id: card.id,
                        yaw_deg: None,
                        parked: None,
                        back: Some(!state.back),
                        note: None,
                    }),
                    index => self.action(Action::GlassWindow {
                        id: card.id,
                        yaw_deg: Some(
                            (state.yaw_deg + if index == 3 { -10. } else { 10. }).clamp(-80., 80.),
                        ),
                        parked: Some(false),
                        back: None,
                        note: None,
                    }),
                }
                return true;
            }
            if lg::hit(&card, x, y) || (card.back && lg::contains(card.rect, x, y)) {
                self.action(Action::GlassWindow {
                    id: card.id,
                    yaw_deg: None,
                    parked: None,
                    back: None,
                    note: None,
                });
                return true;
            }
        }
        true
    }
    pub fn glass_key(&mut self, sym: u32, character: Option<char>, modified: bool) -> bool {
        let scene = self.shell_scene();
        if !scene.looking_glass.enabled || !scene.looking_glass.overview || self.menu.is_some() {
            return false;
        }
        if sym == 0xff1b {
            self.action(Action::GlassOverview { open: false });
            return true;
        }
        if modified {
            return true;
        }
        let selected = scene.looking_glass.selected.filter(|id| {
            scene.window(*id).is_some_and(|w| {
                w.workspace == scene.active_workspace && w.visible_in(self.input_realm)
            })
        });
        if let Some(id) = selected {
            let state = scene
                .looking_glass
                .cards
                .get(&id)
                .cloned()
                .unwrap_or_default();
            if state.back {
                let mut note = state.note;
                if sym == 0xff08 {
                    note.pop();
                } else if sym == 0xff0d {
                    if note.len() < 4096 {
                        note.push('\n');
                    }
                } else if let Some(c) = character.filter(|c| !c.is_control()) {
                    if note.len() + c.len_utf8() <= 4096 {
                        note.push(c);
                    }
                }
                self.action(Action::GlassWindow {
                    id,
                    yaw_deg: None,
                    parked: None,
                    back: None,
                    note: Some(note),
                });
                return true;
            }
            match sym {
                0xff0d => self.glass_open(id),
                0xff51 | 0xff53 => self.action(Action::GlassWindow {
                    id,
                    yaw_deg: Some(
                        (state.yaw_deg + if sym == 0xff51 { -10. } else { 10. }).clamp(-80., 80.),
                    ),
                    parked: Some(false),
                    back: None,
                    note: None,
                }),
                _ if character == Some('p') => self.action(Action::GlassWindow {
                    id,
                    yaw_deg: None,
                    parked: Some(!state.parked),
                    back: None,
                    note: None,
                }),
                _ if character == Some('f') => self.action(Action::GlassWindow {
                    id,
                    yaw_deg: None,
                    parked: None,
                    back: Some(true),
                    note: None,
                }),
                _ => (),
            }
        }
        true
    }
}
