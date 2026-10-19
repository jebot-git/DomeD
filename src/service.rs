//! Loopback-only development control service; Wayland operations stay on the compositor thread.
use crate::{Action, Scene};
use serde::Serialize;
use std::{
    fs,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};
use tiny_http::{Header, Method, Response, Server};

#[derive(Clone, Serialize)]
pub struct App {
    pub id: usize,
    pub name: String,
    #[serde(skip)]
    pub path: PathBuf,
}
pub fn apps() -> Vec<App> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs = vec![PathBuf::from(
        std::env::var("XDG_DATA_HOME").unwrap_or(format!("{home}/.local/share")),
    )];
    dirs.extend(
        std::env::var("XDG_DATA_DIRS")
            .unwrap_or("/usr/local/share:/usr/share".into())
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from),
    );
    let mut result = vec![];
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        if let Ok(files) = fs::read_dir(dir.join("applications")) {
            for f in files.flatten() {
                let path = f.path();
                if path.extension().and_then(|p| p.to_str()) != Some("desktop") {
                    continue;
                }
                // User entries shadow system entries, including hidden tombstones.
                if !seen.insert(f.file_name()) {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                let mut section = false;
                let mut name = None;
                let mut hidden = false;
                let mut executable = false;
                let mut application = false;
                for line in text.lines() {
                    let line = line.trim();
                    if line.starts_with('[') {
                        section = line == "[Desktop Entry]";
                        continue;
                    }
                    if !section {
                        continue;
                    }
                    if let Some(n) = line.strip_prefix("Name=") {
                        name = Some(n.to_string());
                    }
                    if line == "Hidden=true" || line == "NoDisplay=true" || line == "Terminal=true"
                    {
                        hidden = true;
                    }
                    if line.starts_with("Exec=") {
                        executable = true;
                    }
                    if line == "Type=Application" {
                        application = true;
                    }
                }
                if let Some(name) = name.filter(|_| !hidden && executable && application) {
                    result.push(App { id: 0, name, path });
                }
            }
        }
    }
    result.sort_by_key(|a| a.name.to_lowercase());
    for (id, a) in result.iter_mut().enumerate() {
        a.id = id;
    }
    result
}
pub fn launch(app: &App, socket: &str) -> Result<(), String> {
    std::process::Command::new("gio")
        .arg("launch")
        .arg(&app.path)
        .env("WAYLAND_DISPLAY", socket)
        .env("GDK_BACKEND", "wayland")
        .env("QT_QPA_PLATFORM", "wayland")
        .env("SDL_VIDEODRIVER", "wayland")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot start gio: {e}"))
}
pub fn serve(
    scene: Arc<Mutex<Scene>>,
    port: u16,
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
                (&Method::Get, "/api/apps") => (
                    serde_json::to_string(&applications).unwrap(),
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
                                launch(app, &socket)
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
