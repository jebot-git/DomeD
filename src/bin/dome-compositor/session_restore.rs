//! Login-session reopening and placement; disk/proc work stays off the render thread.
use crate::Dome;
use dome::{
    session_state::{Layout, Restart, SavedSession},
    Realm,
};
use smithay::{
    reexports::wayland_server::Resource,
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Client {
    id: u64,
    pid: u32,
    realm: Realm,
    desktop_id: Option<String>,
    app_id: String,
}
struct Save {
    layout: Layout,
    clients: Vec<Client>,
    extra_restarts: Vec<Restart>,
    done: Option<mpsc::Sender<Result<(), String>>>,
}
pub struct Persistence {
    send: mpsc::SyncSender<Save>,
    layout: Option<Layout>,
    restored: HashMap<u64, u64>,
    original_restarts: Vec<Restart>,
    reopened: mpsc::Receiver<(u32, Vec<u64>)>,
    process_windows: HashMap<u32, Vec<u64>>,
    focus_restored: bool,
    started: Instant,
    next_tick: Instant,
    last_save: Instant,
    fingerprint: Vec<u8>,
    interacted: bool,
    closing: bool,
}
fn path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("dome/session.json")
}
fn enabled() -> bool {
    match std::env::var("DOME_SESSION_RESTORE").as_deref() {
        Ok("0") => false,
        Ok("1") => true,
        _ => {
            std::env::var("DOME_LOGIN_SESSION").as_deref() == Ok("1")
                && std::env::var_os("DOME_DRM_TEST_DEVICE").is_none()
                && std::env::var_os("DOME_DRM_TEST_DEVICES").is_none()
        }
    }
}
fn read(path: &Path) -> Result<Option<SavedSession>, String> {
    match fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(meta) if meta.len() > 4 * 1024 * 1024 => return Err("Session file is too large".into()),
        _ => (),
    }
    let saved: SavedSession = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    saved.validate()?;
    Ok(Some(saved))
}
fn argv(pid: u32) -> Option<Vec<String>> {
    let bytes = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Option<Vec<_>> = bytes
        .strip_suffix(&[0])
        .unwrap_or(&bytes)
        .split(|b| *b == 0)
        .map(|v| String::from_utf8(v.to_vec()).ok())
        .collect();
    args.filter(|v| !v.is_empty())
}
fn parent(pid: u32) -> Option<u32> {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("PPid:")
                .and_then(|pid| pid.trim().parse().ok())
        })
}
fn restart(client: &Client) -> Option<(String, Restart)> {
    // The SBS presenter is recreated by its owning viewer, rather than restarted
    // with an orphaned stdin protocol and temporary image paths.
    let own_uid = fs::metadata("/proc/self").ok()?.uid();
    let mut pid = client.pid;
    if pid == 0 || fs::metadata(format!("/proc/{pid}")).ok()?.uid() != own_uid {
        return None;
    }
    if client.app_id == "org.qimgvr.Sbs" {
        let mut owner = None;
        for _ in 0..6 {
            pid = parent(pid)?;
            if pid <= 1 || fs::metadata(format!("/proc/{pid}")).ok()?.uid() != own_uid {
                break;
            }
            if argv(pid).is_some_and(|args| {
                Path::new(&args[0])
                    .file_name()
                    .is_some_and(|name| name == "qimgvr" || name == "qimgv")
            }) {
                owner = Some(pid);
                break;
            }
        }
        pid = owner?;
    }
    let mut args = argv(pid)?;
    if let Some(ppid) = parent(pid) {
        if fs::metadata(format!("/proc/{ppid}")).is_ok_and(|meta| meta.uid() == own_uid) {
            if let Some(parent_args) = argv(ppid).filter(|args| {
                args.iter()
                    .any(|arg| Path::new(arg).file_name().is_some_and(|n| n == "mpvr.py"))
            }) {
                pid = ppid;
                args = parent_args;
            }
        }
    }
    let cwd = fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .filter(|p| p.is_dir());
    // Preserve a virtual-environment interpreter or wrapper path. Resolving its
    // symlink through /proc/exe would silently select a different environment.
    let requested = Path::new(&args[0]);
    let executable = if requested.is_absolute() && requested.is_file() {
        Some(requested.to_path_buf())
    } else if args[0].contains('/') {
        cwd.as_ref()
            .map(|cwd| cwd.join(requested))
            .filter(|path| path.is_file())
    } else {
        dome::desktop_entries::find_executable(&args[0])
    }
    .or_else(|| {
        fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .filter(|path| path.is_file())
    })?;
    args[0] = executable.to_string_lossy().into_owned();
    let env = fs::read(format!("/proc/{pid}/environ"))
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter_map(|entry| std::str::from_utf8(entry).ok()?.split_once('='))
        .filter(|(name, _)| matches!(*name, "QIMGV_SBS_PYTHON" | "ARC_SBS_MODEL"))
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
    Some((
        format!("pid:{pid}"),
        Restart {
            argv: args,
            cwd,
            env,
            realm: if client.app_id == "org.qimgvr.Sbs" {
                Realm::Desktop
            } else {
                client.realm
            },
            windows: vec![client.id],
            desktop_id: client.desktop_id.clone(),
        },
    ))
}
fn write(path: &Path, save: Save) -> Result<(), String> {
    use std::io::Write;
    let mut restarts: BTreeMap<String, Restart> = BTreeMap::new();
    for client in save.clients {
        let command = restart(&client).or_else(|| {
            if client.app_id == "org.qimgvr.Sbs" {
                return None;
            }
            let id = client.desktop_id.clone()?;
            Some((
                format!("desktop:{id}"),
                Restart {
                    argv: vec![],
                    cwd: None,
                    env: BTreeMap::new(),
                    realm: client.realm,
                    windows: vec![client.id],
                    desktop_id: Some(id),
                },
            ))
        });
        if let Some((key, command)) = command {
            if let Some(existing) = restarts.get_mut(&key) {
                existing.windows.push(client.id);
                if client.realm == Realm::Desktop {
                    existing.realm = Realm::Desktop;
                }
            } else {
                restarts.insert(key, command);
            }
        }
    }
    let snapshot = SavedSession {
        version: 1,
        layout: save.layout,
        restarts: restarts.into_values().chain(save.extra_restarts).collect(),
    };
    snapshot.validate()?;
    let bytes = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Session data exceeds 4 MiB".into());
    }
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.map_err(|e: std::io::Error| e.to_string())
}
fn reopen(restarts: Vec<Restart>, desktop: String, xr: String) -> mpsc::Receiver<(u32, Vec<u64>)> {
    let (send, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let apps = dome::service::apps();
        for restart in restarts {
            let socket = if restart.realm == Realm::Xr {
                &xr
            } else {
                &desktop
            };
            if restart.argv.is_empty() || !Path::new(&restart.argv[0]).is_file() {
                if let Some(app) = apps
                    .iter()
                    .find(|app| Some(&app.desktop_id) == restart.desktop_id.as_ref())
                {
                    if let Err(error) = dome::desktop_entries::launch(app, socket) {
                        eprintln!("Session restore: {error}");
                    }
                } else {
                    eprintln!("Session restore: application is no longer installed");
                }
                continue;
            }
            let mut command = std::process::Command::new(&restart.argv[0]);
            command
                .args(&restart.argv[1..])
                .envs(restart.env)
                .env("WAYLAND_DISPLAY", socket)
                .env_remove("WAYLAND_SOCKET")
                .envs(
                    dome::desktop_entries::launch_environment(Path::new(
                        restart.desktop_id.as_deref().unwrap_or(&restart.argv[0]),
                    )).iter().copied(),
                )
                .env("GDK_BACKEND", "wayland,x11")
                .env("QT_QPA_PLATFORM", "wayland;xcb")
                .env("SDL_VIDEODRIVER", "wayland,x11")
                .stdin(std::process::Stdio::null());
            if let Some(cwd) = restart.cwd.filter(|path| path.is_dir()) {
                command.current_dir(cwd);
            }
            match command.spawn() {
                Ok(mut child) => {
                    let _ = send.send((child.id(), restart.windows));
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(error) => eprintln!("Session restore: {}: {error}", restart.argv[0]),
            }
        }
    });
    receiver
}
impl Dome {
    pub fn start_session_restore(&mut self) {
        if !enabled() {
            return;
        }
        let path = path();
        let saved = read(&path).unwrap_or_else(|error| {
            eprintln!("Session restore: {error}");
            None
        });
        let (send, recv) = mpsc::sync_channel::<Save>(1);
        std::thread::spawn(move || {
            for mut save in recv {
                let done = save.done.take();
                let result = write(&path, save);
                if let Err(error) = &result {
                    eprintln!("Session save: {error}");
                }
                if let Some(done) = done {
                    let _ = done.send(result);
                }
            }
        });
        let now = Instant::now();
        let original_restarts = saved
            .as_ref()
            .map_or_else(Vec::new, |saved| saved.restarts.clone());
        let (layout, reopened) = if let Some(saved) = saved {
            saved.layout.prepare(&mut self.scene.lock().unwrap());
            let receiver = reopen(
                saved.restarts,
                self.socket_name.to_string_lossy().into_owned(),
                self.xr_socket_name.to_string_lossy().into_owned(),
            );
            (Some(saved.layout), receiver)
        } else {
            (None, mpsc::channel().1)
        };
        self.session_persistence = Some(Persistence {
            send,
            layout,
            restored: HashMap::new(),
            original_restarts,
            reopened,
            process_windows: HashMap::new(),
            focus_restored: false,
            started: now,
            next_tick: now,
            last_save: now,
            fingerprint: vec![],
            interacted: false,
            closing: false,
        });
    }
    fn session_snapshot(&self, persistence: &Persistence) -> Save {
        let ids: HashSet<_> = self.registry.iter().map(|(id, _)| *id).collect();
        let mut excluded: HashSet<_> = self
            .scene
            .lock()
            .unwrap()
            .windows
            .iter()
            .filter(|w| !ids.contains(&w.id))
            .map(|w| w.id)
            .collect();
        let mut clients = Vec::new();
        for (id, window) in &self.registry {
            let surface = window.toplevel().unwrap().wl_surface();
            let transient = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .parent
                    .is_some()
            });
            if transient {
                excluded.insert(*id);
                continue;
            }
            let scene = self.scene.lock().unwrap();
            let Some(window) = scene.window(*id) else {
                continue;
            };
            let pid = surface
                .client()
                .and_then(|client| client.get_credentials(&self.display_handle).ok())
                .map_or(0, |cred| cred.pid as u32);
            clients.push(Client {
                id: *id,
                pid,
                realm: window.origin,
                app_id: window.app_id.clone(),
                desktop_id: self
                    .apps
                    .iter()
                    .find(|app| app.matches(&window.app_id))
                    .map(|app| app.desktop_id.clone()),
            });
        }
        let mut layout = Layout::capture(&self.scene.lock().unwrap(), &excluded);
        let depth: HashMap<_, _> = self
            .space
            .elements()
            .enumerate()
            .filter_map(|(rank, window)| {
                self.id_for(window.toplevel().unwrap().wl_surface())
                    .map(|id| (id, rank + 1))
            })
            .collect();
        layout
            .windows
            .sort_by_key(|window| depth.get(&window.id).copied().unwrap_or(0));
        let mut extra_restarts = Vec::new();
        // A headset-dependent presenter may appear much later than login. Keep
        // its unmatched placement, using fresh snapshot IDs to avoid collisions.
        if let Some(previous) = &persistence.layout {
            let mut pending_ids = HashMap::new();
            let mut next = layout.windows.iter().map(|w| w.id).max().unwrap_or(0) + 1;
            for saved in previous
                .windows
                .iter()
                .filter(|w| !persistence.restored.contains_key(&w.id))
            {
                let mut window = saved.clone();
                pending_ids.insert(saved.id, next);
                window.id = next;
                next += 1;
                window.workspace = window.workspace.min(layout.workspaces.len() - 1);
                layout.workspaces[window.workspace].lanes[window.lane]
                    .windows
                    .push(window.id);
                if window.pullout_slot.is_some() && layout.pullout_anchor.is_none() {
                    layout.pullout_anchor = previous.pullout_anchor;
                }
                layout.windows.push(window);
            }
            for restart in &persistence.original_restarts {
                if restart
                    .windows
                    .iter()
                    .all(|id| pending_ids.contains_key(id))
                {
                    let mut restart = restart.clone();
                    restart.windows = restart.windows.iter().map(|id| pending_ids[id]).collect();
                    extra_restarts.push(restart);
                }
            }
        }
        Save {
            layout,
            clients,
            extra_restarts,
            done: None,
        }
    }
    pub fn session_interacted(&mut self) {
        self.restore_stacking = false;
        if let Some(persistence) = &mut self.session_persistence {
            persistence.interacted = true;
            persistence.closing = false;
        }
    }
    pub fn update_session_restore(&mut self) {
        let Some(mut persistence) = self.session_persistence.take() else {
            return;
        };
        let now = Instant::now();
        if now < persistence.next_tick || persistence.closing {
            self.session_persistence = Some(persistence);
            return;
        }
        persistence.next_tick = now + Duration::from_millis(100);
        persistence
            .process_windows
            .extend(persistence.reopened.try_iter());
        if let Some(layout) = &persistence.layout {
            let mut scene = self.scene.lock().unwrap();
            let live = scene.windows.clone();
            for window in &live {
                if persistence.restored.values().any(|id| *id == window.id) {
                    continue;
                }
                if !layout.windows.iter().any(|saved| {
                    !persistence.restored.contains_key(&saved.id)
                        && saved.app_id == window.app_id
                        && saved.origin == window.origin
                }) {
                    continue;
                }
                let mut pid = self
                    .registry
                    .iter()
                    .find(|(id, _)| *id == window.id)
                    .and_then(|(_, window)| window.toplevel().unwrap().wl_surface().client())
                    .and_then(|client| client.get_credentials(&self.display_handle).ok())
                    .map(|credentials| credentials.pid as u32);
                let mut group = None;
                for _ in 0..4 {
                    let Some(current) = pid else {
                        break;
                    };
                    if let Some(windows) = persistence.process_windows.get(&current) {
                        group = Some(windows);
                        break;
                    }
                    pid = parent(current).filter(|next| *next != current && *next > 1);
                }
                let candidates: Vec<_> = layout
                    .windows
                    .iter()
                    .filter(|saved| {
                        !persistence.restored.contains_key(&saved.id)
                            && saved.app_id == window.app_id
                            && saved.origin == window.origin
                            && group.is_none_or(|group| group.contains(&saved.id))
                    })
                    .collect();
                let saved = candidates
                    .iter()
                    .find(|saved| saved.title == window.title)
                    .copied()
                    .or_else(|| {
                        (persistence.started.elapsed() > Duration::from_secs(2))
                            .then(|| candidates.first().copied())
                            .flatten()
                    });
                if let Some(saved) = saved {
                    let mut placement = saved.clone();
                    if placement.desktop_visible {
                        let r = &placement.desktop;
                        let output = self.output_geometry_at(
                            ((r.x + r.w / 2.) as f64, (r.y + r.h / 2.) as f64).into(),
                        );
                        let (left, top, width, height) = (
                            output.loc.x as f32,
                            output.loc.y as f32 + 36.,
                            output.size.w as f32,
                            (output.size.h - 96).max(100) as f32,
                        );
                        let fit = |r: &mut dome::Rect| {
                            if r.x + r.w < left + 32.
                                || r.x > left + width - 32.
                                || r.y < top - 36.
                                || r.y > top + height - 32.
                            {
                                r.w = r.w.min(width).max(100.);
                                r.h = r.h.min(height).max(100.);
                                r.x = r.x.clamp(left, left + (width - r.w).max(0.));
                                r.y = r.y.clamp(top, top + (height - r.h).max(0.));
                            }
                        };
                        fit(&mut placement.desktop);
                        if let Some(restore) = &mut placement.restore {
                            fit(restore);
                        }
                    }
                    let rotations: Vec<_> = scene
                        .workspaces
                        .iter()
                        .map(|w| w.lanes.each_ref().map(|lane| lane.rotation_deg))
                        .collect();
                    persistence.restored.insert(saved.id, window.id);
                    if !self.xr_bridge.lock().unwrap().visible() && !placement.desktop_visible {
                        placement.desktop_visible = true;
                    }
                    layout.place(&mut scene, &placement, window.id, &persistence.restored);
                    if !persistence.interacted {
                        self.restore_stacking = true;
                    }
                    if placement.origin == Realm::Xr && !self.input_xr_active {
                        self.focus_history[1] = Some(Some(window.id));
                        if !placement.desktop_visible && scene.focused == Some(window.id) {
                            scene.focused = scene
                                .windows
                                .iter()
                                .rev()
                                .find(|w| {
                                    w.desktop_visible
                                        && !w.minimized
                                        && w.workspace == scene.active_workspace
                                })
                                .map(|w| w.id);
                        }
                    }
                    if persistence.interacted {
                        for (workspace, angles) in scene.workspaces.iter_mut().zip(rotations) {
                            for (lane, angle) in workspace.lanes.iter_mut().zip(angles) {
                                lane.rotation_deg = angle;
                            }
                        }
                    }
                }
            }
            let complete = persistence.restored.len() == layout.windows.len();
            if !persistence.focus_restored {
                if !persistence.interacted {
                    let target = layout
                        .focused
                        .and_then(|id| persistence.restored.get(&id).copied());
                    let available = layout.focused.is_none()
                        || target.is_some_and(|id| {
                            scene
                                .window(id)
                                .is_some_and(|w| w.visible_in(self.input_realm))
                        });
                    if available {
                        self.restore_stacking = true;
                    }
                    if available && scene.focused != target {
                        self.restore_stacking = true;
                        scene.focused = target;
                        scene.revision += 1;
                    }
                }
                let applications_ready = layout
                    .windows
                    .iter()
                    .filter(|w| w.app_id != "org.qimgvr.Sbs")
                    .all(|w| persistence.restored.contains_key(&w.id));
                if (applications_ready && persistence.started.elapsed() >= Duration::from_secs(2))
                    || persistence.started.elapsed() > Duration::from_secs(30)
                {
                    persistence.focus_restored = true;
                }
            }
            if complete && persistence.focus_restored {
                persistence.layout = None;
            }
        }
        if (persistence.layout.is_none() || persistence.started.elapsed() > Duration::from_secs(30))
            && now.duration_since(persistence.last_save) >= Duration::from_secs(1)
        {
            persistence.last_save = now;
            let save = self.session_snapshot(&persistence);
            if let Ok(fingerprint) = serde_json::to_vec(&save.layout) {
                if fingerprint != persistence.fingerprint && persistence.send.try_send(save).is_ok()
                {
                    persistence.fingerprint = fingerprint;
                }
            }
        }
        self.session_persistence = Some(persistence);
    }
    pub fn save_session_on_exit(&mut self) {
        let Some(persistence) = &self.session_persistence else {
            return;
        };
        if persistence.closing {
            return;
        }
        let (done, receiver) = mpsc::channel();
        let mut save = self.session_snapshot(persistence);
        save.done = Some(done);
        if persistence.send.send(save).is_ok() {
            match receiver.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(())) => (),
                Ok(Err(error)) => eprintln!("Session save failed: {error}"),
                Err(error) => eprintln!("Session save did not finish: {error}"),
            }
        }
        if let Some(persistence) = &mut self.session_persistence {
            persistence.closing = true;
        }
    }
}
