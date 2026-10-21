//! Native seat, DRM/KMS scanout, Vulkan-backed Zink or GLES rendering and libinput.
use crate::{desktop::DesktopRenderer, CalloopData, Dome};
use smithay::{
    backend::{
        allocator::{
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc,
        },
        drm::{DrmDevice, DrmDeviceFd, DrmEvent, GbmBufferedSurface},
        egl::{EGLContext, EGLDisplay},
        input::{InputEvent, KeyState, KeyboardKeyEvent},
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            damage::OutputDamageTracker,
            gles::{GlesRenderer, GlesTexture},
            Bind, ExportMem, Offscreen,
        },
        session::{libseat::LibSeatSession, Event as SessionEvent, Session},
        udev::{UdevBackend, UdevEvent},
    },
    output::{Mode as OutputMode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            timer::{TimeoutAction, Timer},
            EventLoop, RegistrationToken,
        },
        drm::{
            control::{self, Device as ControlDevice},
            Device as BasicDevice,
        },
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::backend::GlobalId,
    },
    utils::{DeviceFd, Physical, Size, Transform},
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;
type Scanout = GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, ()>;

struct NativeOutput {
    scanout: Option<Scanout>,
    offscreen: Option<GlesTexture>,
    output: Output,
    global: GlobalId,
    damage: OutputDamageTracker,
    size: Size<i32, Physical>,
    connector: Option<control::connector::Handle>,
    pending: bool,
    desktop: DesktopRenderer,
    rendered_frames: u64,
    presented_frames: u64,
}
struct Native {
    drm: DrmDevice,
    gbm: GbmDevice<DrmDeviceFd>,
    renderer: GlesRenderer,
    renderer_name: String,
    outputs: Vec<NativeOutput>,
    device_id: u64,
    path: PathBuf,
    token: Option<RegistrationToken>,
    active: bool,
    retry_after: Instant,
    capture: Option<PathBuf>,
    report: Option<PathBuf>,
    rendered_frames: u64,
    presented_frames: u64,
    dmabuf_imports: u64,
}
impl Drop for Native {
    fn drop(&mut self) {
        // We own a login/VT, not another compositor's live KMS state. Clear our
        // scanout and release master rather than restoring expired FB/blob IDs.
        if self.drm.is_active() {
            let _ = self.drm.reset_state();
            self.drm.pause();
        }
        self.outputs.clear();
    }
}

