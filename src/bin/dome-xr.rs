//! Native OpenXR quad compositor. Window content and seat input remain owned by Smithay.
use ash::vk::{self, Handle};
use dome::{Pose, Scene};
use openxr as xr;
use std::{
    collections::HashMap,
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
#[path = "dome-xr/gpu.rs"]
mod gpu;
#[path = "dome-xr/screenshot.rs"]
mod screenshot;
const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
#[derive(Clone)]
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    gpu: Option<Arc<dome::xr_gpu::Received>>,
    serial: u64,
}
#[derive(Default)]
struct Network {
    scene: Option<Scene>,
    session: dome::xr_transport::Session,
    sampled: Option<Instant>,
    control: dome::xr_transport::Control,
    control_received: Option<Instant>,
    shell: dome::xr_transport::ShellLayout,
    images: HashMap<u64, Image>,
    gpu_error: Option<String>,
}
fn scene_border(n: &Arc<Mutex<Network>>) -> u8 {
    n.lock()
        .unwrap()
        .scene
        .as_ref()
        .map(|s| s.config.window_border_px)
        .unwrap_or(1)
}
fn main() {
    if let Err(e) = run() {
        eprintln!("dome-xr: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut base = "http://127.0.0.1:49640".to_string();
    let mut frames = u64::MAX;
    let mut capture = None;
    let mut report = None;
    let mut standalone = false;
    let mut music_floor_only = false;
    let mut floor_size: Option<[f32; 2]> = None;
    let mut webxr_socket: Option<std::path::PathBuf> = None;
    let mut placement = 10u32;
    let mut transport = "auto".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--transport" => transport = args.next().ok_or("missing transport")?,
            "--server" => base = args.next().ok_or("missing server")?,
            "--webxr-socket" => {
                webxr_socket = Some(args.next().ok_or("missing WebXR socket")?.into())
            }
            "--frames" => frames = args.next().ok_or("missing frames")?.parse()?,
            "--capture" => capture = args.next(),
            "--report" => report = args.next(),
            "--standalone" => standalone = true,
            "--music-floor-only" => music_floor_only = true,
            "--floor-size" => {
                let value = args.next().ok_or("missing floor width x depth in metres")?;
                let (width, depth) = value.split_once('x').ok_or("floor size must be WIDTHxDEPTH")?;
                let size = [width.parse::<f32>()?, depth.parse::<f32>()?];
                if size.iter().any(|v| !v.is_finite() || *v <= 0. || *v > 100.) {
                    return Err("floor dimensions must be finite and within 0..100 metres".into());
                }
                floor_size = Some(size);
            }
            "--placement" => {
                placement = args.next().ok_or("missing placement")?.parse()?;
                if placement == 0 {
                    return Err("overlay placement must be positive".into());
                }
            }
            "--help" => {
                println!("dome-xr [--server http://127.0.0.1:PORT] [--frames N] [--transport auto|cpu|gpu] [--capture DIR] [--report FILE] [--webxr-socket PATH] [--standalone] [--placement N] [--music-floor-only] [--floor-size WIDTHxDEPTH]\nMusic-floor-only runs a passive audio-reactive overlay without a desktop server.\nFloor-size explicitly supplies the STAGE floor extent when the runtime does not expose bounds.\nDefault: transparent overlay, or passthrough blending when overlay sessions are unavailable.");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {a}").into()),
        }
    }
    if !base.starts_with("http://127.0.0.1:") || base[17..].parse::<u16>().is_err() {
        return Err("server must be numeric loopback http://127.0.0.1:PORT".into());
    }
    if !matches!(transport.as_str(), "auto" | "cpu" | "gpu") { return Err("transport must be auto, cpu or gpu".into()); }
    let gpu_enabled = Arc::new(AtomicBool::new(false));
    let gpu_failed = Arc::new(Mutex::new(std::collections::HashSet::<u64>::new()));
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::Relaxed);
    })?;
    let session_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos() as u64
        ^ std::process::id() as u64;
    let mut initial = Network::default();
    initial.session.id = session_id;
    let network = Arc::new(Mutex::new(initial));
    // Cursor/pose traffic must not queue behind full window texture downloads.
    let input_network = network.clone();
    let input_running = running.clone();
    let input_base = base.clone();
    let input_worker = std::thread::spawn(move || {
        if music_floor_only { return; }
        let n = input_network;
        let r = input_running;
        let base = input_base;
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_millis(250)).build();
        let mut last_heartbeat = Instant::now() - Duration::from_secs(1);
        while r.load(Ordering::Relaxed) {
            if last_heartbeat.elapsed() >= Duration::from_millis(5) {
                let (mut status, timing_supported) = {
                    let state = n.lock().unwrap();
                    let mut status = state.session.clone();
                    status.visible &= state
                        .sampled
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(750));
                    (status, state.control.frame_period_ns.is_some())
                };
                if !r.load(Ordering::Relaxed) {
                    status.visible = false;
                }
                let _ = agent
                    .post(&format!("{base}/api/xr-session"))
                    .send_json(status.wire_value(timing_supported));
                if let Ok(response) = agent.get(&format!("{base}/api/xr-control")).call() {
                    if let Ok(control) = response.into_json::<dome::xr_transport::Control>() {
                        let mut state = n.lock().unwrap();
                        state.control = control;
                        state.control_received = Some(Instant::now());
                    }
                }
                last_heartbeat = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let _ = agent.post(&format!("{base}/api/xr-session")).send_json(
            dome::xr_transport::Session {
                id: session_id,
                ..Default::default()
            }.wire_value(n.lock().unwrap().control.frame_period_ns.is_some()),
        );
    });
    let n = network.clone();
    let r = running.clone();
    let worker_gpu_enabled = gpu_enabled.clone();
    let worker_gpu_failed = gpu_failed.clone();
    let capture_cpu = capture.is_some();
    let worker_transport = transport.clone();
    let webxr_control_port = base
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(49640);
    let screenshot_base=base.clone();
    let worker = std::thread::spawn(move || {
        if music_floor_only { return; }
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(250))
            .build();
        let mut serial = 0;
        let mut gpu_waiting = HashMap::<u64,Instant>::new();
        let mut last_server = Instant::now();
        let mut deadline = Instant::now();
        while r.load(Ordering::Relaxed) {
            if let Ok(response) = agent.get(&format!("{base}/api/state")).call() {
                last_server = Instant::now();
                if let Ok(value) = response.into_json::<serde_json::Value>() {
                    if let Ok(scene) = serde_json::from_value::<Scene>(value["scene"].clone()) {
                        let mut ids: Vec<_> = scene
                            .windows
                            .iter()
                            .filter(|w| w.xr_visible && !w.minimized)
                            .map(|w| w.id)
                            .collect();
                        if let Ok(response) = agent.get(&format!("{base}/api/xr-shell")).call() {
                            if let Ok(layout) =
                                response.into_json::<dome::xr_transport::ShellLayout>()
                            {
                                if layout.notifications.is_some() {
                                    ids.push(dome::xr_transport::NOTIFICATIONS);
                                }
                                n.lock().unwrap().shell = layout;
                            }
                        }
                        ids.extend(
                            scene
                                .popups
                                .iter()
                                .filter(|p| scene.popup_visible(p, false))
                                .map(|p| p.id),
                        );
                        ids.extend([dome::xr_transport::TOP_BAR, dome::xr_transport::TASK_BAR]);
                        {
                            let mut state = n.lock().unwrap();
                            state.images.retain(|id, _| ids.contains(id));
                            state.scene = Some(scene);
                        }
                        let border = scene_border(&n);
                        for id in ids {
                            if worker_gpu_enabled.load(Ordering::Relaxed) && !capture_cpu
                                && !worker_gpu_failed.lock().unwrap().contains(&id) {
                                let path = n.lock().unwrap().control.gpu_socket.clone();
                                if let Some(path) = path {
                                    if let Ok(frame) = dome::xr_gpu::receive(std::path::Path::new(&path), id) {
                                        gpu_waiting.remove(&id);
                                        let mut state = n.lock().unwrap();
                                        let same = state
                                            .images
                                            .get(&id)
                                            .and_then(|i| i.gpu.as_ref())
                                            .is_some_and(|old| {
                                                old.info.serial == frame.info.serial
                                            });
                                        if !same {
                                            let (width,height) = frame.info.output_size();
                                            serial += 1;
                                            state.images.insert(
                                                id,
                                                Image {
                                                    width,
                                                    height,
                                                    rgba: vec![],
                                                    gpu: Some(Arc::new(frame)),
                                                    serial,
                                                },
                                            );
                                        }
                                        continue;
                                    }
                                }
                                if worker_transport == "gpu" {
                                    if gpu_waiting.entry(id).or_insert_with(Instant::now).elapsed() > Duration::from_secs(3) {
                                        n.lock().unwrap().gpu_error = Some(format!("GPU frame unavailable for window {id}; use --transport auto for fallback"));
                                        r.store(false, Ordering::Relaxed);
                                    }
                                    continue;
                                }
                            }
                            if let Ok(response) =
                                agent.get(&format!("{base}/api/xr-frame/{id}")).call()
                            {
                                let pixel_scale = response.header("X-Dome-Scale").and_then(|v| v.parse::<f32>().ok())
                                    .filter(|v| v.is_finite() && *v > 0. && *v <= 2.).unwrap_or(1.);
                                let dims = response
                                    .header("X-Dome-Width")
                                    .and_then(|v| v.parse::<u32>().ok())
                                    .zip(
                                        response
                                            .header("X-Dome-Height")
                                            .and_then(|v| v.parse::<u32>().ok()),
                                    );
                                if let Some((width, height)) = dims
                                    .filter(|(w, h)| *w > 0 && *h > 0 && *w <= 4096 && *h <= 4096)
                                {
                                    let mut rgba = vec![];
                                    if response
                                        .into_reader()
                                        .take(67108865)
                                        .read_to_end(&mut rgba)
                                        .is_ok()
                                        && rgba.len() == width as usize * height as usize * 4
                                    {
                                        let stereo = n
                                            .lock()
                                            .unwrap()
                                            .scene
                                            .as_ref()
                                            .and_then(|s| s.window(id))
                                            .is_some_and(|w| {
                                                w.stereo == dome::stereo::Stereo::SideBySide
                                            });
                                        if !dome::xr_transport::is_shell(id) && !stereo {
                                            dome::xr_input::paint_border_scaled(
                                                &mut rgba, width, height, border, pixel_scale,
                                            );
                                        }
                                        let state = n.lock().unwrap();
                                        if !dome::xr_transport::is_shell(id)
                                            && state
                                                .scene
                                                .as_ref()
                                                .is_some_and(|s| s.focused == Some(id) && s.xr_fullscreen().is_none_or(|w|w.id != id))
                                        {
                                            dome::visual_feedback::focus_border_scaled(
                                                &mut rgba,
                                                width,
                                                height,
                                                state.control.accent,
                                                stereo,
                                                pixel_scale,
                                            );
                                        }
                                        drop(state);
                                        serial += 1;
                                        n.lock().unwrap().images.insert(
                                            id,
                                            Image {
                                                width,
                                                height,
                                                rgba,
                                                gpu: None,
                                                serial,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if last_server.elapsed() > Duration::from_secs(3) {
                r.store(false, Ordering::Relaxed);
            }
            let interval = dome::xr_transport::frame_interval(n.lock().unwrap().session.frame_period_ns)
                .unwrap_or(Duration::from_millis(10));
            deadline = dome::xr_transport::next_frame_deadline(deadline, Instant::now(), interval);
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        }
    });
    let entry = unsafe {
        xr::Entry::load()
            .or_else(|_| xr::Entry::load_from(std::path::Path::new("libopenxr_loader.so.1")))?
    };
    let available = entry.enumerate_extensions()?;
    if !available.khr_vulkan_enable2 {
        return Err("runtime lacks Vulkan enable2".into());
    }
    let mut extensions = xr::ExtensionSet::default();
    extensions.extx_overlay = available.extx_overlay && !standalone;
    extensions.khr_vulkan_enable2 = true;
    extensions.khr_composition_layer_cylinder = available.khr_composition_layer_cylinder;
    let xr_instance = entry.create_instance(
        &xr::ApplicationInfo {
            application_name: "Dome",
            application_version: 1,
            engine_name: "Dome Rust",
            engine_version: 1,
            api_version: xr::Version::new(1, 0, 0),
        },
        &extensions,
        &[],
    )?;
    let runtime = xr_instance.properties()?.runtime_name;
    let system = xr_instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)?;
    let properties = xr_instance.system_properties(system)?;
    let max_layers = properties.graphics_properties.max_layer_count as usize;
    let modes = xr_instance.enumerate_environment_blend_modes(system, VIEW_TYPE)?;
    let environment = dome::xr_environment::select(available.extx_overlay, &modes, standalone)?;
    let blend = environment.blend;
    eprintln!(
        "Dome XR presentation: overlay={}, environment_blend={blend:?}, placement={placement}",
        environment.overlay
    );
    let req = xr_instance.graphics_requirements::<xr::Vulkan>(system)?;
    if req.min_api_version_supported > xr::Version::new(1, 1, 0) {
        return Err("Vulkan 1.1 unsupported by runtime".into());
    }
    unsafe {
        let vk_entry = ash::Entry::load()?;

        let vk_app_info = vk::ApplicationInfo::default()
            .application_version(0)
            .engine_version(0)
            .api_version(vk::make_api_version(0, 1, 1, 0));

        let vk_instance = {
            let vk_instance = xr_instance
                .create_vulkan_instance(
                    system,
                    std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                    &vk::InstanceCreateInfo::default().application_info(&vk_app_info) as *const _
                        as *const _,
                )?
                .map_err(vk::Result::from_raw)?;
            ash::Instance::load(
                vk_entry.static_fn(),
                vk::Instance::from_raw(vk_instance as _),
            )
        };

        let vk_physical_device = vk::PhysicalDevice::from_raw(
            xr_instance
                .vulkan_graphics_device(system, vk_instance.handle().as_raw() as _)
                .unwrap() as _,
        );

        let vk_device_properties = vk_instance.get_physical_device_properties(vk_physical_device);
        if vk_device_properties.api_version < vk::make_api_version(0, 1, 1, 0) {
            vk_instance.destroy_instance(None);
            panic!("Vulkan phyiscal device doesn't support version 1.1");
        }

        let queue_family_index = vk_instance
            .get_physical_device_queue_family_properties(vk_physical_device)
            .into_iter()
            .enumerate()
            .find_map(|(queue_family_index, info)| {
                if info.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                    Some(queue_family_index as u32)
                } else {
                    None
                }
            })
            .expect("Vulkan device has no graphics queue");

        let gpu_extensions = gpu::extensions(&vk_instance, vk_physical_device);
        if transport == "gpu" && gpu_extensions.is_none() { return Err("Required Vulkan DMA-BUF extensions unavailable".into()); }
        let enabled_gpu_extensions = if transport != "cpu" && !capture_cpu { gpu_extensions.clone().unwrap_or_default() } else { vec![] };
        let vk_device = {
            let vk_device = xr_instance
                .create_vulkan_device(
                    system,
                    std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                    vk_physical_device.as_raw() as _,
                    &vk::DeviceCreateInfo::default()
                        .enabled_extension_names(&enabled_gpu_extensions)
                        .queue_create_infos(&[vk::DeviceQueueCreateInfo::default()
                            .queue_family_index(queue_family_index)
                            .queue_priorities(&[1.0])])
                        .push_next(&mut vk::PhysicalDeviceMultiviewFeatures {
                            multiview: vk::TRUE,
                            ..Default::default()
                        }) as *const _ as *const _,
                )?
                .map_err(vk::Result::from_raw)?;

            ash::Device::load(vk_instance.fp_v1_0(), vk::Device::from_raw(vk_device as _))
        };

        let queue = vk_device.get_device_queue(queue_family_index, 0);

        let (session, mut waiter, mut stream) = dome::xr_environment::create_session(
            &xr_instance,
            system,
            &xr::vulkan::SessionCreateInfo {
                instance: vk_instance.handle().as_raw() as _,
                physical_device: vk_physical_device.as_raw() as _,
                device: vk_device.handle().as_raw() as _,
                queue_family_index,
                queue_index: 0,
            },
            environment.overlay,
            placement,
        )?;
        let floor_stage_available = session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY).is_ok();
        let mut music_floor = dome::music_floor::MusicFloor::default();
        let mut floor_bounds = None;
        let mut floor_announced = false;
        let mut floor_uploaded = false;
        let mut floor_bounds_read = Instant::now() - Duration::from_secs(2);
        let stage = session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
            .or_else(|_| {
                session.create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            })?;
        let view =
            session.create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)?;
        let recommended = xr_instance.enumerate_view_configuration_views(system, VIEW_TYPE)?;
        if recommended.len() != 2 {
            return Err("Dome WebXR bridge requires stereo views".into());
        }
        let eye_sizes = [0, 1].map(|i| {
            [
                recommended[i]
                    .recommended_image_rect_width
                    .min(dome::webxr_bridge::MAX_DIMENSION / 2),
                recommended[i]
                    .recommended_image_rect_height
                    .min(dome::webxr_bridge::MAX_DIMENSION),
            ]
        });
        let control_port = webxr_control_port;
        let webxr_socket = webxr_socket.unwrap_or_else(|| {
            std::path::PathBuf::from(
                std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_else(|| "/tmp".into()),
            )
            .join(format!("dome-webxr-{control_port}.sock"))
        });
        let browser = if music_floor_only {
            None
        } else {
            match dome::webxr_bridge::Server::bind(
                &webxr_socket,
                dome::webxr_bridge::Capabilities {
                    name: "DomeD XR session".into(),
                    width: eye_sizes[0][0] + eye_sizes[1][0],
                    height: eye_sizes[0][1].max(eye_sizes[1][1]),
                    view_sizes: eye_sizes,
                    max_width: dome::webxr_bridge::MAX_DIMENSION,
                    max_height: dome::webxr_bridge::MAX_DIMENSION,
                    blend_modes: vec!["opaque".into()],
                    stage: floor_stage_available,
                    period: 0,
                },
            ) {
                Ok(server) => {
                    eprintln!("Dome WebXR session bridge: {}", webxr_socket.display());
                    Some(server)
                }
                Err(error) => {
                    eprintln!(
                        "Dome WebXR bridge unavailable at {}: {error}",
                        webxr_socket.display()
                    );
                    None
                }
            }
        };
        let mut browser_panel: Option<Panel> = None;
        let mut browser_views: Option<[dome::webxr_bridge::ProjectionView; 2]> = None;
        let mut browser_updated: Option<Instant> = None;
        let mut browser_layer_flags = 0u64;
        let mut animator = dome::xr_presentation::Animator::default();
        let mut presentation_initialized = false;
        let mut presentation = dome::xr_presentation::Presentation::default();
        let mut transition = dome::xr_presentation::Frame::default();
        let mut hidden_frames = 0u64;
        let mut fade_frames = 0u64;
        let mut slide_frames = 0u64;
        let start = Instant::now();
        let formats = session.enumerate_swapchain_formats()?;
        let format = [vk::Format::R8G8B8A8_SRGB, vk::Format::R8G8B8A8_UNORM]
            .into_iter()
            .find(|f| formats.contains(&(f.as_raw() as u32)))
            .ok_or("no RGBA swapchain format")?;
        // The browser facade exposes GL_RGBA8, whose bytes encode linear RGB.
        // Desktop snapshots use sRGB bytes and keep their separate format above.
        let browser_format = formats
            .contains(&(vk::Format::R8G8B8A8_UNORM.as_raw() as u32))
            .then_some(vk::Format::R8G8B8A8_UNORM);
        let mut gpu_renderer = if !enabled_gpu_extensions.is_empty() {
            match gpu::Renderer::new(
                &vk_instance,
                &vk_device,
                vk_physical_device,
                queue_family_index,
                format,
            ) {
                Ok(r) => {
                    gpu_enabled.store(true, Ordering::Relaxed);
                    eprintln!("XR DMA-BUF transport enabled");
                    Some(r)
                }
                Err(e) if transport == "gpu" => return Err(e),
                Err(e) => { eprintln!("XR GPU renderer unavailable; using CPU: {e}"); None }
            }
        } else { None };
        let mut gpu_submissions = 0u64;
        let pool = vk_device.create_command_pool(
            &vk::CommandPoolCreateInfo::default()
                .queue_family_index(queue_family_index)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
            None,
        )?;
        let command = vk_device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default()
                .command_pool(pool)
                .command_buffer_count(1),
        )?[0];
        let mut chains: HashMap<u64, Panel> = HashMap::new();
        let mut event = xr::EventDataBuffer::new();
        let mut active = false;
        let mut focused = false;
        let mut visible = false;
        let mut exiting = false;
        let mut count = 0;
        let mut last_head = Pose::default();
        let mut uploaded = 0;
        let mut peak = 0;
        let mut peak_curved = 0;
        let mut peak_flat = 0;
        let mut peak_stereo = 0;
        let mut peak_shell = 0;
        let mut captured = std::collections::HashSet::new();
        let mut last_screenshot_request=0;
        'main: loop {
            while let Some(e) = xr_instance.poll_event(&mut event)? {
                match e {
                    xr::Event::SessionStateChanged(e) => {
                        eprintln!("XR {:?}", e.state());
                        focused = e.state() == xr::SessionState::FOCUSED;
                        visible = focused || e.state() == xr::SessionState::VISIBLE;
                        match e.state() {
                            xr::SessionState::READY => {
                                session.begin(VIEW_TYPE)?;
                                active = true;
                            }
                            xr::SessionState::STOPPING => {
                                if let Some(browser) = &browser {
                                    browser.suspend();
                                }
                                session.end()?;
                                active = false;
                            }
                            xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                                break 'main
                            }
                            _ => (),
                        }
                    }
                    xr::Event::InstanceLossPending(_) => break 'main,
                    _ => (),
                }
            }
            if !running.load(Ordering::Relaxed)
                || count >= frames
                || network.lock().unwrap().control.exit_requested
            {
                if !exiting {
                    if active {
                        session.request_exit()?;
                        exiting = true;
                    } else {
                        break;
                    }
                }
            }
            if !active {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            let frame = waiter.wait()?;
            stream.begin()?;
            count += 1;
            let head = view.locate(&stage, frame.predicted_display_time)?;
            let head_valid = head.location_flags.contains(
                xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID,
            );
            if head_valid {
                last_head = pose(head.pose);
            }
            let head_pose = last_head;
            let (webxr_view_flags, webxr_views) =
                session.locate_views(VIEW_TYPE, frame.predicted_display_time, &stage)?;
            if let Some(browser) = &browser {
                if webxr_views.len() == 2 {
                    browser.publish(dome::webxr_bridge::Tracking {
                        frame_id: count,
                        time: frame.predicted_display_time.as_nanos(),
                        period: frame.predicted_display_period.as_nanos(),
                        should_render: frame.should_render,
                        visible,
                        focused,
                        head_flags: head.location_flags.into_raw(),
                        view_flags: webxr_view_flags.into_raw(),
                        head: bridge_pose(head.pose),
                        views: [0, 1].map(|i| dome::webxr_bridge::View {
                            pose: bridge_pose(webxr_views[i].pose),
                            fov: bridge_fov(webxr_views[i].fov),
                        }),
                    });
                } else {
                    browser.suspend();
                }
            }
            if let Some(mut submitted) = browser
                .as_ref()
                .and_then(|browser| browser.take_submission())
            {
                let copied = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let browser_format =
                        browser_format.ok_or("WebXR requires a linear RGBA8 swapchain format")?;
                    if browser_panel
                        .as_ref()
                        .is_none_or(|p| p.width != submitted.width || p.height != submitted.height)
                    {
                        browser_panel = Some(Panel::new(
                            &session,
                            browser_format,
                            submitted.width,
                            submitted.height,
                        )?);
                    }
                    let image = Image {
                        width: submitted.width,
                        height: submitted.height,
                        rgba: std::mem::take(&mut submitted.rgba),
                        gpu: None,
                        serial: submitted.frame_id,
                    };
                    upload(
                        &vk_instance,
                        &vk_device,
                        vk_physical_device,
                        queue,
                        command,
                        browser_panel.as_mut().unwrap(),
                        &image,
                    )?;
                    Ok(())
                })();
                match copied {
                    Ok(())
                        if browser
                            .as_ref()
                            .is_some_and(|browser| browser.composed(&submitted)) =>
                    {
                        browser_views = Some(submitted.views.clone());
                        browser_layer_flags = submitted.layer_flags;
                        browser_updated = Some(Instant::now());
                        submitted.complete(Ok(()));
                    }
                    Ok(()) => submitted.complete(Err("Browser lease ended during upload".into())),
                    Err(error) => {
                        // A failed resize/upload may have replaced the old
                        // swapchain. Its previous projection rectangles are
                        // no longer safe to submit against the new image.
                        browser_views = None;
                        browser_updated = None;
                        browser_panel = None;
                        eprintln!("Dome WebXR frame rejected: {error}");
                        submitted.complete(Err(error.to_string()));
                    }
                }
            }
            let (scene, mut images, shell, control) = {
                let state = network.lock().unwrap();
                let mut control = state.control.clone();
                if let Some(age) = control.finder_seconds.as_mut() {
                    *age += state.control_received.map_or(0., |t| t.elapsed().as_secs_f32());
                }
                (
                    state.scene.clone(),
                    state.images.clone(),
                    state.shell.clone(),
                    control,
                )
            };
            let mut geometry = vec![];
            let mut layer_opacities = HashMap::new();
            let mut popup_beams = vec![];
            let mut popup_ids = std::collections::HashSet::new();
            let mut stereo_windows = std::collections::HashSet::new();
            if let Some(ref scene) = scene {
                stereo_windows.extend(
                    scene
                        .windows
                        .iter()
                        .filter(|w| w.stereo == dome::stereo::Stereo::SideBySide)
                        .map(|w| w.id),
                );
                presentation = scene.presentation;
                if !presentation_initialized {
                    animator = dome::xr_presentation::Animator::initial(presentation);
                    presentation_initialized = true;
                }
                transition = animator.update(start.elapsed().as_secs_f64(), presentation);
                hidden_frames += u64::from(transition.opacity <= 0.);
                fade_frames += u64::from(transition.opacity > 0. && transition.opacity < 1.);
                slide_frames += u64::from(
                    (transition.top > 0. && transition.top < 1.)
                        || (transition.bottom > 0. && transition.bottom < 1.),
                );
                layer_opacities.extend(scene.panels().into_iter().map(|p| (p.id, p.opacity)));
                popup_ids.extend(scene.popups.iter().map(|p| p.id));
                let hud_scene = scene.clone();
                let views = &webxr_views;
                let horizontal = views
                    .iter()
                    .map(|v| (v.fov.angle_right - v.fov.angle_left).to_degrees())
                    .fold(160f32, f32::min)
                    - 8.;
                let eyes: Vec<_> = views.iter().map(|v| (pose(v.pose), [
                    v.fov.angle_left, v.fov.angle_right, v.fov.angle_down, v.fov.angle_up,
                ])).collect();
                let mouse_viewport = dome::xr_input::shared_viewport(
                    head_pose, &eyes, scene.config.bar_distance_m.min(scene.config.radius_m).max(0.1),
                );
                geometry = dome::xr_input::render_quads_in_fov(scene, head_pose, mouse_viewport);
                popup_beams = dome::spatial_popup::beams(scene, &geometry, head_pose);
                {
                    let mut state = network.lock().unwrap();
                    state.session = dome::xr_transport::Session {
                        id: session_id,
                        visible: visible && head_valid && !exiting,
                        focused,
                        head: head_pose,
                        horizontal_fov_deg: horizontal.max(30.),
                        frame_period_ns: frame.predicted_display_period.as_nanos().max(0) as u64,
                        mouse_viewport,
                    };
                    state.sampled = Some(Instant::now());
                }
                let hud = mouse_viewport.map(|viewport| dome::xr_input::shell_quads_in_fov(
                    &hud_scene, head_pose, &shell, viewport,
                )).unwrap_or_default();
                geometry.extend(hud);
                transition.apply(&mut geometry, head_pose, &hud_scene.config);
            }
            // Only the physical mouse supplies a cursor; the compositor owns its hit UV.
            if control.mouse && focused && head_valid && transition.opacity >= 1. {
                if let (Some(angles), Some(scene)) = (control.mouse_angles, scene.as_ref()) {
                    let cursor = control.cursor.as_ref()
                        .filter(|c| transition.interactive(presentation, c.id));
                    let reticle = dome::xr_input::mouse_cursor_reticle(
                        &geometry, cursor, head_pose, angles, &scene,
                    );
                    let viewport = network.lock().unwrap().session.mouse_viewport;
                    let reticle = if let Some(viewport) = viewport {
                        dome::xr_input::confine_mouse_reticle(reticle, head_pose, viewport)
                    } else {
                        reticle
                    };
                    if let Some(age) = control.finder_seconds.filter(|age| *age < dome::visual_feedback::FINDER_DURATION) {
                        let mut finder = reticle.clone();
                        finder.id = u64::MAX - 1;
                        finder.size = finder.size.map(|v| v * 4.);
                        images.insert(
                            finder.id,
                            Image {
                                width: 128,
                                height: 128,
                                gpu: None,
                                rgba: dome::visual_feedback::finder_pixels(age, control.accent),
                                serial: count + 2,
                            },
                        );
                        geometry.push(finder);
                    }
                    let id = reticle.id;
                    let rgba = dome::xr_input::cursor_pixels(control.light);
                    images.insert(
                        id,
                        Image {
                            width: 32,
                            height: 32,
                            rgba,
                            gpu: None,
                            // Invalidate the uploaded texture on live Appearance changes.
                            serial: 1 + u64::from(control.light),
                        },
                    );
                    geometry.push(reticle);
                }
            }
            for beam in popup_beams.into_iter().filter(|_| transition.opacity > 0.) {
                images.insert(
                    beam.id,
                    Image {
                        width: 1,
                        height: 1,
                        gpu: None,
                        rgba: vec![160, 205, 255, 180],
                        serial: 1,
                    },
                );
                geometry.push(beam);
            }
            // Added after input processing so the floor never captures a pointer.
            if floor_stage_available && floor_bounds_read.elapsed() >= Duration::from_secs(1) {
                floor_bounds_read = Instant::now();
                let bounds = floor_size
                    .or_else(dome::music_floor::configured_bounds)
                    .or_else(|| {
                        session
                            .reference_space_bounds_rect(xr::ReferenceSpaceType::STAGE)
                            .ok()
                            .flatten()
                            .map(|b| [b.width, b.height])
                    });
                if bounds != floor_bounds || !floor_announced {
                    eprintln!("Music floor STAGE bounds: {bounds:?}");
                    floor_announced = true;
                }
                floor_bounds = bounds;
            }
            if let Some(floor) = music_floor.frame(floor_bounds).filter(|_| scene.as_ref().is_none_or(|s| s.xr_fullscreen().is_none())) {
                const FLOOR_ID: u64 = u64::MAX - 32;
                images.insert(
                    FLOOR_ID,
                    Image {
                        width: 128,
                        height: 128,
                        rgba: floor.rgba,
                        gpu: None,
                        serial: floor.serial,
                    },
                );
                geometry.push(dome::xr_input::Quad {
                    id: FLOOR_ID,
                    pose: Pose {
                        position_m: [0., 0.005, 0.],
                        orientation_xyzw: glam::Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
                            .to_array(),
                    },
                    size: floor.size,
                    dome: None,
                });
            }
            if let Some(renderer) = gpu_renderer.as_mut() {
                let ids: Vec<_> = images.iter().filter(|(_,i)| i.gpu.is_some()).map(|(id,_)| *id).collect();
                renderer.retain(&ids);
            }
            // Keep interactive surfaces ahead of background domes when layer limits apply.
            geometry.sort_by_key(|q| {
                let background = scene.as_ref().and_then(|s| s.window(q.id)).filter(|w| {
                    scene
                        .as_ref()
                        .is_some_and(|s| w.workspace != s.active_workspace)
                });
                (
                    background.is_some(),
                    background.map_or(0, |w| {
                        let s = scene.as_ref().unwrap();
                        (w.workspace + s.workspaces.len() - s.active_workspace) % s.workspaces.len()
                    }),
                )
            });
            chains.retain(|id, _| geometry.iter().any(|q| q.id == *id));
            if frame.should_render {
                for q in geometry.iter().take(max_layers) {
                    if let Some(image) = images.get(&q.id) {
                        if chains
                            .get(&q.id)
                            .is_some_and(|p| p.width != image.width || p.height != image.height)
                        {
                            chains.remove(&q.id);
                        }
                        if !chains.contains_key(&q.id) {
                            chains.insert(
                                q.id,
                                Panel::new(&session, format, image.width, image.height)?,
                            );
                        }
                        let p = chains.get_mut(&q.id).unwrap();
                        let fade = (transition.surface_opacity(q.id) * layer_opacities.get(&q.id).copied().unwrap_or(1.)).clamp(0., 1.);
                        let opacity = (fade * 255.).round() as u8;
                        let stereo = scene.as_ref().and_then(|s| s.window(q.id)).is_some_and(|w| w.stereo == dome::stereo::Stereo::SideBySide);
                        let fullscreen = scene.as_ref().and_then(|s| s.xr_fullscreen()).is_some_and(|w| w.id == q.id);
                        let border = if dome::xr_transport::is_shell(q.id) || fullscreen { 0 }
                            else { scene.as_ref().map_or(0, |s| s.config.window_border_px) };
                        let effects = (border, !fullscreen && scene.as_ref().is_some_and(|s| s.focused == Some(q.id)), stereo, control.accent);
                        if p.serial != image.serial || p.opacity != opacity || (image.gpu.is_some() && p.effects != Some(effects)) {
                            if let (Some(frame), Some(renderer)) = (&image.gpu, gpu_renderer.as_mut()) {
                                if let Err(e) = renderer.draw(q.id, frame.clone(), p, queue, command, opacity,
                                    effects.0, effects.1, effects.2, effects.3) {
                                    if transport == "gpu" { return Err(e); }
                                    eprintln!("XR GPU import failed for {}: {e}; using CPU", q.id);
                                    gpu_failed.lock().unwrap().insert(q.id);
                                    network.lock().unwrap().images.remove(&q.id);
                                    renderer.invalidate(q.id);
                                    chains.remove(&q.id);
                                    continue;
                                }
                                gpu_submissions += 1;
                            } else {
                                let mut faded;
                                let uploaded_image = if opacity < 255 {
                                    faded = image.clone();
                                    for channel in &mut faded.rgba {
                                        *channel = ((*channel as u16 * opacity as u16 + 127) / 255) as u8;
                                    }
                                    &faded
                                } else {
                                    image
                                };
                                upload(
                                    &vk_instance,
                                    &vk_device,
                                    vk_physical_device,
                                    queue,
                                    command,
                                    p,
                                    uploaded_image,
                                )?;
                            }
                            p.serial = image.serial;
                            p.effects = Some(effects);
                            p.opacity = opacity;
                            uploaded += 1;
                            if q.id == u64::MAX - 32 && !floor_uploaded {
                                eprintln!("Music floor active: audio-reactive texture submitted to STAGE overlay");
                                floor_uploaded = true;
                            }
                            if let Some(dir) = &capture {
                                if captured.insert(q.id) {
                                    std::fs::create_dir_all(dir)?;
                                    image::save_buffer(
                                        std::path::Path::new(dir)
                                            .join(format!("window-{}.png", q.id)),
                                        &image.rgba,
                                        image.width,
                                        image.height,
                                        image::ColorType::Rgba8,
                                    )?;
                                }
                            }
                        }
                    }
                }
            }
            if geometry.iter().any(|q| q.dome.is_some())
                && !extensions.khr_composition_layer_cylinder
            {
                return Err("Room dome requires XR_KHR_composition_layer_cylinder; enable cylinder layer support in the XR runtime".into());
            }
            let mut visible: Vec<_> = geometry
                .iter()
                .take(max_layers)
                .filter_map(|q| chains.get(&q.id).map(|p| (q, p)))
                .collect();
            let browser_visible = browser
                .as_ref()
                .is_some_and(|browser| browser.content_active(browser_updated))
                && browser_views.is_some()
                && browser_panel.is_some();
            let available_layers = max_layers.saturating_sub(usize::from(browser_visible));
            let mut layer_budget = 0;
            visible.retain(|(q, _)| {
                let cost = if stereo_windows.contains(&q.id) { 2 } else { 1 };
                if layer_budget + cost > available_layers {
                    return false;
                }
                layer_budget += cost;
                true
            });
            visible.sort_by_key(|(q, _)| {
                let group = if q.id == u64::MAX - 32 {
                    0
                } else if dome::xr_transport::is_shell(q.id) || q.id >= u64::MAX - 1 {
                    4
                } else if popup_ids.contains(&q.id) {
                    3
                } else if q.id & dome::spatial_popup::BEAM_ID_BASE != 0 {
                    2
                } else {
                    1
                };
                let order = scene
                    .as_ref()
                    .map(|s| dome::xr_input::window_order(s, q.id))
                    .unwrap_or(1);
                let depth = scene
                    .as_ref()
                    .and_then(|s| {
                        s.window(q.id).map(|w| {
                            (w.workspace + s.workspaces.len() - s.active_workspace)
                                % s.workspaces.len()
                        })
                    })
                    .unwrap_or(0);
                (group, std::cmp::Reverse(depth), order, q.dome.is_none())
            });
            if let Some(id)=control.screenshot_request.filter(|id|*id!=last_screenshot_request) {
                if frame.should_render && webxr_views.len()==2 {
                    last_screenshot_request=id;
                    let eyes=[0,1].map(|i|dome::screenshot::Eye{pose:pose(webxr_views[i].pose),
                        fov:[webxr_views[i].fov.angle_left,webxr_views[i].fov.angle_right,
                            webxr_views[i].fov.angle_down,webxr_views[i].fov.angle_up]});
                    let sources=visible.iter().filter_map(|(q,_)|images.get(&q.id).map(|image|(
                        (*q).clone(),image.clone(),stereo_windows.contains(&q.id),
                        (transition.surface_opacity(q.id)*layer_opacities.get(&q.id).copied().unwrap_or(1.)).clamp(0.,1.)))).collect();
                    let size=[eye_sizes.iter().map(|s|s[0]).max().unwrap_or(1024).min(2048),
                              eye_sizes.iter().map(|s|s[1]).max().unwrap_or(1024).min(2048)];
                    screenshot::capture(screenshot_base.clone(),id,sources,eyes,size,scene.as_ref().map_or(0,|s|s.config.window_border_px),
                        scene.as_ref().filter(|s|s.xr_fullscreen().is_none()).and_then(|s|s.focused),control.accent,browser_visible);
                }
            }
            peak_stereo = peak_stereo.max(
                visible
                    .iter()
                    .filter(|(q, _)| stereo_windows.contains(&q.id))
                    .count(),
            );
            let stage_ref = &stage;
            let layers: Vec<_> = visible
                .iter()
                .flat_map(|(q, p)| {
                    let stereo = if stereo_windows.contains(&q.id) {
                        dome::stereo::Stereo::SideBySide
                    } else {
                        dome::stereo::Stereo::Mono
                    };
                    stereo.regions(p.width).into_iter().map(move |region| {
                        let eye = match region.eye {
                            dome::stereo::Eye::Both => xr::EyeVisibility::BOTH,
                            dome::stereo::Eye::Left => xr::EyeVisibility::LEFT,
                            dome::stereo::Eye::Right => xr::EyeVisibility::RIGHT,
                        };
                        let sub = xr::SwapchainSubImage::new()
                            .swapchain(&p.chain)
                            .image_array_index(0)
                            .image_rect(xr::Rect2Di {
                                offset: xr::Offset2Di {
                                    x: region.x as _,
                                    y: 0,
                                },
                                extent: xr::Extent2Di {
                                    width: region.width as _,
                                    height: p.height as _,
                                },
                            });
                        if let Some(d) = &q.dome {
                            Layer::Dome(
                                xr::CompositionLayerCylinderKHR::new()
                                    .space(stage_ref)
                                    .eye_visibility(eye)
                                    .layer_flags(
                                        xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA,
                                    )
                                    .sub_image(sub)
                                    .pose(xr_pose(d.layer_pose()))
                                    .radius(d.radius)
                                    .central_angle(d.horizontal)
                                    .aspect_ratio(d.aspect_ratio()),
                            )
                        } else {
                            Layer::Flat(
                                xr::CompositionLayerQuad::new()
                                    .space(stage_ref)
                                    .eye_visibility(eye)
                                    .layer_flags(
                                        xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA,
                                    )
                                    .pose(xr_pose(q.pose))
                                    .size(xr::Extent2Df {
                                        width: q.size[0],
                                        height: q.size[1],
                                    })
                                    .sub_image(sub),
                            )
                        }
                    })
                })
                .collect();
            peak = peak.max(
                visible
                    .iter()
                    .filter(|(q, _)| !dome::xr_transport::is_shell(q.id) && q.id < u64::MAX - 1)
                    .count(),
            );
            peak_shell = peak_shell.max(
                visible
                    .iter()
                    .filter(|(q, _)| dome::xr_transport::is_shell(q.id))
                    .count(),
            );
            peak_curved = peak_curved.max(
                visible
                    .iter()
                    .filter(|(q, _)| q.dome.is_some() && !dome::xr_transport::is_shell(q.id))
                    .count(),
            );
            peak_flat = peak_flat.max(
                visible
                    .iter()
                    .filter(|(q, _)| q.dome.is_none() && !dome::xr_transport::is_shell(q.id))
                    .count(),
            );
            let projection_views: Option<Vec<_>> = if browser_visible {
                let panel = browser_panel.as_ref().unwrap();
                Some(
                    browser_views
                        .as_ref()
                        .unwrap()
                        .iter()
                        .map(|v| {
                            xr::CompositionLayerProjectionView::new()
                                .pose(from_bridge_pose(v.pose))
                                .fov(from_bridge_fov(v.fov))
                                .sub_image(
                                    xr::SwapchainSubImage::new()
                                        .swapchain(&panel.chain)
                                        .image_array_index(0)
                                        .image_rect(xr::Rect2Di {
                                            offset: xr::Offset2Di {
                                                x: v.rect.x as i32,
                                                y: v.rect.y as i32,
                                            },
                                            extent: xr::Extent2Di {
                                                width: v.rect.width as i32,
                                                height: v.rect.height as i32,
                                            },
                                        }),
                                )
                        })
                        .collect(),
                )
            } else {
                None
            };
            let projection = projection_views.as_ref().map(|views| {
                xr::CompositionLayerProjection::new()
                    .space(&stage)
                    .layer_flags(xr::CompositionLayerFlags::from_raw(browser_layer_flags))
                    .views(views)
            });
            let mut refs: Vec<&xr::CompositionLayerBase<xr::Vulkan>> = Vec::new();
            if frame.should_render {
                if let Some(layer) = projection.as_ref() {
                    refs.push(&**layer);
                }
                refs.extend(layers.iter().map(|l| match l {
                    Layer::Flat(q) => &**q,
                    Layer::Dome(d) => &**d,
                }));
            }
            stream.end(frame.predicted_display_time, blend, &refs)?;
        }
        if let Some(browser) = &browser {
            browser.suspend();
        }
        running.store(false, Ordering::Relaxed);
        let _ = worker.join();
        let _ = input_worker.join();
        let summary = serde_json::json!({"runtime":runtime,"system":properties.system_name,"overlay":environment.overlay,"environment_blend":format!("{blend:?}"),"overlay_placement":placement,"frames":count,"peak_window_layers":peak,"peak_curved_layers":peak_curved,"peak_flat_layers":peak_flat,"peak_stereo_windows":peak_stereo,"peak_shell_layers":peak_shell,"uploads":uploaded,"gpu_submissions":gpu_submissions,"input_methods":["mouse","keyboard"],"presentation_hidden_frames":hidden_frames,"presentation_fade_frames":fade_frames,"presentation_slide_frames":slide_frames,"clean_exit":true,"webxr":browser.as_ref().map(|browser| browser.stats()).unwrap_or_default()});
        println!("{summary}");
        if let Some(path) = report {
            std::fs::write(path, serde_json::to_vec_pretty(&summary)?)?;
        }
        vk_device.device_wait_idle()?;
        drop(gpu_renderer);
        drop(chains);
        drop(browser_panel);
        drop(browser);
        drop((stage, view, stream, waiter, session));
        vk_device.destroy_command_pool(pool, None);
        vk_device.destroy_device(None);
        vk_instance.destroy_instance(None);
        if let Some(error) = network.lock().unwrap().gpu_error.take() { return Err(error.into()); }
    }
    Ok(())
}
fn pose(p: xr::Posef) -> Pose {
    Pose {
        position_m: [p.position.x, p.position.y, p.position.z],
        orientation_xyzw: [
            p.orientation.x,
            p.orientation.y,
            p.orientation.z,
            p.orientation.w,
        ],
    }
}
fn xr_pose(p: Pose) -> xr::Posef {
    xr::Posef {
        position: xr::Vector3f {
            x: p.position_m[0],
            y: p.position_m[1],
            z: p.position_m[2],
        },
        orientation: xr::Quaternionf {
            x: p.orientation_xyzw[0],
            y: p.orientation_xyzw[1],
            z: p.orientation_xyzw[2],
            w: p.orientation_xyzw[3],
        },
    }
}
struct Panel {
    chain: xr::Swapchain<xr::Vulkan>,
    images: Vec<vk::Image>,
    effects: Option<(u8, bool, bool, [u8;3])>,
    width: u32,
    height: u32,
    serial: u64,
    opacity: u8,
}
impl Panel {
    fn new(
        s: &xr::Session<xr::Vulkan>,
        format: vk::Format,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let chain = s.create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                | xr::SwapchainUsageFlags::SAMPLED
                | xr::SwapchainUsageFlags::TRANSFER_DST,
            format: format.as_raw() as _,
            sample_count: 1,
            width,
            height,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        })?;
        let images = chain
            .enumerate_images()?
            .into_iter()
            .map(vk::Image::from_raw)
            .collect();
        Ok(Self {
            chain,
            images,
            effects: None,
            width,
            height,
            serial: 0,
            opacity: 255,
        })
    }
}
unsafe fn upload(
    instance: &ash::Instance,
    device: &ash::Device,
    physical: vk::PhysicalDevice,
    queue: vk::Queue,
    command: vk::CommandBuffer,
    panel: &mut Panel,
    image: &Image,
) -> Result<(), Box<dyn std::error::Error>> {
    let size = image.rgba.len() as u64;
    let buffer = device.create_buffer(
        &vk::BufferCreateInfo::default()
            .size(size)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC),
        None,
    )?;
    let req = device.get_buffer_memory_requirements(buffer);
    let props = instance.get_physical_device_memory_properties(physical);
    let kind = (0..props.memory_type_count)
        .find(|i| {
            req.memory_type_bits & (1 << i) != 0
                && props.memory_types[*i as usize].property_flags.contains(
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )
        })
        .ok_or("no coherent staging memory")?;
    let memory = device.allocate_memory(
        &vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(kind),
        None,
    )?;
    device.bind_buffer_memory(buffer, memory, 0)?;
    let map = device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())?;
    std::ptr::copy_nonoverlapping(image.rgba.as_ptr(), map as *mut u8, image.rgba.len());
    device.unmap_memory(memory);
    let index = panel.chain.acquire_image()?;
    panel.chain.wait_image(xr::Duration::INFINITE)?;
    device.reset_command_buffer(command, vk::CommandBufferResetFlags::empty())?;
    device.begin_command_buffer(
        command,
        &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
    )?;
    let range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1);
    let target = panel.images[index as usize];
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .image(target)
        .subresource_range(range)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
    device.cmd_pipeline_barrier(
        command,
        vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::PipelineStageFlags::TRANSFER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier],
    );
    let region = vk::BufferImageCopy::default()
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1),
        )
        .image_extent(vk::Extent3D {
            width: image.width,
            height: image.height,
            depth: 1,
        });
    device.cmd_copy_buffer_to_image(
        command,
        buffer,
        target,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        &[region],
    );
    let barrier = barrier
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_READ);
    device.cmd_pipeline_barrier(
        command,
        vk::PipelineStageFlags::TRANSFER,
        vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier],
    );
    device.end_command_buffer(command)?;
    device.queue_submit(
        queue,
        &[vk::SubmitInfo::default().command_buffers(&[command])],
        vk::Fence::null(),
    )?;
    device.queue_wait_idle(queue)?;
    panel.chain.release_image()?;
    device.destroy_buffer(buffer, None);
    device.free_memory(memory, None);
    Ok(())
}

