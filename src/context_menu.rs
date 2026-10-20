//! Common click anchoring and bounded scrolling for compositor and client menus.
use crate::Rect;
use serde::{Deserialize, Serialize};

pub const GAP: f32 = 8.;
pub const SCROLL_BAND: f32 = 24.;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub content_height: f32,
    pub height: f32,
    pub offset: f32,
}
impl Viewport {
    pub fn new(content_height: f32, limit: f32, offset: f32) -> Self {
        let height = content_height.min(limit.max(1.));
        let mut result = Self {
            content_height,
            height,
            offset: 0.,
        };
        result.offset = offset.clamp(0., result.max_offset());
        result
    }
    pub fn band(self) -> f32 {
        if self.content_height > self.height {
            SCROLL_BAND.min((self.height - 1.) / 2.).max(0.)
        } else {
            0.
        }
    }
    pub fn body(self) -> f32 {
        self.height - 2. * self.band()
    }
    pub fn max_offset(self) -> f32 {
        (self.content_height - self.body()).max(0.)
    }
    pub fn control(self, y: f32) -> Option<i32> {
        if self.band() == 0. {
            None
        } else if y >= 0. && y < self.band() {
            Some(-1)
        } else if y >= self.height - self.band() && y < self.height {
            Some(1)
        } else {
            None
        }
    }
    pub fn content_y(self, y: f32) -> f32 {
        self.offset + y - self.band()
    }
    pub fn scroll(&mut self, delta: f32) {
        self.offset = (self.offset + delta).clamp(0., self.max_offset());
    }
    pub fn reveal(&mut self, top: f32, height: f32) {
        if top < self.offset {
            self.offset = top;
        }
        if top + height > self.offset + self.body() {
            self.offset = top + height - self.body();
        }
        self.offset = self.offset.clamp(0., self.max_offset());
    }
}

/// Bottom centre sits above the click, with output-edge clamping on desktop.
pub fn rect(parent: Rect, click: [f32; 2], size: [f32; 2], work: Rect) -> Rect {
    let [w, h] = size;
    Rect {
        x: (parent.x + click[0] - w / 2.).clamp(work.x, (work.x + work.w - w).max(work.x)),
        y: (parent.y + click[1] - h - GAP).clamp(work.y, (work.y + work.h - h).max(work.y)),
        w,
        h,
    }
}

/// Keep context panels between the view-relative shell bars, including tilted parents.
pub fn fit_xr(panel: &mut crate::xr_input::Quad, head: crate::Pose, config: &crate::Config) {
    let upper =
        (config.vertical_fov_deg / 2. - config.upper_bar_height_deg)
            .to_radians()
            .tan();
    let lower =
        (-config.vertical_fov_deg / 2. + config.taskbar_height_deg)
            .to_radians()
            .tan();
    for _ in 0..8 {
        let mut bottom = f32::NEG_INFINITY;
        let mut top = f32::INFINITY;
        for uv in [[0., 0.], [1., 0.], [0., 1.], [1., 1.]] {
            let point = panel.point(uv) - glam::Vec3::from_array(head.position_m);
            let p = head.inverse_rotate(point.to_array());
            if p[2] >= -0.01 {
                return;
            }
            bottom = bottom.max(-p[2] * lower - p[1]);
            top = top.min(-p[2] * upper - p[1]);
        }
        if bottom <= top {
            let shift = 0f32.clamp(bottom, top);
            let delta = glam::Vec3::from_array(head.rotate([0., shift, 0.]));
            panel.pose.position_m =
                (glam::Vec3::from_array(panel.pose.position_m) + delta).to_array();
            return;
        }
        panel.size = panel.size.map(|v| v * 0.8);
    }
}

/// A cropped menu texture with compositor-owned top/bottom scroll controls.
pub fn texture(rgba: &[u8], width: u32, viewport: Viewport) -> Vec<u8> {
    let height = viewport.height.round() as usize;
    let width = width as usize;
    let mut out = vec![0; width * height * 4];
    for y in 0..height {
        if let Some(direction) = viewport.control(y as f32) {
            let enabled = if direction < 0 {
                viewport.offset > 0.
            } else {
                viewport.offset < viewport.max_offset()
            };
            for x in 0..width {
                let pixel = &mut out[(y * width + x) * 4..(y * width + x) * 4 + 4];
                pixel.copy_from_slice(&[31, 37, 45, 255]);
                let cy = if direction < 0 {
                    viewport.band() / 2.
                } else {
                    viewport.height - viewport.band() / 2.
                };
                let dy = (y as f32 - cy) * direction as f32;
                if (-4. ..=4.).contains(&dy) && (x as f32 - width as f32 / 2.).abs() <= (4. - dy) {
                    let color = if enabled { 235 } else { 100 };
                    pixel.copy_from_slice(&[color, color, color, 255]);
                }
            }
        } else {
            let source = viewport.content_y(y as f32).floor().max(0.) as usize;
            let start = source * width * 4;
            if let Some(row) = rgba.get(start..start + width * 4) {
                out[y * width * 4..(y + 1) * width * 4].copy_from_slice(row);
            }
        }
    }
    out
}