fn connector_name(info: &control::connector::Info) -> String {
    format!("{}-{}", info.interface().as_str(), info.interface_id())
}
impl Native {
    fn refresh_output(&mut self, state: &mut Dome) -> Result<(), Error> {
        let wanted = std::env::var("DOME_DRM_CONNECTOR").ok();
        let config = state.scene.lock().unwrap().config.clone();
        let resources = self.drm.resource_handles()?;
        let mut connected = Vec::new();
        for handle in resources.connectors() {
            let info = self.drm.get_connector(*handle, true)?;
            if info.state() != control::connector::State::Connected
                || wanted.as_ref().is_some_and(|w| *w != connector_name(&info))
            {
                continue;
            }
            let name = output_name(&self.path, &info);
            let mut modes: Vec<_> = info.modes().iter().copied().map(OutputMode::from).collect();
            modes.sort_by_key(|m| (m.size.w, m.size.h, m.refresh));
            modes.dedup();
            state.display_modes.insert(name.clone(), modes);
            let profile = config.displays.iter().find(|d| d.name == name);
            if profile.is_some_and(|d| !d.enabled) {
                continue;
            }
            let Some(mode) = info
                .modes()
                .iter()
                .max_by_key(|m| {
                    (
                        profile.is_some_and(|d| {
                            d.width == m.size().0 as i32
                                && d.height == m.size().1 as i32
                                && (d.refresh == 0 || d.refresh == OutputMode::from(**m).refresh)
                        }),
                        m.mode_type().contains(control::ModeTypeFlags::PREFERRED),
                        m.size().0 as u32 * m.size().1 as u32,
                        m.vrefresh(),
                    )
                })
                .copied()
            else {
                continue;
            };
            let mut candidates = Vec::new();
            for encoder in info.encoders() {
                for crtc in resources.filter_crtcs(self.drm.get_encoder(*encoder)?.possible_crtcs())
                {
                    if !candidates.contains(&crtc) {
                        candidates.push(crtc);
                    }
                }
            }
            connected.push((info, mode, candidates));
        }
        // Augmenting paths avoid consuming the only CRTC available to another connector.
        let candidates: Vec<Vec<_>> = connected.iter().map(|c| c.2.clone()).collect();
        let assignments = assign_crtcs(&candidates);
        let selected: Vec<_> = connected
            .into_iter()
            .zip(assignments)
            .filter_map(|((info, mode, _), crtc)| {
                if crtc.is_none() {
                    eprintln!("No available CRTC for {}", connector_name(&info));
                }
                crtc.map(|crtc| (info, mode, crtc))
            })
            .collect();
        let mut retained = Vec::new();
        for old in self.outputs.drain(..) {
            let keep = selected.iter().any(|(info, mode, crtc)| {
                old.connector == Some(info.handle())
                    && old
                        .scanout
                        .as_ref()
                        .is_some_and(|s| s.crtc() == *crtc && s.pending_mode() == *mode)
            });
            if keep {
                retained.push(old);
            } else {
                state.space.unmap_output(&old.output);
                state
                    .display_handle
                    .remove_global::<Dome>(old.global.clone());
            }
        }
        self.outputs = retained;
        for (info, mode, crtc) in selected {
            if self
                .outputs
                .iter()
                .any(|o| o.connector == Some(info.handle()))
            {
                continue;
            }
            let name = output_name(&self.path, &info);
            let size: Size<i32, Physical> = (mode.size().0 as i32, mode.size().1 as i32).into();
            let surface = self.drm.create_surface(crtc, mode, &[info.handle()])?;
            let allocator = GbmAllocator::new(
                self.gbm.clone(),
                GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
            );
            let scanout = GbmBufferedSurface::new(
                surface,
                allocator,
                &[Fourcc::Xrgb8888, Fourcc::Argb8888],
                self.renderer
                    .egl_context()
                    .dmabuf_render_formats()
                    .iter()
                    .copied(),
            )?;
            self.add_output(
                state,
                name,
                size,
                OutputMode::from(mode).refresh,
                Some(scanout),
                info.size(),
            );
            let output = &self.outputs.last().unwrap().output;
            for mode in info.modes() {
                output.add_mode(OutputMode::from(*mode));
            }
        }
        Ok(())
    }
    fn add_output(
        &mut self,
        state: &mut Dome,
        name: String,
        size: Size<i32, Physical>,
        refresh: i32,
        scanout: Option<Scanout>,
        physical_size: Option<(u32, u32)>,
    ) {
        let connector = scanout
            .as_ref()
            .and_then(|s| s.pending_connectors().into_iter().next());
        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: physical_size
                    .map(|(w, h)| (w as i32, h as i32))
                    .unwrap_or_default()
                    .into(),
                subpixel: Subpixel::Unknown,
                make: "DomeD".into(),
                model: name.clone(),
            },
        );
        let global = output.create_global::<Dome>(&state.display_handle);
        let mode = OutputMode { size, refresh };
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            None,
            Some((0, 0).into()),
        );
        output.set_preferred(mode);
        self.outputs.push(NativeOutput {
            scanout,
            offscreen: None,
            damage: OutputDamageTracker::from_output(&output),
            output,
            global,
            size,
            connector,
            pending: false,
            desktop: DesktopRenderer::default(),
            rendered_frames: 0,
            presented_frames: 0,
        });
        eprintln!(
            "Dome native output: {name} {}x{} at {refresh} mHz",
            size.w, size.h
        );
    }
    fn draw(&mut self, state: &mut Dome) -> Result<(), Error> {
        if !self.active || Instant::now() < self.retry_after {
            return Ok(());
        }
        for output in &mut self.outputs {
            if output.pending {
                continue;
            }
            output.desktop.prepare(state, output.size);
            let origin = state
                .space
                .output_geometry(&output.output)
                .map(|g| g.loc)
                .unwrap_or_default();
            if let Some(scanout) = &mut output.scanout {
                let (mut buffer, age) = scanout.next_buffer()?;
                let sync = {
                    let mut framebuffer = self.renderer.bind(&mut buffer)?;
                    let sync = output.desktop.render_at(
                        state,
                        &mut self.renderer,
                        &mut framebuffer,
                        &mut output.damage,
                        age as usize,
                        true,
                        origin,
                    )?;
                    if let Some(path) = &self.capture {
                        capture(
                            &mut self.renderer,
                            &framebuffer,
                            output.size,
                            &capture_path(path, &output.output.name()),
                        )?;
                    }
                    sync
                };
                scanout.queue_buffer(Some(sync), None, ())?;
                output.pending = true;
            } else if let Some(texture) = &mut output.offscreen {
                let mut framebuffer = self.renderer.bind(texture)?;
                let sync = output.desktop.render_at(
                    state,
                    &mut self.renderer,
                    &mut framebuffer,
                    &mut output.damage,
                    0,
                    true,
                    origin,
                )?;
                sync.wait()?;
            }
            crate::desktop::finish_frame(state, &output.output);
            self.rendered_frames += 1;
            output.rendered_frames += 1;
        }
        self.write_report()?;
        Ok(())
    }
    fn write_report(&self) -> Result<(), Error> {
        if let Some(path) = &self.report {
            let outputs: Vec<_> = self.outputs.iter().map(|o| {
                let loc = o.output.current_location();
                serde_json::json!({"name":o.output.name(),"width":o.size.w,"height":o.size.h,"x":loc.x,"y":loc.y,
                    "physical_scanout":o.scanout.is_some(),"rendered_frames":o.rendered_frames,"presented_frames":o.presented_frames})
            }).collect();
            let first = self.outputs.first();
            let value = serde_json::json!({"backend":"drm","renderer":crate::rendering::selection(),"renderer_name":self.renderer_name,"device":self.path,"active":self.active,"outputs":outputs,
                "rendered_frames":self.rendered_frames,"presented_frames":self.presented_frames,
                "width":first.map(|o|o.size.w),"height":first.map(|o|o.size.h),
                "physical_scanout":first.is_some_and(|o|o.scanout.is_some()),"dmabuf_imports":self.dmabuf_imports});
            let path = report_path(path, &self.path);
            let temporary = path.with_extension("tmp");
            std::fs::write(&temporary, serde_json::to_vec_pretty(&value)?)?;
            std::fs::rename(temporary, path)?;
        }
        Ok(())
    }
    fn pause(&mut self, state: &mut Dome) {
        self.active = false;
        self.drm.pause();
        state.native_session_pause();
        let _ = self.write_report();
    }
    fn resume(&mut self, state: &mut Dome) -> Result<(), Error> {
        self.drm.activate(false)?;
        for output in &mut self.outputs {
            if let Some(scanout) = &mut output.scanout {
                scanout.reset_buffers();
            }
            output.pending = false;
        }
        self.refresh_output(state)?;
        self.active = true;
        state.native_session_active = true;
        state.applied_revision = u64::MAX;
        self.write_report()?;
        Ok(())
    }
    fn draw_or_recover(&mut self, state: &mut Dome) {
        if let Err(error) = self.draw(state) {
            eprintln!("Native frame failed; retrying in one second: {error}");
            for output in &mut self.outputs {
                if let Some(scanout) = &mut output.scanout {
                    scanout.reset_buffers();
                }
                output.pending = false;
            }
            let _ = self.drm.reset_state();
            self.retry_after = Instant::now() + Duration::from_secs(1);
        }
    }
}