enum Layer<'a> {
    Flat(xr::CompositionLayerQuad<'a, xr::Vulkan>),
    Dome(xr::CompositionLayerCylinderKHR<'a, xr::Vulkan>),
}

fn bridge_pose(p: xr::Posef) -> dome::webxr_bridge::Pose {
    dome::webxr_bridge::Pose {
        position: [p.position.x, p.position.y, p.position.z],
        orientation: [
            p.orientation.x,
            p.orientation.y,
            p.orientation.z,
            p.orientation.w,
        ],
    }
}
fn from_bridge_pose(p: dome::webxr_bridge::Pose) -> xr::Posef {
    xr::Posef {
        position: xr::Vector3f {
            x: p.position[0],
            y: p.position[1],
            z: p.position[2],
        },
        orientation: xr::Quaternionf {
            x: p.orientation[0],
            y: p.orientation[1],
            z: p.orientation[2],
            w: p.orientation[3],
        },
    }
}
fn bridge_fov(f: xr::Fovf) -> dome::webxr_bridge::Fov {
    dome::webxr_bridge::Fov {
        left: f.angle_left,
        right: f.angle_right,
        up: f.angle_up,
        down: f.angle_down,
    }
}
fn from_bridge_fov(f: dome::webxr_bridge::Fov) -> xr::Fovf {
    xr::Fovf {
        angle_left: f.left,
        angle_right: f.right,
        angle_up: f.up,
        angle_down: f.down,
    }
}
