use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::WaylandSurfaceRenderElement,
                AsRenderElements, Kind,
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

smithay::backend::renderer::element::render_elements! {
    DesktopElement<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Chrome=MemoryRenderBufferRenderElement<GlesRenderer>,
}

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
    let mut decoration_buffers: std::collections::HashMap<u64, (String, MemoryRenderBuffer)> =
        Default::default();
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
                    state.xr_input();
                    state.sync_scene();
                    let size = backend.window_size();
                    let damage = Rectangle::from_size(size);

                    let key = format!(
                        "{:?}:{}:{}:{}:{:?}:{:?}:{}:{}",

                        (state.applied_revision,state.appearance_open,state.session_confirm,format!("{:?}:{:?}",state.appearance,state.session.state()),state.about,state.task_popup,state.task_context,state.pins.clone(),state.tray.state().revision,if state.task_popup.is_some(){state.start_time.elapsed().as_millis()/100}else{0}),
                        state.launcher,
                        state.desktop_manager,
                        state.launch_query,
                        size,state.menu,state.menu_hover,std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()/60
                    );
                    if shell_buffer.as_ref().is_none_or(|(old, _)| old != &key) {
                        let realm = state.input_realm;
                        state.input_realm = dome::Realm::Xr;
                        let xr_pixels = state.shell_pixels(size.w, size.h);
                        state.export_xr_shell(&xr_pixels,size.w,size.h);
                        state.input_realm = dome::Realm::Desktop;
                        let pixels = state.shell_pixels(size.w, size.h);
                        state.input_realm = realm;
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
                    {let mut bridge=state.xr_bridge.lock().unwrap();for (id,f)in &mut bridge.frames{if dome::xr_transport::is_shell(*id){f.stamp=std::time::Instant::now();}}}
                    {
                        let (renderer, mut framebuffer) = backend.bind().unwrap();
                        state.export_xr_surface(renderer);
                        let shell = MemoryRenderBufferRenderElement::from_buffer(renderer,(0.,0.),&shell_buffer.as_ref().unwrap().1,None,None,None,Kind::Unspecified).unwrap();
                        let scene=state.scene.lock().unwrap().clone();
                        decoration_buffers.retain(|id,_|scene.window(*id).is_some());
                        let mut elements:Vec<DesktopElement>=vec![shell.into()];
                        for window in state.space.elements().rev(){
                            let Some(id)=state.id_for(window.toplevel().unwrap().wl_surface())else{continue;};
                            let Some(w)=scene.window(id)else{continue;};let Some(pos)=state.space.element_location(window)else{continue;};let geo=window.geometry();
                            elements.extend(window.render_elements::<DesktopElement>(renderer,(pos.x-geo.loc.x,pos.y-geo.loc.y).into(),1.0.into(),1.));
                            if w.maximized{continue;}
                            let dw=geo.size.w+8;let dh=geo.size.h+34;
                            if dw<8||dh<34||dw>8192||dh>8192{continue;}
                            let key=format!("{dw}:{dh}:{}:{}",w.title,scene.focused==Some(id));
                            if decoration_buffers.get(&id).is_none_or(|(old,_)|old!=&key){let bytes=crate::qt::paint(serde_json::json!({"kind":"frame","width":dw,"height":dh,"title":w.title,"active":scene.focused==Some(id)}));decoration_buffers.insert(id,(key,MemoryRenderBuffer::from_slice(&bytes,smithay::backend::allocator::Fourcc::Argb8888,(dw,dh),1,Transform::Normal,None)));}
                            let deco=MemoryRenderBufferRenderElement::from_buffer(renderer,((pos.x-4)as f64,(pos.y-30)as f64),&decoration_buffers[&id].1,None,None,None,Kind::Unspecified).unwrap();elements.push(deco.into());
                        }
                        damage_tracker.render_output(renderer,&mut framebuffer,0,&elements,[state.appearance.background[0],state.appearance.background[1],state.appearance.background[2],1.]).unwrap();
                    }
                    backend.submit(Some(&[damage])).unwrap();

                    state.registry.iter().map(|(_, w)| w).for_each(|window| {
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