fn capture(
    renderer: &mut GlesRenderer,
    framebuffer: &smithay::backend::renderer::gles::GlesTarget<'_>,
    size: Size<i32, Physical>,
    path: &Path,
) -> Result<(), Error> {
    use std::io::Write;
    let mapping = renderer.copy_framebuffer(
        framebuffer,
        smithay::utils::Rectangle::from_size((size.w, size.h).into()),
        Fourcc::Abgr8888,
    )?;
    let bytes = renderer.map_texture(&mapping)?;
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary)?;
    write!(file, "P6\n{} {}\n255\n", size.w, size.h)?;
    let rgb: Vec<u8> = bytes
        .chunks_exact(4)
        .flat_map(|p| p[..3].iter().copied())
        .collect();
    file.write_all(&rgb)?;
    drop(file);
    std::fs::rename(temporary, path)?;
    Ok(())
}

pub fn init(event_loop: &mut EventLoop<CalloopData>, data: &mut CalloopData) -> Result<(), Error> {
    let test_device = std::env::var_os("DOME_DRM_TEST_DEVICE").map(PathBuf::from);
    let test_devices = std::env::var("DOME_DRM_TEST_DEVICES").ok();
    let testing = test_device.is_some() || test_devices.is_some();
    if !testing
        && (std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some())
    {
        return Err("Native DRM must start from the login screen or an unused TTY; use --backend nested inside a desktop".into());
    }
    let (mut session, notifier) = if !testing {
        let (session, notifier) = LibSeatSession::new()?;
        if !session.is_active() {
            return Err("Native seat is not active".into());
        }
        (Some(session), Some(notifier))
    } else {
        (None, None)
    };
    let seat = session
        .as_ref()
        .map(|s| s.seat())
        .unwrap_or_else(|| "seat0".into());
    let paths: Vec<PathBuf> = if let Some(paths) = test_devices {
        paths.split(',').map(PathBuf::from).collect()
    } else if let Some(path) = test_device.clone() {
        vec![path]
    } else if let Some(path) = option_env!("DOME_BUILD_DRM_DEVICE") {
        vec![path.into()]
    } else if let Some(path) = std::env::var_os("DOME_DRM_DEVICE") {
        vec![path.into()]
    } else {
        #[cfg(feature = "multi-gpu")]
        { smithay::backend::udev::all_gpus(&seat)? }
        #[cfg(not(feature = "multi-gpu"))]
        { smithay::backend::udev::primary_gpu(&seat)?.into_iter().collect() }
    };
    if paths.is_empty() {
        return Err("No DRM devices on this seat".into());
    }
    validate_gpu_count(paths.len())?;
    #[cfg(not(feature = "multi-gpu"))]
    let selected_path = paths[0].clone();
    eprintln!("Dome DRM multi-GPU compiled: {}; selected devices: {paths:?}", cfg!(feature = "multi-gpu"));
    let devices: Devices = Rc::new(RefCell::new(Vec::new()));
    for path in paths {
        match init_gpu(
            event_loop.handle(),
            &mut data.state,
            &path,
            session.as_mut(),
            testing,
        ) {
            Ok(native) => devices.borrow_mut().push(native),
            Err(error) => eprintln!("Cannot initialize GPU {}: {error}", path.display()),
        }
    }
    if devices.borrow().is_empty() {
        return Err("No usable DRM GPU".into());
    }
    // Prefer the display GPU for client allocation while still initializing idle GPUs.
    devices
        .borrow_mut()
        .sort_by_key(|g| g.borrow().outputs.is_empty());
    relayout(&devices, &mut data.state)?;
    // Multi-GPU builds advertise only formats importable on every GPU.
    let formats = {
        let devices = devices.borrow();
        let formats = devices[0]
            .borrow()
            .renderer
            .egl_context()
            .dmabuf_texture_formats()
            .clone();
        #[cfg(feature = "multi-gpu")]
        let mut formats = formats;
        #[cfg(feature = "multi-gpu")]
        for device in devices.iter().skip(1) {
            let gpu = device.borrow();
            formats = formats
                .intersection(gpu.renderer.egl_context().dmabuf_texture_formats())
                .copied()
                .collect();
        }
        formats.iter().copied().collect::<Vec<_>>()
    };
    let render_device = devices.borrow()[0].borrow().device_id;
    let feedback =
        smithay::wayland::dmabuf::DmabufFeedbackBuilder::new(render_device, formats).build()?;
    data.state
        .dmabuf_state
        .create_global_with_default_feedback::<Dome>(&data.state.display_handle, &feedback);
    let importers = devices.clone();
    data.state.dmabuf_importer = Some(Box::new(move |dmabuf| {
        use smithay::backend::renderer::ImportDma;
        let devices = importers.borrow();
        let import = |gpu: &Rc<RefCell<Native>>| {
            let mut gpu = gpu.borrow_mut();
            let good = gpu.renderer.import_dmabuf(dmabuf, None).is_ok();
            if good {
                gpu.dmabuf_imports += 1;
            }
            good
        };
        #[cfg(feature = "multi-gpu")]
        { devices.iter().all(import) }
        #[cfg(not(feature = "multi-gpu"))]
        { devices.first().is_some_and(import) }
    }));
    if let Some(session) = session {
        let mut input = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
            session.clone().into(),
        );
        input
            .udev_assign_seat(&seat)
            .map_err(|_| "Unable to assign libinput seat")?;
        let mut vt_session = session.clone();
        event_loop.handle().insert_source(
            LibinputInputBackend::new(input.clone()),
            move |event, _, data| {
                if !data.state.native_session_active {
                    return;
                }
                match &event {
                    InputEvent::DeviceAdded { device }
                        if device.has_capability(
                            smithay::reexports::input::DeviceCapability::Keyboard,
                        ) =>
                    {
                        data.state.keyboard_devices.push(device.clone());
                        let leds = data.state.seat.get_keyboard().unwrap().led_state();
                        data.state.update_keyboard_leds(leds);
                    }
                    InputEvent::DeviceRemoved { device } => {
                        data.state.keyboard_devices.retain(|d| d != device)
                    }
                    _ => (),
                }
                if let InputEvent::Keyboard { event, .. } = &event {
                    let mods = data.state.seat.get_keyboard().unwrap().modifier_state();
                    let code = u32::from(event.key_code());
                    let vt = match code {
                        67..=76 => Some((code - 66) as i32),
                        95 => Some(11),
                        96 => Some(12),
                        _ => None,
                    };
                    if event.state() == KeyState::Pressed && mods.ctrl && mods.alt {
                        if let Some(vt) = vt {
                            data.state.shell_keys.insert(event.key_code());
                            if let Err(error) = vt_session.change_vt(vt) {
                                eprintln!("VT switch: {error}");
                            }
                            return;
                        }
                    }
                }
                data.state.process_input_event(event);
            },
        )?;
        let display = devices.clone();
        event_loop.handle().insert_source(
            notifier.unwrap(),
            move |event, _, data| match event {
                SessionEvent::PauseSession => {
                    input.suspend();
                    for gpu in display.borrow().iter() {
                        gpu.borrow_mut().pause(&mut data.state);
                    }
                }
                SessionEvent::ActivateSession => {
                    for gpu in display.borrow().iter() {
                        if let Err(error) = gpu.borrow_mut().resume(&mut data.state) {
                            eprintln!("DRM seat resume: {error}");
                        }
                    }
                    let _ = relayout(&display, &mut data.state);
                    if let Err(error) = input.resume() {
                        eprintln!("Input seat resume: {error:?}");
                        for gpu in display.borrow().iter() {
                            gpu.borrow_mut().pause(&mut data.state);
                        }
                        return;
                    }
                }
            },
        )?;
        let displays = devices.clone();
        let hotplug_handle = event_loop.handle();
        let mut hotplug_session = session.clone();
        event_loop
            .handle()
            .insert_source(UdevBackend::new(&seat)?, move |event, _, data| {
                match event {
                    UdevEvent::Added { device_id, path } => {
                        // A single-GPU binary may recover its selected card, but
                        // cannot acquire a second GPU through hotplug.
                        #[cfg(not(feature = "multi-gpu"))]
                        if !same_gpu_path(&selected_path, &path) {
                            return;
                        }
                        if !displays
                            .borrow()
                            .iter()
                            .any(|g| g.borrow().device_id == device_id)
                        {
                            match init_gpu(
                                hotplug_handle.clone(),
                                &mut data.state,
                                &path,
                                Some(&mut hotplug_session),
                                false,
                            ) {
                                Ok(gpu) => displays.borrow_mut().push(gpu),
                                Err(error) => eprintln!("GPU hotplug: {error}"),
                            }
                        }
                    }
                    UdevEvent::Changed { device_id } => {
                        for gpu in displays.borrow().iter() {
                            let mut gpu = gpu.borrow_mut();
                            if gpu.device_id == device_id && gpu.active {
                                if let Err(error) = gpu.refresh_output(&mut data.state) {
                                    eprintln!("Output hotplug: {error}");
                                }
                            }
                        }
                    }
                    UdevEvent::Removed { device_id } => {
                        displays.borrow_mut().retain(|gpu| {
                            let mut gpu = gpu.borrow_mut();
                            if gpu.device_id != device_id {
                                return true;
                            }
                            if let Some(token) = gpu.token.take() {
                                hotplug_handle.remove(token);
                            }
                            for output in gpu.outputs.drain(..) {
                                data.state.space.unmap_output(&output.output);
                                data.state
                                    .display_handle
                                    .remove_global::<Dome>(output.global);
                            }
                            false
                        });
                    }
                }
                if let Err(error) = relayout(&displays, &mut data.state) {
                    eprintln!("Display layout: {error}");
                }
            })?;
    }
    if testing {
        if let Some(path) = std::env::var_os("DOME_DRM_TEST_CONTROL") {
            use smithay::reexports::calloop::{generic::Generic, Interest, Mode, PostAction};
            let socket = std::os::unix::net::UnixDatagram::bind(path)?;
            socket.set_nonblocking(true)?;
            let test = devices.clone();
            event_loop.handle().insert_source(
                Generic::new(socket, Interest::READ, Mode::Level),
                move |_, socket, data| {
                    let mut bytes = [0u8; 32];
                    if let Ok(n) = socket.recv(&mut bytes) {
                        let devices = test.borrow();
                        for gpu in devices.iter() {
                            let mut native = gpu.borrow_mut();
                            match std::str::from_utf8(&bytes[..n]).unwrap_or("").trim() {
                                "pause" => native.pause(&mut data.state),
                                "resume" => {
                                    if let Err(error) = native.resume(&mut data.state) {
                                        eprintln!("Test seat resume: {error}");
                                    }
                                }
                                "rescan" => {
                                    if let Err(error) = native.refresh_output(&mut data.state) {
                                        eprintln!("Test output rescan: {error}");
                                    }
                                }
                                "quit" => data.state.loop_signal.stop(),
                                _ => (),
                            }
                        }
                        drop(devices);
                        let _ = relayout(&test, &mut data.state);
                    }
                    Ok(PostAction::Continue)
                },
            )?;
        }
    }
    let tick = devices.clone();
    let mut applied_displays = data.state.scene.lock().unwrap().config.displays.clone();
    event_loop.handle().insert_source(
        Timer::from_duration(Duration::from_millis(16)),
        move |_, _, data| {
            let profiles = data.state.scene.lock().unwrap().config.displays.clone();
            if profiles != applied_displays {
                for gpu in tick.borrow().iter() {
                    if let Err(error) = gpu.borrow_mut().refresh_output(&mut data.state) {
                        eprintln!("Display configuration: {error}");
                    }
                }
                let _ = relayout(&tick, &mut data.state);
                applied_displays = profiles;
            }
            for gpu in tick.borrow().iter() {
                gpu.borrow_mut().draw_or_recover(&mut data.state);
            }
            TimeoutAction::ToDuration(Duration::from_millis(16))
        },
    )?;
    crate::activate_client_socket(&mut data.state);
    Ok(())
}

