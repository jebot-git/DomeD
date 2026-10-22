//! Read CPU copies only for the requested frame, without disabling DMA-BUF rendering.
use super::{Duration, Image, Instant, Read};
use dome::screenshot::{Eye, Layer, Pixels};
pub fn capture(
    base: String,
    id: u64,
    sources: Vec<(dome::xr_input::Quad, Image, bool, f32)>,
    eyes: [Eye; 2],
    size: [u32; 2],
    border: u8,
    focused: Option<u64>,
    accent: [u8; 3],
    browser_visible: bool,
) {
    std::thread::spawn(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(2))
            .build();
        let result = (|| -> Result<Vec<u8>, String> {
            if browser_visible {
                return Err("Screenshot of an immersive WebXR projection is not available; return to the DomeD view first".into());
            }
            let deadline = Instant::now() + Duration::from_secs(12);
            let mut layers = vec![];
            for (quad, image, stereo, opacity) in sources {
                let pixels = if image.rgba.is_empty() {
                    let mut pixels = None;
                    while Instant::now() < deadline {
                        if let Ok(response) = agent
                            .get(&format!("{base}/api/xr-frame/{}", quad.id))
                            .call()
                        {
                            let width = response
                                .header("X-Dome-Width")
                                .and_then(|v| v.parse::<u32>().ok())
                                .unwrap_or(0);
                            let height = response
                                .header("X-Dome-Height")
                                .and_then(|v| v.parse::<u32>().ok())
                                .unwrap_or(0);
                            let scale = response
                                .header("X-Dome-Scale")
                                .and_then(|v| v.parse::<f32>().ok())
                                .filter(|s| s.is_finite() && *s > 0. && *s <= 2.)
                                .unwrap_or(1.);
                            let mut rgba = vec![];
                            if width > 0
                                && height > 0
                                && width <= 4096
                                && height <= 4096
                                && response
                                    .into_reader()
                                    .take(67108865)
                                    .read_to_end(&mut rgba)
                                    .is_ok()
                            {
                                let mut p = Pixels {
                                    width,
                                    height,
                                    rgba,
                                };
                                if p.valid() {
                                    if !dome::xr_transport::is_shell(quad.id) && !stereo {
                                        dome::xr_input::paint_border_scaled(
                                            &mut p.rgba,
                                            width,
                                            height,
                                            border,
                                            scale,
                                        );
                                    }
                                    if focused == Some(quad.id) {
                                        dome::visual_feedback::focus_border_scaled(
                                            &mut p.rgba,
                                            width,
                                            height,
                                            accent,
                                            stereo,
                                            scale,
                                        );
                                    }
                                    pixels = Some(p);
                                    break;
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    pixels
                        .ok_or_else(|| format!("No screenshot pixels for XR surface {}", quad.id))?
                } else {
                    Pixels {
                        width: image.width,
                        height: image.height,
                        rgba: image.rgba,
                    }
                };
                layers.push(Layer {
                    quad,
                    pixels,
                    stereo,
                    opacity,
                });
            }
            dome::screenshot::sbs(&layers, eyes, size[0], size[1])?.png()
        })();
        let request = agent.post(&format!("{base}/api/screenshot/xr/{id}"));
        let response = match result {
            Ok(png) => request.set("Content-Type", "image/png").send_bytes(&png),
            Err(e) => request.set("Content-Type", "text/plain").send_string(&e),
        };
        if let Err(e) = response {
            eprintln!("XR screenshot result: {e}");
        }
    });
}
