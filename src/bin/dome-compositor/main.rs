#![allow(irrefutable_let_patterns)]

mod appmenu;
mod file_manager;
mod global_menu;
mod handlers;
mod rendering;
mod screencast;
mod screenshot;
mod selection_tooltips;

mod audio;
mod clock;
mod default_apps;
mod clipboard;
mod commands;
mod context_menus;
mod decorations;
mod desktop;
#[cfg(feature = "standalone")]
mod drm;
mod grabs;
mod histories;
mod ime;
mod input;
mod input_routing;
mod keyboard;
mod launcher;
mod x11_bridge;
mod menu_navigation;
mod notifications;
mod notification_windows;
mod outputs;
mod qt;
mod session;
mod session_restore;
mod session_services;
mod gtk_menu;
mod shell;
mod snapping;
mod spatial_popups;
mod state;
mod tasks;
mod task_previews;
mod toolkit_appearance;
mod tray;
mod winit;
mod xr_bridge;
mod xr_refresh;
mod xr_gpu;
mod xr_settings;

use smithay::reexports::{
    calloop::EventLoop,
    wayland_server::{Display, DisplayHandle},
};
pub use state::Dome;

pub struct CalloopData {
    state: Dome,
    display_handle: DisplayHandle,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--refresh-session-fonts"] {
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is required")?);
        let configuration = toolkit_appearance::refresh_session_fonts(&runtime)?;
        println!("Refreshed session bitmap fonts; FONTCONFIG_FILE={}", configuration.display());
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "--export-fonts") {
        if args.len() != 2 {
            return Err("Usage: dome-compositor --export-fonts DIRECTORY".into());
        }
        let requested = std::path::PathBuf::from(&args[1]);
        let root = if requested.is_absolute() { requested } else { std::env::current_dir()?.join(requested) };
        let configuration = toolkit_appearance::write_fonts(&root)?;
        println!("Exported {} bitmap font files; FONTCONFIG_FILE={}",
            dome::bundled_fonts::FONTS.len(), configuration.display());
        return Ok(());
    }
    let mut backend = std::env::var("DOME_BACKEND").unwrap_or_else(|_| {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
            "nested"
        } else {
            "drm"
        }
        .into()
    });
    while args
        .first()
        .is_some_and(|a| matches!(a.as_str(), "--backend" | "--renderer"))
    {
        let option = args[0].clone();
        let value = args
            .get(1)
            .ok_or_else(|| format!("{option} requires a value"))?
            .clone();
        if option == "--backend" {
            backend = value;
        } else {
            std::env::set_var("DOME_RENDERER", value);
        }
        args.drain(..2);
    }
    if !matches!(rendering::selection().as_str(), "vulkan" | "gles") {
        return Err("DOME_RENDERER must be vulkan or gles".into());
    }
    if args.first().is_some_and(|a| a == "--help" || a == "-h") {
        println!("dome-compositor [--backend drm|nested] [--renderer vulkan|gles] [--terminal | --command PROGRAM ARGS...]\n  drm: native libseat/libinput/DRM compositor (login screen or unused TTY)\n  nested: development window in an existing Wayland/X11 desktop\n  --renderer: Vulkan via Mesa Zink (default), or native GLES\n  --refresh-session-fonts: update stable and legacy session font exports\n  --export-fonts DIRECTORY: refresh bitmap assets and Fontconfig configuration without starting a session\n  --check-backend: report backend support without opening a seat or display");
        return Ok(());
    }
    if !matches!(backend.as_str(), "drm" | "nested") {
        return Err(format!("Unknown backend: {backend}").into());
    }
    if args.first().is_some_and(|a| a == "--check-backend") {
        if backend == "drm" && !cfg!(feature = "standalone") {
            return Err("Native DRM support is not compiled in".into());
        }
        println!(
            "DomeD backend: {backend}; native DRM compiled: {}; renderer: {}",
            cfg!(feature = "standalone"),
            rendering::selection()
        );
        println!("Multi-GPU compiled: {}; build DRM device: {}",
            cfg!(feature = "multi-gpu"),
            option_env!("DOME_BUILD_DRM_DEVICE").unwrap_or("automatic"));
        return Ok(());
    }
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }

    let mut event_loop: EventLoop<CalloopData> = EventLoop::try_new()?;
    // Register before creating worker threads so they inherit the blocked mask.
    #[cfg(feature = "standalone")]
    {
        use smithay::reexports::calloop::signals::{Signal, Signals};
        event_loop.handle().insert_source(
            Signals::new(&[Signal::SIGTERM, Signal::SIGINT, Signal::SIGHUP])?,
            |_, _, data| {
                data.state.loop_signal.stop();
            },
        )?;
    }

    // Initialize our offscreen Qt chrome before exporting the client toolkit environment.
    crate::qt::bitmap_fonts();
    let mut appearance = dome::appearance::Appearance::load();
    crate::qt::normalize_appearance(&mut appearance);
    let _ = crate::qt::resolved_font(&appearance);
    let toolkit_appearance = toolkit_appearance::ToolkitAppearance::start(&appearance)
        .unwrap_or_else(|error| { eprintln!("Toolkit appearance service: {error}"); None });
    let display: Display<Dome> = Display::new()?;
    let display_handle = display.handle();
    let mut state = Dome::new(&mut event_loop, display);
    state.appearance = appearance;
    state.toolkit_appearance = toolkit_appearance;
    event_loop.handle().insert_source(
        smithay::reexports::calloop::timer::Timer::from_duration(std::time::Duration::from_millis(20)),
        |_, _, data| {
            data.state.repeat_launcher_key();
            data.state.repeat_menu_scroll();
            data.state.update_session_restore();
            smithay::reexports::calloop::timer::TimeoutAction::ToDuration(std::time::Duration::from_millis(20))
        },
    )?;
    dome::load_geometry(&mut state.scene.lock().unwrap())?;
    if std::env::var_os("DOME_CONFIG").is_none() {
        if let Some(config) = dome::geometry_settings::load(&dome::geometry_settings::path())? {
            state.scene.lock().unwrap().config = config;
        }
    }
    state.geometry_saved = state.scene.lock().unwrap().config.clone();

    let mut data = CalloopData {
        state,
        display_handle,
    };

    let screencast = screencast::start()?;
    match backend.as_str() {
        "nested" => crate::winit::init_winit(&mut event_loop, &mut data)?,
        "drm" => {
            #[cfg(feature = "standalone")]
            crate::drm::init(&mut event_loop, &mut data)?;
            #[cfg(not(feature = "standalone"))]
            return Err("Rebuild with --features standalone for native DRM support".into());
        }
        _ => unreachable!(),
    }

    data.state.xr_bridge.lock().unwrap().xr_socket =
        data.state.xr_socket_name.to_string_lossy().into_owned();
    eprintln!(
        "Dome XR Wayland socket: {}",
        data.state.xr_socket_name.to_string_lossy()
    );
    let _file_manager = file_manager::start(
        data.state.socket_name.to_string_lossy().into_owned(),
        data.state.xr_socket_name.to_string_lossy().into_owned(),
        data.state.xr_bridge.clone(),
    );
    let port = std::env::var("DOME_PREVIEW_PORT")
        .unwrap_or_else(|_| "49640".into())
        .parse()?;
    data.state.control_port = port;
    let _service = dome::service::serve_native(
        data.state.scene.clone(),
        port,
        Some(data.state.xr_bridge.clone()),
    )
    .map_err(|e| e.to_string())?;
    let _gpu_service = dome::xr_gpu::serve(data.state.xr_bridge.clone(), port)
        .map_err(|e| eprintln!("XR GPU transport unavailable: {e}")).ok();
    // Publish only after the loopback service has bound successfully. Older
    // readers still recognize the version-1 per-socket launch realm.
    let _launch_contexts = [
        dome::launch_context::Registration::publish_with_service(
            &data.state.socket_name,
            dome::Realm::Desktop,
            Some(port),
            Some(data.state.xr_socket_name.to_string_lossy().into_owned()),
        )?,
        dome::launch_context::Registration::publish_with_service(
            &data.state.xr_socket_name,
            dome::Realm::Xr,
            Some(port),
            Some(data.state.xr_socket_name.to_string_lossy().into_owned()),
        )?,
    ];
    eprintln!(
        "Dome Wayland socket: {}",
        data.state.socket_name.to_string_lossy()
    );
    eprintln!("Control and spatial preview: http://127.0.0.1:{port}");
    screencast::update(&data.state, &screencast);
    data.state.start_session_restore();
    if args.first().is_some_and(|s| s == "--terminal") {
        dome::terminal::startup(
            &data.state.socket_name.to_string_lossy(),
            if std::env::var("DOME_START_XR").as_deref() == Ok("1") {
                dome::Realm::Xr
            } else {
                dome::Realm::Desktop
            },
        )?;
    } else if args.first().is_some_and(|s| s == "--command" || s == "-c") {
        if let Some(program) = args.get(1) {
            std::process::Command::new(program)
                .args(&args[2..])
                .spawn()?;
        }
    }
    if std::env::var("DOME_START_XR").as_deref() == Ok("1") {
        data.state.start_xr();
    }

    event_loop.run(None, &mut data, move |data| {
        screencast::update(&data.state, &screencast);
    })?;

    data.state.save_session_on_exit();
    if let Some(mut child) = data.state.xwayland_child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    data.state.xr_bridge.lock().unwrap().exit_requested = true;
    if let Some(mut child) = data.state.xr_child.take() {
        if child.try_wait()?.is_none() {
            child.kill()?;
        }
        let _ = child.wait();
    }
    Ok(())
}