impl Dome {
    fn native_session_pause(&mut self) {
        use smithay::{
            backend::input::ButtonState,
            input::{
                keyboard::FilterResult,
                pointer::{ButtonEvent, MotionEvent},
            },
            utils::SERIAL_COUNTER,
        };
        self.native_session_active = false;
        let time = self.start_time.elapsed().as_millis() as u32;
        if let Some(keyboard) = self.seat.get_keyboard() {
            for code in keyboard.pressed_keys() {
                self.native_ignored_keys.insert(code);
                let owned = self.shell_keys.remove(&code);
                keyboard.input::<(), _>(
                    self,
                    code,
                    KeyState::Released,
                    SERIAL_COUNTER.next_serial(),
                    time,
                    move |_, _, _| {
                        if owned {
                            FilterResult::Intercept(())
                        } else {
                            FilterResult::Forward
                        }
                    },
                );
            }
            keyboard.unset_grab(self);
            keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
        if let Some(pointer) = self.seat.get_pointer() {
            let mut buttons: std::collections::HashSet<_> = self.physical_buttons.drain().collect();
            buttons.extend(self.desktop_buttons.drain());
            buttons.extend(self.xr_buttons.drain());
            self.ignored_buttons.extend(buttons.iter().copied());
            for button in buttons {
                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: ButtonState::Released,
                        serial: SERIAL_COUNTER.next_serial(),
                        time,
                    },
                );
            }
            pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), time);
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location: pointer.current_location(),
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
        }
        self.native_ignored_keys.extend(self.shell_keys.drain());
        self.chrome_drag = None;
        self.chrome_button = None;
        self.snap_preview = None;
        self.dismiss_wayland_popups();
        self.context_menus.clear();
        self.menu = None;
        self.launcher = false;
        self.task_popup = None;
        self.xr_target = None;
        self.xr_shell_pos = None;
        self.mouse_angles = None;
        self.input_xr_active = false;
        self.input_realm = dome::Realm::Desktop;
        let mut bridge = self.xr_bridge.lock().unwrap();
        bridge.packets.clear();
        bridge.cursor = None;
        bridge.mouse_angles = None;
        bridge.mouse_buttons.clear();
        bridge.last_mouse = None;
        bridge.exit_requested = true;
        drop(bridge);
        let _ = self.display_handle.flush_clients();
        if std::env::var("DOME_LOGIN_SESSION").as_deref() == Ok("1") {
            let _ = std::process::Command::new("/usr/local/bin/ghostty-xrctl")
                .arg("hide")
                .spawn();
        }
    }
}

