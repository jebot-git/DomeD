//! Compositor-owned screenshot selection; compression and storage run off-thread.
use dome::screenshot::{Capture, Pixels};
use smithay::{
    backend::renderer::{
        damage::OutputDamageTracker,
        element::RenderElement,
        gles::{GlesRenderer, GlesTexture, GlesTarget},
        Bind, ExportMem, Offscreen, TextureMapping,
    },
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
};
use std::{sync::mpsc, time::Instant};
/// Render in logical desktop orientation, independently of the presentation target.
/// Native DRM uses Normal while nested presentation uses Flipped180. Reading
/// either presentation framebuffer and only checking mapping.flipped() cannot
/// account for that difference. Keep screenshot pixels top-down in both cases.
fn capture_pixels<E: RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    elements: &[E],
    size: Size<i32, Physical>,
    background: [f32; 4],
) -> Result<Pixels, Box<dyn std::error::Error>> {
    let mut texture: GlesTexture = renderer.create_buffer(
        smithay::backend::allocator::Fourcc::Abgr8888, (size.w, size.h).into())?;
    let mut target: GlesTarget<'_> = renderer.bind(&mut texture)?;
    let mut damage = OutputDamageTracker::new(size, 1., Transform::Flipped180);
    damage.render_output(renderer, &mut target, 0, elements, background)?.sync.wait()?;
    let mapping = renderer.copy_framebuffer(
        &target, Rectangle::from_size((size.w, size.h).into()),
        smithay::backend::allocator::Fourcc::Abgr8888)?;
    let flipped = mapping.flipped();
    let bytes = renderer.map_texture(&mapping)?;
    let stride = size.w as usize * 4;
    let rgba = if flipped {
        bytes.chunks_exact(stride).rev().flatten().copied().collect()
    } else { bytes.to_vec() };
    Ok(Pixels { width: size.w as u32, height: size.h as u32, rgba })
}
pub struct Selection {
    pub bounds: Rectangle<i32, Logical>,
    pub pixels: Pixels,
    outputs: Vec<(Rectangle<i32, Logical>, bool)>,
    pub start: Option<[i32; 2]>,
    pub end: [i32; 2],
    pub serial: u64,
}
impl Selection {
    fn new(mut outputs: Vec<Rectangle<i32, Logical>>, serial: u64) -> Result<Self, String> {
        let mut unique = Vec::new();
        outputs.retain(|g| { if unique.contains(g) { false } else { unique.push(*g); true } });
        let bounds = outputs.iter().copied().reduce(|a, b| a.merge(b))
            .ok_or("No active desktop outputs")?;
        if outputs.iter().any(|g| g.size.w <= 0 || g.size.h <= 0) {
            return Err("Invalid output dimensions".into());
        }
        let pixels = Pixels::opaque_black(bounds.size.w as u32, bounds.size.h as u32)?;
        Ok(Self { bounds, pixels, outputs: outputs.into_iter().map(|g| (g, false)).collect(),
                  start: None, end: [0, 0], serial })
    }
    pub fn ready(&self) -> bool { self.outputs.iter().all(|(_, captured)| *captured) }
    pub fn has_output(&self, geometry: Rectangle<i32, Logical>) -> bool {
        self.outputs.iter().any(|(g, _)| *g == geometry)
    }
    fn record(&mut self, output: Rectangle<i32, Logical>, pixels: Pixels) -> Result<(), String> {
        let (geometry, captured) = self.outputs.iter_mut().find(|(g, _)| *g == output)
            .ok_or("Output layout changed during capture")?;
        if pixels.width != geometry.size.w as u32 || pixels.height != geometry.size.h as u32 {
            return Err("Output size changed during capture".into());
        }
        self.pixels.blit(&pixels, (output.loc.x - self.bounds.loc.x) as u32,
                         (output.loc.y - self.bounds.loc.y) as u32)?;
        *captured = true;
        Ok(())
    }
}
pub struct Screenshots {
    pub selection: Option<Selection>,
    serial: u64,
    busy: bool,
    xr_started: Option<Instant>,
    send: mpsc::Sender<Result<(Capture, Result<std::path::PathBuf, String>), String>>,
    receive: mpsc::Receiver<Result<(Capture, Result<std::path::PathBuf, String>), String>>,
}
impl Default for Screenshots {
    fn default() -> Self {
        let (send, receive) = mpsc::channel();
        Self {
            selection: None,
            serial: 0,
            busy: false,
            xr_started: None,
            send,
            receive,
        }
    }
}
impl crate::Dome {
    pub fn start_screenshot(&mut self) {
        if self.screenshot.busy || self.screenshot.selection.is_some() {
            return;
        }
        self.menu = None;
        self.tray.close();
        self.clock.open = false;
        self.close_audio();
        self.history_panel = None;
        self.launcher = false;
        self.task_popup = None;
        self.task_context = false;
        self.about = false;
        self.desktop_manager = false;
        if self.input_realm == dome::Realm::Xr {
            let mut bridge = self.xr_bridge.lock().unwrap();
            if !bridge.visible() {
                drop(bridge);
                self.screenshot_notice("Screenshot failed", "No active XR view");
                return;
            }
            self.screenshot.serial += 1;
            bridge.screenshot_request = Some(self.screenshot.serial);
            bridge.screenshot_result = None;
            self.screenshot.xr_started = Some(Instant::now());
            self.screenshot.busy = true;
        } else {
            self.screenshot.serial += 1;
            let outputs = self.space.outputs().filter_map(|o| self.space.output_geometry(o)).collect();
            match Selection::new(outputs, self.screenshot.serial) {
                Ok(selection) => self.screenshot.selection = Some(selection),
                Err(message) => self.screenshot_notice("Screenshot failed", &message),
            }
        }
    }
    fn screenshot_notice(&self, title: &str, body: &str) {
        self.notifications.screenshot(title, body);
    }
    pub fn screenshot_key(&mut self, key: u32) -> bool {
        if self.screenshot.selection.is_none() {
            return false;
        }
        if key == 0xff1b {
            self.screenshot.selection = None;
            self.ignored_buttons
                .extend(self.physical_buttons.iter().copied());
        }
        true
    }
    pub fn screenshot_motion(&mut self, pos: Point<f64, Logical>) -> bool {
        let Some(selection) = self.screenshot.selection.as_mut() else {
            return false;
        };
        selection.end = [
            (pos.x - selection.bounds.loc.x as f64).round() as i32,
            (pos.y - selection.bounds.loc.y as f64).round() as i32,
        ];
        true
    }
    pub fn screenshot_button(&mut self, button: u32, pressed: bool) -> bool {
        if self.screenshot.selection.is_none() {
            return false;
        }
        if pressed {
            self.physical_buttons.insert(button);
        } else {
            self.physical_buttons.remove(&button);
        }
        if button == 0x111 && pressed {
            self.screenshot.selection = None;
            self.ignored_buttons.insert(button);
            return true;
        }
        if button != 0x110 {
            return true;
        }
        let position = self.seat.get_pointer().unwrap().current_location();
        self.screenshot_motion(position);
        let selection = self.screenshot.selection.as_mut().unwrap();
        if !selection.ready() {
            return true;
        }
        if pressed {
            selection.start = Some(selection.end);
        } else if let Some(start) = selection.start {
            let selection = self.screenshot.selection.take().unwrap();
            if let Some(pixels) = selection.pixels.crop(start, selection.end) {
                self.screenshot.busy = true;
                let send = self.screenshot.send.clone();
                std::thread::spawn(move || {
                    let result = pixels.png().map(|png| {
                        let path = dome::screenshot::save_png(&png, false);
                        (Capture::Png { bytes:png, stereo:false }, path)
                    });
                    let _ = send.send(result);
                });
            }
        }
        true
    }
    pub fn capture_screenshot<E: RenderElement<GlesRenderer>>(
        &mut self,
        renderer: &mut GlesRenderer,
        elements: &[E],
        output: Rectangle<i32, Logical>,
        background: [f32; 4],
    ) {
        let Some(selection) = self
            .screenshot
            .selection
            .as_mut()
            .filter(|s| s.outputs.iter().any(|(g, captured)| *g == output && !captured))
        else {
            return;
        };
        let size = output.size;
        let result = capture_pixels(renderer, elements, (size.w, size.h).into(), background)
            .map_err(|e| e.to_string()).and_then(|pixels| selection.record(output, pixels));
        match result {
            Ok(()) => {},
            Err(e) => {
                let message = e.to_string();
                self.screenshot.selection = None;
                self.screenshot_notice("Screenshot failed", &message);
            }
        }
    }
    pub fn poll_screenshots(&mut self) {
        if self.screenshot.selection.as_ref().is_some_and(|s| {
            let outputs: Vec<_> = self.space.outputs().filter_map(|o| self.space.output_geometry(o)).collect();
            outputs.iter().any(|g| !s.has_output(*g)) || s.outputs.iter().any(|(g, _)| !outputs.contains(g))
        }) {
            self.screenshot.selection = None;
            self.ignored_buttons.extend(self.physical_buttons.iter().copied());
            self.screenshot_notice("Screenshot cancelled", "Monitor layout changed during capture");
        }
        if self.tray.take_screenshot_request() { self.start_screenshot(); }
        let result = self.xr_bridge.lock().unwrap().screenshot_result.take();
        if let Some(result) = result {
            let send = self.screenshot.send.clone();
            self.screenshot.xr_started = None;
            std::thread::spawn(move || {
                let _ = send.send(result.map(|capture| {
                    let path = capture.save();
                    (capture, path)
                }));
            });
        }
        if self
            .screenshot
            .xr_started
            .is_some_and(|t| t.elapsed().as_secs() > 30)
        {
            self.xr_bridge.lock().unwrap().screenshot_request = None;
            self.screenshot.xr_started = None;
            self.screenshot.busy = false;
            self.screenshot_notice(
                "Screenshot failed",
                "The XR renderer did not complete the capture",
            );
        }
        while let Ok(result) = self.screenshot.receive.try_recv() {
            self.screenshot.busy = false;
            match result {
                Ok((capture, path)) => {
                    let gltf = matches!(capture, Capture::Gltf(_));
                    self.publish_capture(capture);
                    match path {
                        Ok(path) => self.screenshot_notice(
                            if gltf { "XR glTF capture copied and saved" } else { "Screenshot copied and saved" },
                            &path.display().to_string(),
                        ),
                        Err(e) => self.screenshot_notice("Screenshot copied; saving failed", &e),
                    }
                }
                Err(e) => self.screenshot_notice("Screenshot failed", &e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::backend::renderer::element::{
        memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement}, Kind,
    };

    #[test]
    fn desktop_capture_keeps_rows_columns_and_crop_in_both_presentations() {
        let mut renderer = crate::rendering::offscreen().expect("offscreen EGL renderer");
        // Every pixel differs in both axes, so reflection or rotation cannot pass.
        let size: Size<i32, Physical> = (7, 5).into();
        let rgba: Vec<u8> = (0..5).flat_map(|y| (0..7).flat_map(move |x|
            [x * 31, y * 47, (x + y) * 17, 255])).collect();
        let buffer = MemoryRenderBuffer::from_slice(&rgba,
            smithay::backend::allocator::Fourcc::Abgr8888, (7, 5), 1, Transform::Normal, None);
        let element = MemoryRenderBufferRenderElement::from_buffer(
            &mut renderer, (0., 0.), &buffer, None, None, None, Kind::Unspecified).unwrap();
        let elements = [element];
        for transform in [Transform::Normal, Transform::Flipped180] {
            let mut presentation: GlesTexture = renderer.create_buffer(
                smithay::backend::allocator::Fourcc::Abgr8888, (7, 5).into()).unwrap();
            let mut target: GlesTarget<'_> = renderer.bind(&mut presentation).unwrap();
            let mut damage = OutputDamageTracker::new(size, 1., transform);
            damage.render_output(&mut renderer, &mut target, 0, &elements, [0., 0., 0., 1.])
                .unwrap().sync.wait().unwrap();
            let pixels = capture_pixels(&mut renderer, &elements, size, [0., 0., 0., 1.]).unwrap();
            assert_eq!(pixels.rgba, rgba, "presentation transform: {transform:?}");
            let crop = pixels.crop([5, 4], [1, 1]).unwrap();
            let expected: Vec<u8> = (1..4).flat_map(|y| (1..5).flat_map(move |x|
                [x * 31, y * 47, (x + y) * 17, 255])).collect();
            assert_eq!(crop.rgba, expected, "crop orientation: {transform:?}");
            assert_eq!((crop.width, crop.height), (4, 3));
        }
    }
    #[test]
    fn multihead_capture_stitches_negative_offsets_gaps_and_cross_monitor_crops() {
        use smithay::backend::allocator::Fourcc;
        let mut renderer = crate::rendering::offscreen().unwrap();
        let geometries = [Rectangle::new((-4, -2).into(), (4, 3).into()),
                          Rectangle::new((0, 0).into(), (5, 4).into())];
        let mut selection = Selection::new(geometries.to_vec(), 42).unwrap();
        assert_eq!(selection.bounds, Rectangle::new((-4, -2).into(), (9, 6).into()));
        for (head, geometry) in geometries.into_iter().enumerate() {
            let (w, h) = (geometry.size.w, geometry.size.h);
            let rgba: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x|
                [head as u8 * 100 + x as u8 * 11, y as u8 * 23, 91, 255])).collect();
            let buffer = MemoryRenderBuffer::from_slice(&rgba, Fourcc::Abgr8888,
                (w, h), 1, Transform::Normal, None);
            let element = MemoryRenderBufferRenderElement::from_buffer(
                &mut renderer, (0., 0.), &buffer, None, None, None, Kind::Unspecified).unwrap();
            let pixels = capture_pixels(&mut renderer, &[element], (w, h).into(), [0., 0., 0., 1.]).unwrap();
            selection.record(geometry, pixels).unwrap();
            assert_eq!(selection.ready(), head == 1);
        }
        // Both axes vary per output: a reflected, shifted or swapped tile fails.
        for y in -2..4 {
            for x in -4..5 {
                let expected = if x < 0 && y < 1 {
                    [((x + 4) * 11) as u8, ((y + 2) * 23) as u8, 91, 255]
                } else if x >= 0 && y >= 0 {
                    [(100 + x * 11) as u8, (y * 23) as u8, 91, 255]
                } else { [0, 0, 0, 255] };
                let index = (((y + 2) * 9 + x + 4) * 4) as usize;
                assert_eq!(selection.pixels.rgba[index..index + 4], expected);
            }
        }
        let crop = selection.pixels.crop([7, 4], [2, 1]).unwrap();
        assert_eq!((crop.width, crop.height), (5, 3));
        let decoded = image::load_from_memory(&crop.png().unwrap()).unwrap().into_rgba8();
        assert_eq!(decoded.as_raw(), &crop.rgba);
        // The crop includes both heads and the empty corner between them.
        assert_eq!(&crop.rgba[..4], &[22, 23, 91, 255]);
        assert_eq!(&crop.rgba[3 * 4..4 * 4], &[0, 0, 0, 255]);
        assert_eq!(&crop.rgba[8 * 4..9 * 4], &[111, 0, 91, 255]);
    }

    #[test]
    fn multihead_capture_checks_layout_dimensions_and_mirrored_outputs() {
        let output = Rectangle::new((100, -20).into(), (4, 3).into());
        let mut selection = Selection::new(vec![output, output], 1).unwrap();
        assert!(!selection.ready());
        assert!(selection.record(output, Pixels::opaque_black(3, 3).unwrap()).is_err());
        assert!(selection.record(Rectangle::from_size((4, 3).into()),
            Pixels::opaque_black(4, 3).unwrap()).is_err());
        selection.record(output, Pixels::opaque_black(4, 3).unwrap()).unwrap();
        assert!(selection.ready());
        assert!(Selection::new(vec![], 1).is_err());
        assert!(Selection::new(vec![Rectangle::from_size((10000, 10000).into())], 1).is_err());
    }

}
