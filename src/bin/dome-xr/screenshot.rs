//! Read CPU copies only for the requested frame, without disabling DMA-BUF rendering.
use super::{Duration, Image, Instant, Read};
use dome::screenshot::{Capture, Eye, Layer, Pixels};
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
    gltf: bool,
    head: dome::Pose,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(2))
            .build();
        let result = (|| -> Result<Capture, String> {
            if browser_visible {
                return Err("Screenshot of an immersive WebXR projection is not available; return to the DomeD view first".into());
            }
            let deadline = Instant::now() + Duration::from_secs(12);
            let mut layers = vec![];
            let mut total_pixels = 0usize;
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
                total_pixels += pixels.width as usize * pixels.height as usize;
                if total_pixels > dome::screenshot::MAX_PIXELS {
                    return Err("XR capture textures exceed 32 megapixels".into());
                }
                layers.push(Layer {
                    quad,
                    pixels,
                    stereo,
                    opacity,
                });
            }
            if gltf {
                dome::screenshot::glb(&layers, eyes, head).map(Capture::Gltf)
            } else {
                dome::screenshot::sbs(&layers, eyes, size[0], size[1])?.png()
                    .map(|bytes|Capture::Png { bytes, stereo:true })
            }
        })();
        let request = agent.post(&format!("{base}/api/screenshot/xr/{id}"));
        let response = match result {
            Ok(capture) => request.set("Content-Type", capture.mime_type()).send_bytes(capture.bytes()),
            Err(e) => request.set("Content-Type", "text/plain").send_string(&e),
        };
        if let Err(e) = response {
            eprintln!("XR screenshot result: {e}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::{Header, Response, Server};

    fn eyes() -> [Eye;2] {
        [-0.032,0.032].map(|x|Eye {pose:dome::Pose {position_m:[x,0.,0.],..Default::default()},fov:[-0.7,0.7,-0.7,0.7]})
    }
    #[test]
    fn gpu_capture_fetches_pixels_and_posts_spatial_model_without_changing_transport() {
        let server=Server::http("127.0.0.1:0").unwrap();
        let base=format!("http://{}",server.server_addr());
        let quad=dome::xr_input::Quad {id:77,pose:dome::Pose {position_m:[0.,0.,-2.],..Default::default()},size:[2.,1.],dome:None};
        // Empty CPU data represents the normal DMA-BUF rendering path.
        let image=Image {width:4,height:2,rgba:vec![],gpu:None,serial:1};
        let worker=capture(base,12,vec![(quad,image,false,1.)],eyes(),[8,8],1,None,[0,0,0],false,true,dome::Pose::default());
        let request=server.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        assert_eq!(request.url(),"/api/xr-frame/77");
        request.respond(Response::from_data([12,34,56,255].repeat(8))
            .with_header(Header::from_bytes("X-Dome-Width","4").unwrap())
            .with_header(Header::from_bytes("X-Dome-Height","2").unwrap())
            .with_header(Header::from_bytes("X-Dome-Scale","1").unwrap())).unwrap();
        let mut request=server.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        assert_eq!(request.url(),"/api/screenshot/xr/12");
        assert!(request.headers().iter().any(|h|h.field.equiv("Content-Type")&&h.value.as_str()=="model/gltf-binary"));
        let mut bytes=vec![];request.as_reader().read_to_end(&mut bytes).unwrap();
        dome::screenshot::validate_glb(&bytes).unwrap();
        request.respond(Response::from_string("ok")).unwrap();worker.join().unwrap();
    }
    #[test]
    fn immersive_webxr_still_reports_an_error_instead_of_an_incomplete_capture() {
        let server=Server::http("127.0.0.1:0").unwrap();
        let worker=capture(format!("http://{}",server.server_addr()),13,vec![],eyes(),[8,8],1,None,[0,0,0],true,true,dome::Pose::default());
        let mut request=server.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        assert_eq!(request.url(),"/api/screenshot/xr/13");
        assert!(request.headers().iter().any(|h|h.field.equiv("Content-Type")&&h.value.as_str()=="text/plain"));
        let mut text=String::new();request.as_reader().read_to_string(&mut text).unwrap();assert!(text.contains("WebXR"));
        request.respond(Response::from_string("ok")).unwrap();worker.join().unwrap();
    }
    #[test]
    fn legacy_desktop_keeps_the_original_sbs_pixels_and_mime_type() {
        let server=Server::http("127.0.0.1:0").unwrap();
        let quad=dome::xr_input::Quad {id:78,pose:dome::Pose {position_m:[0.,0.,-2.],..Default::default()},size:[4.,4.],dome:None};
        let image=Image {width:2,height:1,rgba:vec![255,0,0,255,0,0,255,255],gpu:None,serial:1};
        let worker=capture(format!("http://{}",server.server_addr()),14,vec![(quad,image,true,1.)],eyes(),[1,1],0,None,[0,0,0],false,false,dome::Pose::default());
        let mut request=server.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        assert_eq!(request.url(),"/api/screenshot/xr/14");
        assert!(request.headers().iter().any(|h|h.field.equiv("Content-Type")&&h.value.as_str()=="image/png"));
        let mut bytes=vec![];request.as_reader().read_to_end(&mut bytes).unwrap();
        let decoded=image::load_from_memory(&bytes).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(),(2,1));assert_eq!(decoded.as_raw(),&[255,0,0,255,0,0,255,255]);
        request.respond(Response::from_string("ok")).unwrap();worker.join().unwrap();
    }
}