type Devices = Rc<RefCell<Vec<Rc<RefCell<Native>>>>>;
fn validate_gpu_count(count: usize) -> Result<(), Error> {
    if count > 1 && !cfg!(feature = "multi-gpu") {
        return Err("Multiple DRM devices require a build with --features multi-gpu".into());
    }
    Ok(())
}

#[cfg(not(feature = "multi-gpu"))]
fn same_gpu_path(selected: &Path, candidate: &Path) -> bool {
    match (selected.canonicalize(), candidate.canonicalize()) {
        (Ok(selected), Ok(candidate)) => selected == candidate,
        _ => false,
    }
}

fn init_gpu(
    handle: smithay::reexports::calloop::LoopHandle<'_, CalloopData>,
    state: &mut Dome,
    path: &Path,
    session: Option<&mut LibSeatSession>,
    _test: bool,
) -> Result<Rc<RefCell<Native>>, Error> {
    // Stable PCI by-path pins must retain the same output identities as cardN paths.
    let path = path.canonicalize()?;
    crate::rendering::create(|| init_gpu_inner(handle, state, &path, session, _test))
}
fn init_gpu_inner(
    handle: smithay::reexports::calloop::LoopHandle<'_, CalloopData>,
    state: &mut Dome,
    path: &Path,
    session: Option<&mut LibSeatSession>,
    _test: bool,
) -> Result<Rc<RefCell<Native>>, Error> {
    let owned = if let Some(session) = session {
        session.open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY,
        )?
    } else {
        // Only the kernel's virtual KMS driver may bypass seat/input ownership.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        struct Probe(std::fs::File);
        impl std::os::fd::AsFd for Probe {
            fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
                self.0.as_fd()
            }
        }
        impl BasicDevice for Probe {}
        if Probe(file.try_clone()?).get_driver()?.name() != std::ffi::OsStr::new("vkms") {
            return Err("DOME_DRM_TEST_DEVICE only accepts the vkms driver; refusing physical display/input access".into());
        }
        file.into()
    };
    let fd = DrmDeviceFd::new(DeviceFd::from(owned));
    let device_id = fd.dev_id()?;
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd)?;
    let display = unsafe { EGLDisplay::new(gbm.clone())? };
    let context = EGLContext::new(&display)?;
    let mut renderer = unsafe { GlesRenderer::new(context)? };
    let renderer_name = crate::rendering::verify(&mut renderer)?;
    let native = Rc::new(RefCell::new(Native {
        drm,
        gbm,
        renderer,
        renderer_name,
        outputs: Vec::new(),
        device_id,
        path: path.into(),
        token: None,
        active: true,
        retry_after: Instant::now(),
        capture: std::env::var_os("DOME_DRM_CAPTURE").map(PathBuf::from),
        report: std::env::var_os("DOME_DRM_REPORT").map(PathBuf::from),
        rendered_frames: 0,
        presented_frames: 0,
        dmabuf_imports: 0,
    }));
    native.borrow_mut().refresh_output(state)?;
    let draw = Rc::downgrade(&native);
    let token = handle.insert_source(drm_notifier, move |event, _, data| {
        let Some(draw) = draw.upgrade() else { return };
        let mut native = draw.borrow_mut();
        match event {
            DrmEvent::VBlank(crtc) => {
                native.presented_frames += 1;
                for output in &mut native.outputs {
                    if let Some(scanout) = &mut output.scanout {
                        if scanout.crtc() == crtc {
                            if let Err(error) = scanout.frame_submitted() {
                                eprintln!("Page flip completion: {error}");
                            }
                            output.pending = false;
                            output.presented_frames += 1;
                        }
                    }
                }
                native.draw_or_recover(&mut data.state);
            }
            DrmEvent::Error(error) => eprintln!("Native DRM event: {error}"),
        }
    })?;
    native.borrow_mut().token = Some(token);
    Ok(native)
}

