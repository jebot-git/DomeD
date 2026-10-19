#![allow(irrefutable_let_patterns)]

mod handlers;

mod grabs;
mod input;
mod shell;
mod state;
mod winit;

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
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }

    let mut event_loop: EventLoop<CalloopData> = EventLoop::try_new()?;

    let display: Display<Dome> = Display::new()?;
    let display_handle = display.handle();
    let state = Dome::new(&mut event_loop, display);
    dome::load_geometry(&mut state.scene.lock().unwrap())?;

    let mut data = CalloopData {
        state,
        display_handle,
    };

    crate::winit::init_winit(&mut event_loop, &mut data)?;

    let port = std::env::var("DOME_PREVIEW_PORT")
        .unwrap_or_else(|_| "49640".into())
        .parse()?;
    let _service =
        dome::service::serve(data.state.scene.clone(), port).map_err(|e| e.to_string())?;
    eprintln!(
        "Dome Wayland socket: {}",
        data.state.socket_name.to_string_lossy()
    );
    eprintln!("Control and spatial preview: http://127.0.0.1:{port}");
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--command" || s == "-c") {
        if let Some(program) = args.get(1) {
            std::process::Command::new(program)
                .args(&args[2..])
                .spawn()?;
        }
    }

    event_loop.run(None, &mut data, move |_| {
        // Dome is running
    })?;

    Ok(())
}