fn activate_client_socket(state: &mut Dome) {
    dome::askpass::configure();
    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
    if std::env::var("DOME_LOGIN_SESSION").as_deref() == Ok("1") {
        // An Xwayland process attached to another compositor cannot serve this
        // login. Keep our bridge in the session's process group for logout.
        if std::env::var("DOME_START_XWAYLAND").as_deref() != Ok("0") {
            if let Some(program) = dome::desktop_entries::find_executable("xwayland-satellite") {
                if let Some(number) = (100..200).find(|n| {
                    !std::path::Path::new(&format!("/tmp/.X{n}-lock")).exists()
                        && !std::path::Path::new(&format!("/tmp/.X11-unix/X{n}")).exists()
                }) {
                    let display = format!(":{number}");
                    match std::process::Command::new(program)
                        .arg(&display)
                        .env("WAYLAND_DISPLAY", &state.x11_socket_name)
                        .env_remove("WAYLAND_SOCKET")
                        .spawn() {
                        Ok(child) => {
                            eprintln!("Dome Xwayland bridge: {display}");
                            std::env::set_var("DISPLAY", display);
                            state.xwayland_child = Some(child);
                        }
                        Err(error) => eprintln!("Dome Xwayland startup: {error}"),
                    }
                }
            } else {
                eprintln!("X11 apps require xwayland-satellite and Xwayland; bridge is not installed");
            }
        }
        gtk_menu::enable_legacy_module();
        std::env::set_var("QT_IM_MODULE", "wayland");
        std::env::set_var("GTK_IM_MODULE", "fcitx");
        std::env::set_var("XMODIFIERS", "@im=fcitx");
    }
    state.ime.start(&state.display_handle);
    if std::env::var("DOME_LOGIN_SESSION").as_deref() == Ok("1") || state.toolkit_appearance.is_some() {
        let _ = std::process::Command::new("dbus-update-activation-environment")
            .args([
                "--systemd",
                "WAYLAND_DISPLAY",
                "SUDO_ASKPASS",
                "XDG_CURRENT_DESKTOP",
                "XDG_SESSION_DESKTOP",
                "XDG_SESSION_TYPE",
                "QT_QPA_PLATFORM",
                "QT_IM_MODULE",
                "GTK_IM_MODULE",
                "XMODIFIERS",
                "GTK_MODULES",
                "GTK_USE_PORTAL",
                "FONTCONFIG_FILE",
                "XDG_DATA_DIRS",
                "QT_QPA_PLATFORMTHEME",
                "QT_STYLE_OVERRIDE",
                "QT_QUICK_CONTROLS_STYLE",
                "GTK_THEME=",
                "UBUNTU_MENUPROXY",
                "LD_LIBRARY_PATH",
                "DISPLAY",
            ])
            .status();
    }
    if std::env::var("DOME_LOGIN_SESSION").as_deref() == Ok("1") {
        // Fedora's portal has Requisite=graphical-session.target. Publish the
        // ready display before starting our dependency owner: distributions
        // can forbid direct starts of graphical-session.target.
        let status = std::process::Command::new("systemctl")
            .args(["--user", "start", "dome-session.target"]).status();
        if !status.is_ok_and(|s| s.success()) { eprintln!("Could not start dome-session.target"); }
        session_services::start();
    }
}
