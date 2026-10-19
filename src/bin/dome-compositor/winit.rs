use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                Kind,
            },
            gles::GlesRenderer,
        },
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

    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);

    let backend = std::rc::Rc::new(std::cell::RefCell::new(backend));
    let tick = backend.clone();
    event_loop.handle().insert_source(
        smithay::reexports::calloop::timer::Timer::from_duration(Duration::from_millis(16)),
        move |_, _, _| {
            tick.borrow().window().request_redraw();
            smithay::reexports::calloop::timer::TimeoutAction::ToDuration(Duration::from_millis(16))
        },
    )?;
    let mut shell_buffer: Option<(String, MemoryRenderBuffer)> = None;
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
                    state.sync_scene();
                    let size = backend.window_size();
                    let damage = Rectangle::from_size(size);

                    let key = format!(
                        "{}:{}:{}:{}:{:?}",
                        state.applied_revision,
                        state.launcher,
                        state.desktop_manager,
                        state.launch_query,
                        size
                    );
                    if shell_buffer.as_ref().is_none_or(|(old, _)| old != &key) {
                        let pixels = state.shell_pixels(size.w, size.h);
                        shell_buffer = Some((
                            key,
                            MemoryRenderBuffer::from_slice(
                                &pixels,
                                smithay::backend::allocator::Fourcc::Argb8888,
                                (size.w, size.h),
                                1,
                                Transform::Normal,
                                None,
                            ),
                        ));
                    }
                    {
                        let (renderer, mut framebuffer) = backend.bind().unwrap();
                        let shell = MemoryRenderBufferRenderElement::from_buffer(
                            renderer,
                            (0., 0.),
                            &shell_buffer.as_ref().unwrap().1,
                            None,
                            None,
                            None,
                            Kind::Unspecified,
                        )
                        .unwrap();
                        smithay::desktop::space::render_output::<
                            _,
                            MemoryRenderBufferRenderElement<GlesRenderer>,
                            _,
                            _,
                        >(
                            &output,
                            renderer,
                            &mut framebuffer,
                            1.0,
                            0,
                            [&state.space],
                            &[shell],
                            &mut damage_tracker,
                            [0.025, 0.045, 0.06, 1.0],
                        )
                        .unwrap();
                    }
                    backend.submit(Some(&[damage])).unwrap();

                    state.space.elements().for_each(|window| {
                        window.send_frame(
                            &output,
                            state.start_time.elapsed(),
                            Some(Duration::ZERO),
                            |_, _| Some(output.clone()),
                        )
                    });

                    state.space.refresh();
                    state.popups.cleanup();
                    let _ = display.flush_clients();

                    // The 60 Hz calloop timer schedules frames without a busy redraw loop.
                }
                WinitEvent::CloseRequested => {
                    state.loop_signal.stop();
                }
                _ => (),
            };
        })?;

    Ok(())
}
