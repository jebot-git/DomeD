//! Loopback-only development control service; Wayland operations stay on the compositor thread.
use crate::{Action, Scene};

use std::{
    io::Read,
    sync::{Arc, Mutex},
    thread,
};
use tiny_http::{Header, Method, Response, Server};

pub use crate::desktop_entries::{apps, launch, launch_action, App};
pub fn serve(
    scene: Arc<Mutex<Scene>>,
    port: u16,
) -> Result<thread::JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
    serve_native(scene, port, None)
}
pub fn serve_native(
    scene: Arc<Mutex<Scene>>,
    port: u16,
    bridge: Option<crate::xr_transport::Shared>,
) -> Result<thread::JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(("127.0.0.1", port))?;
    let applications = apps();
    let socket = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    Ok(thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let host = format!("127.0.0.1:{port}");
            let allowed = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Host"))
                .is_some_and(|h| h.value.as_str() == host)
                && request
                    .headers()
                    .iter()
                    .filter(|h| h.field.equiv("Origin"))
                    .all(|h| h.value.as_str() == format!("http://{host}"))
                && request
                    .headers()
                    .iter()
                    .filter(|h| h.field.equiv("Sec-Fetch-Site"))
                    .all(|h| matches!(h.value.as_str(), "same-origin" | "none"));
            if !allowed {
                let _ = request.respond(
                    Response::from_string("Local same-origin requests only").with_status_code(403),
                );
                continue;
            }
            if request.method() == &Method::Get && request.url() == "/api/keybindings/defaults" {
                let _ = request.respond(Response::from_string(serde_json::to_string(&crate::shortcuts::defaults()).unwrap()).with_header(Header::from_bytes("Content-Type", "application/json").unwrap()));
                continue;
            }
            if request.method() == &Method::Get && request.url() == "/api/xr-control" {
                let control = bridge
                    .as_ref()
                    .map(|b| b.lock().unwrap().control())
                    .unwrap_or_default();
                let _ = request.respond(
                    Response::from_string(serde_json::to_string(&control).unwrap()).with_header(
                        Header::from_bytes("Content-Type", "application/json").unwrap(),
                    ),
                );
                continue;
            }
            if request.method() == &Method::Get && matches!(request.url(), "/api/integrations/qimgvr" | "/api/integrations/mpvr") {
                let app_id = if request.url().ends_with("/mpvr") { crate::stereo::MPVR_SBS_APP_ID } else { crate::stereo::QIMGVR_SBS_APP_ID };
                let status = bridge.as_ref().map(|b| {
                    let b = b.lock().unwrap();
                    serde_json::json!({"version":1, "compositor_pid":std::process::id(),
                        "xr_socket":b.xr_socket, "xr_active":b.visible(), "sbs_app_id":app_id})
                }).unwrap_or_else(|| serde_json::json!({"version":1,"xr_active":false}));
                let _ = request.respond(Response::from_string(status.to_string())
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap()));
                continue;
            }
            if request.method() == &Method::Post && request.url() == "/api/xr-session" {
                let mut body = String::new();
                let json = request.headers().iter().any(|h| {
                    h.field.equiv("Content-Type") && h.value.as_str() == "application/json"
                });
                let result = request
                    .as_reader()
                    .take(4097)
                    .read_to_string(&mut body)
                    .map_err(|e| e.to_string())
                    .and_then(|_| {
                        if json && body.len() <= 4096 {
                            serde_json::from_str::<crate::xr_transport::Session>(&body)
                                .map_err(|e| e.to_string())
                        } else {
                            Err("Invalid session status".into())
                        }
                    })
                    .and_then(|s| {
                        bridge
                            .as_ref()
                            .ok_or("Native compositor required")?
                            .lock()
                            .unwrap()
                            .heartbeat(s)
                    });
                let (code, value) = match result {
                    Ok(()) => (200, serde_json::json!({})),
                    Err(e) => (400, serde_json::json!({"error":e})),
                };
                let _ = request.respond(
                    Response::from_string(value.to_string())
                        .with_status_code(code)
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        ),
                );
                continue;
            }
            if request.method() == &Method::Get && request.url() == "/api/xr-shell" {
                let layout = bridge
                    .as_ref()
                    .map(|b| b.lock().unwrap().shell.clone())
                    .unwrap_or_default();
                let _ = request.respond(
                    Response::from_string(serde_json::to_string(&layout).unwrap()).with_header(
                        Header::from_bytes("Content-Type", "application/json").unwrap(),
                    ),
                );
                continue;
            }
            if request.method() == &Method::Get && request.url().starts_with("/api/xr-frame/") {
                let id = request
                    .url()
                    .trim_start_matches("/api/xr-frame/")
                    .parse::<u64>()
                    .ok();
                let valid = id.is_some_and(|id| {
                    crate::xr_transport::is_shell(id) || {
                        let scene = scene.lock().unwrap();
                        scene.window(id).is_some() || scene.popup(id).is_some()
                    }
                });
                let frame = if valid {
                    bridge.as_ref().and_then(|b| {
                        let mut b = b.lock().unwrap();
                        let id = id.unwrap();
                        b.requested.insert(id, std::time::Instant::now());
                        b.frames
                            .get(&id)
                            .filter(|f| f.stamp.elapsed().as_secs_f32() < 0.5)
                            .cloned()
                    })
                } else {
                    None
                };
                if let Some(f) = frame {
                    let _ = request.respond(
                        Response::from_data((*f.rgba).clone())
                            .with_header(
                                Header::from_bytes("Content-Type", "application/octet-stream")
                                    .unwrap(),
                            )
                            .with_header(
                                Header::from_bytes("X-Dome-Width", f.width.to_string()).unwrap(),
                            )
                            .with_header(
                                Header::from_bytes("X-Dome-Height", f.height.to_string()).unwrap(),
                            ),
                    );
                } else {
                    let _ = request
                        .respond(Response::from_string("Frame not ready").with_status_code(409));
                }
                continue;
            }
            if request.method() == &Method::Post && request.url() == "/api/xr-input" {
                let json = request.headers().iter().any(|h| {
                    h.field.equiv("Content-Type") && h.value.as_str() == "application/json"
                });
                let mut body = String::new();
                let read = request.as_reader().take(16385).read_to_string(&mut body);
                let result = if !json || read.is_err() || body.len() > 16384 {
                    Err("Invalid XR packet".into())
                } else {
                    serde_json::from_str::<crate::xr_input::Packet>(&body)
                        .map_err(|e| e.to_string())
                        .and_then(|p| {
                            let mut check = scene.lock().unwrap().clone();
                            for a in &p.actions {
                                if !matches!(
                                    a,
                                    Action::Focus { .. }
                                        | Action::SpatialTransform { .. }
                                        | Action::PullOut { .. }
                                        | Action::ReturnToDome { .. }
                                        | Action::DomeOffset { .. }
                                ) {
                                    return Err("Unsupported XR action".into());
                                }
                                check.apply(a.clone())?;
                            }
                            for e in &p.events {
                                if let crate::xr_input::SeatEvent::Scroll {
                                    horizontal,
                                    vertical,
                                } = e
                                {
                                    if [horizontal, vertical]
                                        .iter()
                                        .any(|v| !v.is_finite() || v.abs() > 10000.)
                                    {
                                        return Err("Invalid scroll".into());
                                    }
                                }
                                if let crate::xr_input::SeatEvent::Motion { id, uv } = e {
                                    if (!crate::xr_transport::is_shell(*id)
                                        && check.window(*id).is_none()
                                        && check.popup(*id).is_none())
                                        || uv.iter().any(|v| !v.is_finite() || v.abs() > 100.)
                                    {
                                        return Err("Invalid pointer target".into());
                                    }
                                }
                                if let crate::xr_input::SeatEvent::Button { button, .. } = e {
                                    if ![0x110, 0x111, 0x112].contains(button) {
                                        return Err("Unsupported pointer button".into());
                                    }
                                }
                            }
                            bridge
                                .as_ref()
                                .ok_or("XR input requires the native compositor")?
                                .lock()
                                .unwrap()
                                .push(p)
                        })
                };
                let (code, body) = match result {
                    Ok(()) => (200, "{}".to_string()),
                    Err(e) => (400, serde_json::json!({"error":e}).to_string()),
                };
                let _ = request.respond(
                    Response::from_string(body)
                        .with_status_code(code)
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        ),
                );
                continue;
            }
            let (body, mime, status) = match (request.method(), request.url()) {
                (&Method::Get, "/") => (
                    include_str!("../web/index.html").to_string(),
                    "text/html",
                    200,
                ),
                (&Method::Get, "/app.js") => (
                    include_str!("../web/app.js").to_string(),
                    "text/javascript",
                    200,
                ),
                (&Method::Get, "/style.css") => (
                    include_str!("../web/style.css").to_string(),
                    "text/css",
                    200,
                ),
                (&Method::Get, "/api/state") => {
                    let s = scene.lock().unwrap();
                    (serde_json::json!({"scene":&*s,"panels":s.panels(),"spans":s.config.lane_spans(s.workspaces[s.active_workspace].shares)}).to_string(),"application/json",200)
                }
                (&Method::Get, "/api/xr-scene") => {
                    let scene = scene.lock().unwrap();
                    let frame = scene.frame_pose();
                    let panels:Vec<_>=scene.panels().iter().map(|p|{
                    let vertices:Vec<_>=scene.mesh(p,48).iter().map(|point|frame.inverse_rotate(std::array::from_fn(|i|point[i]-frame.position_m[i]))).collect();
                    let window=scene.window(p.id).unwrap();
                    serde_json::json!({"id":p.id,"workspace":p.workspace,"opacity":p.opacity,"vertices":vertices,"title":window.title,"app_id":window.app_id})
                }).collect();
                    (serde_json::json!({"revision":scene.revision,"config":scene.config,"source":scene.source,"active_workspace":scene.active_workspace,"panels":panels}).to_string(),"application/json",200)
                }
                (&Method::Get, "/api/apps") => (
                    serde_json::to_string(
                        &applications
                            .iter()
                            .filter(|a| a.visible)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                    "application/json",
                    200,
                ),
                (&Method::Post, "/api/action" | "/api/launch") => {
                    let is_launch = request.url() == "/api/launch";
                    let json = request.headers().iter().any(|h| {
                        h.field.equiv("Content-Type") && h.value.as_str() == "application/json"
                    });
                    let mut body = String::new();
                    let read = request.as_reader().take(16385).read_to_string(&mut body);
                    let result = if !json || read.is_err() || body.len() > 16384 {
                        Err("Expected JSON, maximum 16 KiB".into())
                    } else if is_launch {
                        match serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|v| v["id"].as_u64())
                            .and_then(|id| applications.get(id as usize))
                        {
                            Some(app) if scene.lock().unwrap().source != "preview" => {
                                let value: serde_json::Value = serde_json::from_str(&body).unwrap();
                                match value
                                    .get("realm")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("desktop")
                                {
                                    "desktop" => launch(app, &socket),
                                    "xr" => bridge
                                        .as_ref()
                                        .ok_or_else(|| "Native compositor required".to_string())
                                        .and_then(|b| launch(app, &b.lock().unwrap().xr_socket)),
                                    _ => Err("Unknown launch realm".into()),
                                }
                            }
                            Some(_) => {
                                Err("App launching is enabled only by the nested compositor".into())
                            }
                            None => Err("Unknown app".into()),
                        }
                    } else {
                        serde_json::from_str::<Action>(&body)
                            .map_err(|e| e.to_string())
                            .and_then(|action| scene.lock().unwrap().apply(action))
                    };
                    match result {
                        Ok(()) => ("{}".into(), "application/json", 200),
                        Err(e) => (
                            serde_json::json!({"error":e}).to_string(),
                            "application/json",
                            400,
                        ),
                    }
                }
                _ => ("Not found".into(), "text/plain", 404),
            };
            let response=Response::from_string(body).with_status_code(status)
            .with_header(Header::from_bytes("Content-Type",format!("{mime}; charset=utf-8")).unwrap())
            .with_header(Header::from_bytes("Cache-Control","no-store").unwrap())
            .with_header(Header::from_bytes("Content-Security-Policy","default-src 'self'; style-src 'self'; script-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'").unwrap())
            .with_header(Header::from_bytes("X-Content-Type-Options","nosniff").unwrap());
            let _ = request.respond(response);
        }
    }))
}