fn capture_path(path: &Path, name: &str) -> PathBuf {
    // Preserve the original single-output capture for the existing smoke harness.
    if std::env::var_os("DOME_DRM_CAPTURE_ALL").is_none() {
        return path.into();
    }
    path.with_file_name(format!(
        "{}-{}.ppm",
        path.file_stem().unwrap_or_default().to_string_lossy(),
        name
    ))
}
fn report_path(path: &Path, device: &Path) -> PathBuf {
    if std::env::var_os("DOME_DRM_REPORT_ALL").is_none() {
        return path.into();
    }
    path.with_file_name(format!(
        "{}-{}.json",
        path.file_stem().unwrap_or_default().to_string_lossy(),
        device.file_name().unwrap_or_default().to_string_lossy()
    ))
}
fn relayout(devices: &Devices, state: &mut Dome) -> Result<(), Error> {
    let devices = devices.borrow();
    let physical = devices
        .iter()
        .any(|g| g.borrow().outputs.iter().any(|o| o.scanout.is_some()));
    for gpu in devices.iter() {
        let mut gpu = gpu.borrow_mut();
        if physical {
            gpu.outputs.retain(|o| {
                if o.scanout.is_some() {
                    return true;
                }
                state.space.unmap_output(&o.output);
                state.display_handle.remove_global::<Dome>(o.global.clone());
                false
            });
        }
    }
    if !physical {
        if let Some(gpu) = devices.first() {
            let mut gpu = gpu.borrow_mut();
            if gpu.outputs.is_empty() {
                gpu.add_output(
                    state,
                    "XR-headless".into(),
                    (1280, 800).into(),
                    60000,
                    None,
                    None,
                );
                gpu.outputs[0].offscreen = Some(
                    gpu.renderer
                        .create_buffer(Fourcc::Argb8888, (1280, 800).into())?,
                );
            }
        }
    }
    let config = state.scene.lock().unwrap().config.clone();
    let mut x = 0;
    for gpu in devices.iter() {
        let mut gpu = gpu.borrow_mut();
        for o in &mut gpu.outputs {
            let profile = config.displays.iter().find(|d| d.name == o.output.name());
            let pos = (
                profile.and_then(|d| d.x).unwrap_or(x),
                profile.map_or(0, |d| d.y),
            );
            o.output
                .change_current_state(None, None, None, Some(pos.into()));
            state.space.map_output(&o.output, pos);
            x = x.max(pos.0 + o.size.w);
        }
    }
    state.recover_output_layout();
    state.applied_revision = u64::MAX;
    Ok(())
}

