use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker, gles::GlesTexture, Bind, ExportMem, Frame, Offscreen,
            Renderer,
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

    // The host window may be Xvfb or a desktop without Vulkan WSI. Composition
    // still runs through Vulkan; a separate host context presents the finished image.
    let (mut backend, winit) = winit::init()?;
    let mut vulkan = if crate::rendering::selection() == "vulkan" {
        Some(crate::rendering::offscreen()?)
    } else {
        crate::rendering::verify(backend.renderer())?;
        None
    };
    let mut vulkan_target: Option<(
        smithay::utils::Size<i32, smithay::utils::Physical>,
        GlesTexture,
    )> = None;

    backend
        .window()
        .set_title("Dome — experimental Wayland compositor");
    if std::env::var("DOME_LOGIN_SESSION").as_deref() == Ok("1") {
        backend.window().set_decorations(false);
        backend.window().set_fullscreen(Some(
            smithay::reexports::winit::window::Fullscreen::Borderless(None),
        ));
    }
    // Render the same DomeD cursor as native outputs; never show a second host cursor.
    backend.window().set_cursor_visible(false);
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
    let mut present_damage = OutputDamageTracker::from_output(&output);

    let renderer = vulkan.as_mut().unwrap_or_else(|| backend.renderer());
    if let Err(error) = crate::rendering::advertise_dmabuf(state, renderer) {
        eprintln!("Nested client DMA-BUF unavailable: {error}; retaining shared-memory clients");
    }

    crate::activate_client_socket(state);

    let backend = std::rc::Rc::new(std::cell::RefCell::new(backend));
    let vulkan = std::rc::Rc::new(std::cell::RefCell::new(vulkan));
    let tick_renderer = vulkan.clone();
    let tick = backend.clone();
    event_loop.handle().insert_source(
        smithay::reexports::calloop::timer::Timer::from_duration(Duration::from_millis(16)),
        move |deadline, _, data| {
            if let Some(renderer) = tick_renderer.borrow_mut().as_mut() {
                data.state.tick_xr(renderer);
            } else {
                data.state.tick_xr(tick.borrow_mut().renderer());
            }
            tick.borrow().window().request_redraw();
            smithay::reexports::calloop::timer::TimeoutAction::ToInstant(
                dome::xr_transport::next_frame_deadline(deadline, std::time::Instant::now(), data.state.xr_tick_interval()))
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
                    let mut vulkan = vulkan.borrow_mut();
                    if let Some(compositor) = vulkan.as_mut() {
                        if vulkan_target.as_ref().is_none_or(|(old, _)| *old != size) {
                            vulkan_target = Some((
                                size,
                                compositor
                                    .create_buffer(
                                        smithay::backend::allocator::Fourcc::Abgr8888,
                                        (size.w, size.h).into(),
                                    )
                                    .unwrap(),
                            ));
                        }
                        let mut target = compositor
                            .bind(&mut vulkan_target.as_mut().unwrap().1)
                            .unwrap();
                        let sync = desktop
                            .render(
                                state,
                                compositor,
                                &mut target,
                                &mut damage_tracker,
                                0,
                            )
                            .unwrap();
                        sync.wait().unwrap();
                        if let Some(path) = std::env::var_os("DOME_RENDER_CAPTURE") {
                            crate::rendering::capture(
                                compositor,
                                &target,
                                size,
                                std::path::Path::new(&path),
                            )
                            .unwrap();
                        }
                        let mapping = compositor
                            .copy_framebuffer(
                                &target,
                                Rectangle::from_size((size.w, size.h).into()),
                                smithay::backend::allocator::Fourcc::Abgr8888,
                            )
                            .unwrap();
                        let pixels = compositor.map_texture(&mapping).unwrap().to_vec();
                        drop(mapping);
                        drop(target);
                        let (presenter, mut framebuffer) = backend.bind().unwrap();
                        use smithay::backend::renderer::element::{
                            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                            Kind,
                        };
                        let stride = size.w as usize * 4;
                        let mut top_down = Vec::with_capacity(pixels.len());
                        for row in pixels.chunks_exact(stride).rev() {
                            top_down.extend_from_slice(row);
                        }
                        let buffer = MemoryRenderBuffer::from_slice(
                            &top_down,
                            smithay::backend::allocator::Fourcc::Abgr8888,
                            (size.w, size.h),
                            1,
                            Transform::Normal,
                            None,
                        );
                        let element = MemoryRenderBufferRenderElement::from_buffer(
                            presenter,
                            (0., 0.),
                            &buffer,
                            None,
                            None,
                            None,
                            Kind::Unspecified,
                        )
                        .unwrap();
                        present_damage
                            .render_output(
                                presenter,
                                &mut framebuffer,
                                0,
                                &[element],
                                [0., 0., 0., 1.],
                            )
                            .unwrap()
                            .sync
                            .wait()
                            .unwrap();
                    } else {
                        let (renderer, mut framebuffer) = backend.bind().unwrap();
                        let sync = desktop
                            .render(
                                state,
                                renderer,
                                &mut framebuffer,
                                &mut damage_tracker,
                                0,
                            )
                            .unwrap();
                        if let Some(path) = std::env::var_os("DOME_RENDER_CAPTURE") {
                            sync.wait().unwrap();
                            crate::rendering::capture(
                                renderer,
                                &framebuffer,
                                size,
                                std::path::Path::new(&path),
                            )
                            .unwrap();
                            // Mapping readback makes the context surfaceless; restore it before swap.
                            renderer
                                .render(&mut framebuffer, size, Transform::Flipped180)
                                .unwrap()
                                .finish()
                                .unwrap()
                                .wait()
                                .unwrap();
                        }
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
