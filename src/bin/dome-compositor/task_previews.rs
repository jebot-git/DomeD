//! Small, rate-limited taskbar captures, independent of full-resolution XR frames.
use base64::Engine;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            damage::OutputDamageTracker,
            element::{
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                Element, Id, Kind,
            },
            gles::{GlesRenderer, GlesTexture},
            utils::CommitCounter,
            Bind, ExportMem, Offscreen,
        },
    },
    utils::{Physical, Rectangle, Transform},
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

type Signature = Vec<(Id, CommitCounter, Rectangle<i32, Physical>)>;
struct Preview {
    size: (i32, i32),
    signature: Signature,
    pixels: Vec<u8>,
    encoded: String,
}
#[derive(Default)]
pub struct TaskPreviews {
    frames: HashMap<u64, Preview>,
    last_capture: Option<Instant>,
    next: usize,
    pub revision: u64,
}
impl TaskPreviews {
    pub fn json(&self, id: u64) -> serde_json::Value {
        self.frames.get(&id).map_or(serde_json::Value::Null, |frame| {
            serde_json::json!({"width":frame.size.0,"height":frame.size.1,"rgba":frame.encoded})
        })
    }
}
fn preview_size(width: i32, height: i32) -> Option<((i32, i32), f64)> {
    if width < 1 || height < 1 {
        return None;
    }
    let scale = (126. / width as f64).min(76. / height as f64).min(1.);
    Some((
        (
            (width as f64 * scale).round().max(1.) as i32,
            (height as f64 * scale).round().max(1.) as i32,
        ),
        scale,
    ))
}
impl crate::Dome {
    pub fn update_task_previews(&mut self, renderer: &mut GlesRenderer) {
        let ids: Vec<_> = self
            .task_popup
            .filter(|_| !self.task_context)
            .and_then(|index| self.task_list().get(index).cloned())
            .map(|task| task.windows.into_iter().take(4).collect())
            .unwrap_or_default();
        // Bound memory to the visible group; closing the popup stops all preview work.
        self.task_previews.frames.retain(|id, _| ids.contains(id));
        if ids.is_empty()
            || self
                .task_previews
                .last_capture
                .is_some_and(|last| last.elapsed() < Duration::from_millis(100))
        {
            return;
        }
        self.task_previews.last_capture = Some(Instant::now());
        let id = ids[self.task_previews.next % ids.len()];
        self.task_previews.next = self.task_previews.next.wrapping_add(1);
        let Some((surface, geo)) = self.popup_surface(id) else {
            return;
        };
        let Some((size, scale)) = preview_size(geo.size.w, geo.size.h) else {
            return;
        };
        let elements = render_elements_from_surface_tree::<
            GlesRenderer,
            WaylandSurfaceRenderElement<GlesRenderer>,
        >(
            renderer,
            &surface,
            (
                (-geo.loc.x as f64 * scale).round() as i32,
                (-geo.loc.y as f64 * scale).round() as i32,
            ),
            scale,
            1.,
            Kind::Unspecified,
        );
        let signature: Signature = elements
            .iter()
            .map(|element| {
                (
                    element.id().clone(),
                    element.current_commit(),
                    element.geometry(scale.into()),
                )
            })
            .collect();
        if self
            .task_previews
            .frames
            .get(&id)
            .is_some_and(|frame| frame.size == size && frame.signature == signature)
        {
            return;
        }
        let capture = (|| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            // Scale on the GPU before readback: at most 126 x 76 pixels, even for a 4K window.
            let mut texture: GlesTexture = renderer.create_buffer(Fourcc::Argb8888, size.into())?;
            let mut target = renderer.bind(&mut texture)?;
            let mut tracker = OutputDamageTracker::new(size, scale, Transform::Normal);
            tracker.render_output(renderer, &mut target, 0, &elements, [0.; 4])?;
            let mapping = renderer.copy_framebuffer(
                &target,
                Rectangle::from_size(size.into()),
                Fourcc::Abgr8888,
            )?;
            Ok(renderer.map_texture(&mapping)?.to_vec())
        })();
        if let Ok(pixels) = capture {
            let changed = self
                .task_previews
                .frames
                .get(&id)
                .is_none_or(|frame| frame.size != size || frame.pixels != pixels);
            let encoded = if changed {
                base64::engine::general_purpose::STANDARD.encode(&pixels)
            } else {
                self.task_previews.frames.get(&id).unwrap().encoded.clone()
            };
            self.task_previews.frames.insert(
                id,
                Preview {
                    size,
                    signature,
                    pixels,
                    encoded,
                },
            );
            if changed {
                self.task_previews.revision = self.task_previews.revision.wrapping_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_dimensions_are_bounded_even_for_large_and_thin_windows() {
        for (width, height) in [
            (3840, 2160),
            (16384, 8192),
            (1, 16384),
            (16384, 1),
            (40, 20),
        ] {
            let ((w, h), scale) = preview_size(width, height).unwrap();
            assert!((1..=126).contains(&w) && (1..=76).contains(&h));
            assert!(scale <= 1.);
            assert!((w as f64 - width as f64 * scale).abs() <= 1.);
            assert!((h as f64 - height as f64 * scale).abs() <= 1.);
        }
        assert!(preview_size(0, 100).is_none());
        assert!(preview_size(100, -1).is_none());
    }
}