fn assign_crtcs<T: Copy + Eq>(candidates: &[Vec<T>]) -> Vec<Option<T>> {
    fn visit<T: Copy + Eq>(
        i: usize,
        candidates: &[Vec<T>],
        assigned: &mut [Option<T>],
        seen: &mut Vec<T>,
    ) -> bool {
        for &crtc in &candidates[i] {
            if seen.contains(&crtc) {
                continue;
            }
            seen.push(crtc);
            let owner = assigned.iter().position(|c| *c == Some(crtc));
            if owner.is_none_or(|j| visit(j, candidates, assigned, seen)) {
                assigned[i] = Some(crtc);
                return true;
            }
        }
        false
    }
    let mut assigned = vec![None; candidates.len()];
    for i in 0..candidates.len() {
        visit(i, candidates, &mut assigned, &mut Vec::new());
    }
    assigned
}
#[cfg(test)]
mod tests {
    use super::assign_crtcs;
    #[test]
    fn multiple_devices_require_compile_time_support() {
        assert!(super::validate_gpu_count(1).is_ok());
        assert_eq!(super::validate_gpu_count(2).is_ok(), cfg!(feature = "multi-gpu"));
    }
    #[cfg(not(feature = "multi-gpu"))]
    #[test]
    fn single_gpu_hotplug_accepts_only_the_selected_device() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("dome-gpu-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("arc"), []).unwrap();
        std::fs::write(dir.join("uhd"), []).unwrap();
        symlink(dir.join("arc"), dir.join("pci-card")).unwrap();
        assert!(super::same_gpu_path(&dir.join("pci-card"), &dir.join("arc")));
        assert!(!super::same_gpu_path(&dir.join("pci-card"), &dir.join("uhd")));
        assert!(!super::same_gpu_path(&dir.join("missing"), &dir.join("arc")));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn assigns_distinct_crtcs_and_rehomes_flexible_connector() {
        assert_eq!(assign_crtcs(&[vec![1, 2], vec![1]]), vec![Some(2), Some(1)]);
        let assigned = assign_crtcs(&[vec![1], vec![1], vec![2]]);
        assert_eq!(assigned.iter().filter(|a| a.is_some()).count(), 2);
    }
}

fn output_name(path: &Path, info: &control::connector::Info) -> String {
    let card = path.file_name().unwrap_or_default();
    let device = std::fs::canonicalize(Path::new("/sys/class/drm").join(card).join("device"));
    let id = device
        .as_ref()
        .ok()
        .and_then(|p| p.file_name())
        .unwrap_or(card)
        .to_string_lossy();
    format!("{id}-{}", connector_name(info))
}
