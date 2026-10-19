use std::time::Duration;

use smithay::{
    backend::{
        renderer::damage::OutputDamageTracker,
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Transform},
};

use crate::{CalloopData, Dome};

pub fn init_winit(
    event_loop: &mut EventLoop<CalloopData>,
    data: &mut CalloopData,
) -> Result<(), Box<dyn std::error::Error>> {
    let display_handle = &mut data.display_handle;
    let state = &mut data.state;

    let (backend, winit) = winit::init()?;

    backend
        .window()
        .set_title("Dome — experimental Wayland compositor");
    if std::env::var("DOME_LOGIN_SESSION").as_deref() == Ok("1") {
        backend.window().set_decorations(false);
        backend.window().set_fullscreen(Some(
            smithay::reexports::winit::window::Fullscreen::Borderless(None),
        ));
    }
    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };

    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Smithay".into(),
            model: "Winit".into(),
        },
    );
    let _global = output.create_global::<Dome>(display_handle);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));

    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    crate::activate_client_socket(state);

    let backend = std::rc::Rc::new(std::cell::RefCell::new(backend));
    let tick = backend.clone();
    event_loop.handle().insert_source(
        smithay::reexports::calloop::timer::Timer::from_duration(Duration::from_millis(16)),
        move |_, _, _| {
            tick.borrow().window().request_redraw();
            smithay::reexports::calloop::timer::TimeoutAction::ToDuration(Duration::from_millis(16))
        },
    )?;
    let mut desktop = crate::desktop::DesktopRenderer::default();
    event_loop
        .handle()
        .insert_source(winit, move |event, _, data| {
            let mut backend = backend.borrow_mut();
            let display = &mut data.display_handle;
            let state = &mut data.state;

            match event {
                WinitEvent::Resized { size, .. } => {
                    state.applied_revision = u64::MAX;
                    output.change_current_state(
                        Some(Mode {
                            size,
                            refresh: 60_000,
                        }),
                        None,
                        None,
                        None,
                    );
                }
                WinitEvent::Input(event) => state.process_input_event(event),
                WinitEvent::Redraw => {
                    let size = backend.window_size();
                    desktop.prepare(state, size);
                    {
                        let (renderer, mut framebuffer) = backend.bind().unwrap();
                        let _sync = desktop
                            .render(
                                state,
                                renderer,
                                &mut framebuffer,
                                &mut damage_tracker,
                                0,
                                false,
                            )
                            .unwrap();
                    }
                    backend.submit(Some(&[Rectangle::from_size(size)])).unwrap();
                    crate::desktop::finish_frame(state, &output);
                    let _ = display.flush_clients();
                }
                WinitEvent::CloseRequested => {
                    state.loop_signal.stop();
                }
                _ => (),
            };
        })?;

    Ok(())
}
