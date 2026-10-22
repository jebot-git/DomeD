//! Compositor-owned screenshot selection; compression and storage run off-thread.
use dome::screenshot::Pixels;
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
    pub pixels: Option<Pixels>,
    pub start: Option<[i32; 2]>,
    pub end: [i32; 2],
    pub serial: u64,
}
pub struct Screenshots {
    pub selection: Option<Selection>,
    serial: u64,
    busy: bool,
    xr_started: Option<Instant>,
    send: mpsc::Sender<Result<(Vec<u8>, Result<std::path::PathBuf, String>), String>>,
    receive: mpsc::Receiver<Result<(Vec<u8>, Result<std::path::PathBuf, String>), String>>,
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
            let bounds =
                self.output_geometry_at(self.seat.get_pointer().unwrap().current_location());
            if bounds.size.w as usize * bounds.size.h as usize > dome::screenshot::MAX_PIXELS {
                self.screenshot_notice("Screenshot failed", "Output is too large");
                return;
            }
            self.screenshot.serial += 1;
            self.screenshot.selection = Some(Selection {
                bounds,
                pixels: None,
                start: None,
                end: [0, 0],
                serial: self.screenshot.serial,
            });
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
        if selection.pixels.is_none() {
            return true;
        }
        if pressed {
            selection.start = Some(selection.end);
        } else if let Some(start) = selection.start {
            let selection = self.screenshot.selection.take().unwrap();
            if let Some(pixels) = selection.pixels.unwrap().crop(start, selection.end) {
                self.screenshot.busy = true;
                let send = self.screenshot.send.clone();
                std::thread::spawn(move || {
                    let result = pixels.png().map(|png| {
                        let path = dome::screenshot::save_png(&png, false);
                        (png, path)
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
        origin: Point<i32, Logical>,
        background: [f32; 4],
    ) {
        let Some(selection) = self
            .screenshot
            .selection
            .as_mut()
            .filter(|s| s.pixels.is_none() && s.bounds.loc == origin)
        else {
            return;
        };
        let size = selection.bounds.size;
        let result = capture_pixels(renderer, elements, (size.w, size.h).into(), background);
        match result {
            Ok(pixels) => selection.pixels = Some(pixels),
            Err(e) => {
                let message = e.to_string();
                self.screenshot.selection = None;
                self.screenshot_notice("Screenshot failed", &message);
            }
        }
    }
    pub fn poll_screenshots(&mut self) {
        if self.tray.take_screenshot_request() { self.start_screenshot(); }
        let result = self.xr_bridge.lock().unwrap().screenshot_result.take();
        if let Some(result) = result {
            let send = self.screenshot.send.clone();
            self.screenshot.xr_started = None;
            std::thread::spawn(move || {
                let _ = send.send(result.map(|png| {
                    let path = dome::screenshot::save_png(&png, true);
                    (png, path)
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
                Ok((png, path)) => {
                    self.publish_screenshot(png);
                    match path {
                        Ok(path) => self.screenshot_notice(
                            "Screenshot copied and saved",
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
}
