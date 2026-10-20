//! Native OpenXR quad compositor. Window content and seat input remain owned by Smithay.
use ash::vk::{self, Handle};
use dome::{
    xr_input::{Detector, Hand, Interaction, Packet},
    Pose, Scene,
};
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
const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
#[derive(Clone)]
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    serial: u64,
}
#[derive(Default)]
struct Network {
    scene: Option<Scene>,
    session: dome::xr_transport::Session,
    sampled: Option<Instant>,
    control: dome::xr_transport::Control,
    shell: dome::xr_transport::ShellLayout,
    images: HashMap<u64, Image>,
    out: std::collections::VecDeque<Packet>,
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
    let mut placement = 10u32;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--server" => base = args.next().ok_or("missing server")?,
            "--frames" => frames = args.next().ok_or("missing frames")?.parse()?,
            "--capture" => capture = args.next(),
            "--report" => report = args.next(),
            "--standalone" => standalone = true,
            "--placement" => {
                placement = args.next().ok_or("missing placement")?.parse()?;
                if placement == 0 {
                    return Err("overlay placement must be positive".into());
                }
            }
            "--help" => {
                println!("dome-xr [--server http://127.0.0.1:PORT] [--frames N] [--capture DIR] [--report FILE] [--standalone] [--placement N]\nDefault: transparent overlay, or passthrough blending when overlay sessions are unavailable.");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {a}").into()),
        }
    }
    if !base.starts_with("http://127.0.0.1:") || base[17..].parse::<u16>().is_err() {
        return Err("server must be numeric loopback http://127.0.0.1:PORT".into());
    }
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
    let n = network.clone();
    let r = running.clone();
    let worker = std::thread::spawn(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(250))
            .build();
        let mut serial = 0;
        let mut last_server = Instant::now();
        let mut last_heartbeat = Instant::now() - Duration::from_secs(1);
        while r.load(Ordering::Relaxed) {
            if last_heartbeat.elapsed() >= Duration::from_millis(100) {
                let mut status = {
                    let state = n.lock().unwrap();
                    let mut status = state.session.clone();
                    status.visible &= state
                        .sampled
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(750));
                    status
                };
                if !r.load(Ordering::Relaxed) {
                    status.visible = false;
                }
                let _ = agent
                    .post(&format!("{base}/api/xr-session"))
                    .send_json(serde_json::to_value(status).unwrap());
                if let Ok(response) = agent.get(&format!("{base}/api/xr-control")).call() {
                    if let Ok(control) = response.into_json::<dome::xr_transport::Control>() {
                        n.lock().unwrap().control = control;
                    }
                }
                last_heartbeat = Instant::now();
            }
            let packets: Vec<_> = n.lock().unwrap().out.drain(..).collect();
            for p in packets {
                if agent
                    .post(&format!("{base}/api/xr-input"))
                    .send_json(serde_json::to_value(p).unwrap())
                    .is_err()
                {
                    n.lock().unwrap().out.clear();
                    let _ = agent.post(&format!("{base}/api/xr-input")).send_json(
                        serde_json::to_value(Packet {
                            events: vec![dome::xr_input::SeatEvent::Reset],
                            actions: vec![],
                        })
                        .unwrap(),
                    );
                }
            }
            if let Ok(response) = agent.get(&format!("{base}/api/state")).call() {
                last_server = Instant::now();
                if let Ok(value) = response.into_json::<serde_json::Value>() {
                    if let Ok(scene) = serde_json::from_value::<Scene>(value["scene"].clone()) {
                        let mut ids: Vec<_> = scene
                            .windows
                            .iter()
                            .filter(|w| w.workspace == scene.active_workspace && !w.minimized)
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
                            if let Ok(response) =
                                agent.get(&format!("{base}/api/xr-frame/{id}")).call()
                            {
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
                                        let stereo = n.lock().unwrap().scene.as_ref().and_then(|s| s.window(id)).is_some_and(|w| w.stereo == dome::stereo::Stereo::SideBySide);
                                        if !dome::xr_transport::is_shell(id) && !stereo {
                                            dome::xr_input::paint_border(
                                                &mut rgba, width, height, border,
                                            );
                                        }
                                        serial += 1;
                                        n.lock().unwrap().images.insert(
                                            id,
                                            Image {
                                                width,
                                                height,
                                                rgba,
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
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = agent.post(&format!("{base}/api/xr-session")).send_json(
            serde_json::to_value(dome::xr_transport::Session {
                id: session_id,
                ..Default::default()
            })
            .unwrap(),
        );
        let _ = agent.post(&format!("{base}/api/xr-input")).send_json(
            serde_json::to_value(Packet {
                events: vec![dome::xr_input::SeatEvent::Reset],
                actions: vec![],
            })
            .unwrap(),
        );
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
    extensions.ext_hand_tracking = available.ext_hand_tracking;
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

        let vk_device = {
            let vk_device = xr_instance
                .create_vulkan_device(
                    system,
                    std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                    vk_physical_device.as_raw() as _,
                    &vk::DeviceCreateInfo::default()
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
        let stage = session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
            .or_else(|_| {
                session.create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            })?;
        let view =
            session.create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)?;
        let trackers =
            if extensions.ext_hand_tracking && xr_instance.supports_hand_tracking(system)? {
                [
                    session.create_hand_tracker(xr::Hand::LEFT).ok(),
                    session.create_hand_tracker(xr::Hand::RIGHT).ok(),
                ]
            } else {
                [None, None]
            };
        let mut detectors = [Detector::default(), Detector::default()];
        let mut interaction = Interaction::default();
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
            let (scene, mut images, shell, control) = {
                let state = network.lock().unwrap();
                (
                    state.scene.clone(),
                    state.images.clone(),
                    state.shell.clone(),
                    state.control.clone(),
                )
            };
            let mut hands = [Hand::default(); 2];
            for i in 0..2 {
                if let Some(t) = &trackers[i] {
                    let joints = stage.locate_hand_joints(t, frame.predicted_display_time)?;
                    let tracked = joints.as_ref().is_some_and(|j| {
                        j.iter().all(|p| {
                            p.location_flags
                                .contains(xr::SpaceLocationFlags::POSITION_VALID)
                        }) && j[1]
                            .location_flags
                            .contains(xr::SpaceLocationFlags::POSITION_TRACKED)
                    });
                    hands[i] = detectors[i].sample(
                        start.elapsed().as_secs_f64(),
                        joints.map(|j| {
                            std::array::from_fn(|k| {
                                [
                                    j[k].pose.position.x,
                                    j[k].pose.position.y,
                                    j[k].pose.position.z,
                                ]
                            })
                        }),
                        tracked,
                    );
                }
            }
            let mut geometry = vec![];
            let mut popup_beams = vec![];
            let mut popup_ids = std::collections::HashSet::new();
            let mut stereo_windows = std::collections::HashSet::new();
            if let Some(scene) = scene {
                stereo_windows.extend(scene.windows.iter().filter(|w| w.stereo == dome::stereo::Stereo::SideBySide).map(|w| w.id));
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
                geometry = dome::xr_input::quads(&scene, head_pose);
                popup_ids.extend(scene.popups.iter().map(|p| p.id));
                popup_beams = dome::spatial_popup::beams(&scene, &geometry, head_pose);
                let mut hud_scene = scene.clone();
                let (_, views) =
                    session.locate_views(VIEW_TYPE, frame.predicted_display_time, &stage)?;
                let horizontal = views
                    .iter()
                    .map(|v| (v.fov.angle_right - v.fov.angle_left).to_degrees())
                    .fold(160f32, f32::min)
                    - 8.;
                hud_scene.config.upper_bar_max_width_deg = hud_scene
                    .config
                    .upper_bar_max_width_deg
                    .min(horizontal.max(30.));
                hud_scene.config.taskbar_max_width_deg = hud_scene
                    .config
                    .taskbar_max_width_deg
                    .min(horizontal.max(30.));
                {
                    let mut state = network.lock().unwrap();
                    state.session = dome::xr_transport::Session {
                        id: session_id,
                        visible: visible && head_valid && !exiting,
                        focused,
                        head: head_pose,
                        horizontal_fov_deg: horizontal.max(30.),
                    };
                    state.sampled = Some(Instant::now());
                }
                let hud = dome::xr_input::shell_quads(&hud_scene, head_pose, &shell);
                let interactive_hud: Vec<_> = hud
                    .iter()
                    .filter(|q| transition.interactive(presentation, q.id))
                    .cloned()
                    .collect();
                let p = interaction.update_surfaces(
                    start.elapsed().as_secs_f64(),
                    focused && head_valid && !exiting && !control.mouse && transition.opacity >= 1.,
                    hands,
                    &scene,
                    head_pose,
                    &interactive_hud,
                );
                geometry.extend(hud);
                transition.apply(&mut geometry, head_pose, &hud_scene.config);
                let mut state = network.lock().unwrap();
                if state.out.len() >= 8 {
                    state.out.clear();
                    state.out.push_back(interaction.reset());
                } else {
                    state.out.push_back(p);
                }
            }
            // Mouse and hand cursors follow the hit surface; empty space has a view cursor.
            // Tracked hands continue to use physical ray intersections.
            let cursor_hands = if control.mouse && focused && head_valid && transition.opacity >= 1.
            {
                let mut cursor = [Hand::default(); 2];
                if let Some(angles) = control.mouse_angles {
                    cursor[0].valid = true;
                    cursor[0].position = head_pose.position_m;
                    cursor[0].ray = dome::xr_input::mouse_ray(head_pose, angles).to_array();
                }
                cursor
            } else if !control.mouse && focused && head_valid {
                hands
            } else {
                [Hand::default(); 2]
            };
            for (i, hand) in cursor_hands.iter().enumerate().filter(|(_, h)| h.valid) {
                let hit = geometry
                    .iter()
                    .filter(|q| transition.interactive(presentation, q.id))
                    .filter_map(|q| {
                        q.hit(
                            glam::Vec3::from_array(hand.position),
                            glam::Vec3::from_array(hand.ray),
                            false,
                        )
                        .map(|uv| (q, uv))
                    })
                    .min_by(|(a, ua), (b, ub)| {
                        a.point(*ua)
                            .distance_squared(glam::Vec3::from_array(hand.position))
                            .total_cmp(
                                &b.point(*ub)
                                    .distance_squared(glam::Vec3::from_array(hand.position)),
                            )
                    });
                let reticle = hit
                    .map(|(q, uv)| dome::xr_input::surface_reticle(q, uv, u64::MAX - i as u64))
                    .or_else(|| if control.mouse {
                        control.mouse_angles.map(|angles| dome::xr_input::mouse_reticle(head_pose, angles))
                    } else { None });
                if let Some(reticle) = reticle {
                    let id = reticle.id;
                    let pressed = hand.grip || hand.pinch.iter().any(|p| *p);
                    let mut rgba = vec![0; 16 * 16 * 4];
                    for y in 0..16 {
                        for x in 0..16 {
                            let d = (x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2);
                            if (20. ..=50.).contains(&d) || x == 7 || y == 7 {
                                rgba[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].copy_from_slice(
                                    if pressed {
                                        &[255, 190, 40, 255]
                                    } else {
                                        &[245, 245, 245, 255]
                                    },
                                );
                            }
                        }
                    }
                    images.insert(
                        id,
                        Image {
                            width: 16,
                            height: 16,
                            rgba,
                            serial: if pressed { 2 } else { 1 },
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
                        rgba: vec![160, 205, 255, 180],
                        serial: 1,
                    },
                );
                geometry.push(beam);
            }
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
                        let opacity = (transition.opacity.clamp(0., 1.) * 255.).round() as u8;
                        if p.serial != image.serial || p.opacity != opacity {
                            let mut faded;
                            let uploaded_image = if opacity < 255 {
                                faded = image.clone();
                                transition.fade_rgba(&mut faded.rgba);
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
                            p.serial = image.serial;
                            p.opacity = opacity;
                            uploaded += 1;
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
            visible.sort_by_key(|(q, _)| {
                let group = if dome::xr_transport::is_shell(q.id) || q.id >= u64::MAX - 1 {
                    3
                } else if popup_ids.contains(&q.id) {
                    2
                } else if q.id & dome::spatial_popup::BEAM_ID_BASE != 0 {
                    1
                } else {
                    0
                };
                (group, q.dome.is_none())
            });
            let mut layer_budget = 0;
            visible.retain(|(q, _)| {
                let cost = if stereo_windows.contains(&q.id) { 2 } else { 1 };
                if layer_budget + cost > max_layers { return false; }
                layer_budget += cost; true
            });
            peak_stereo = peak_stereo.max(visible.iter().filter(|(q, _)| stereo_windows.contains(&q.id)).count());
            let stage_ref = &stage;
            let layers: Vec<_> = visible
                .iter()
                .flat_map(|(q, p)| {
                    let stereo = if stereo_windows.contains(&q.id) { dome::stereo::Stereo::SideBySide } else { dome::stereo::Stereo::Mono };
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
                            offset: xr::Offset2Di { x: region.x as _, y: 0 },
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
                                .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
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
                                .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
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
            peak_curved = peak_curved.max(visible.iter().filter(|(q, _)| q.dome.is_some()).count());
            peak_flat = peak_flat.max(
                visible
                    .iter()
                    .filter(|(q, _)| q.dome.is_none() && !dome::xr_transport::is_shell(q.id))
                    .count(),
            );
            let refs: Vec<&xr::CompositionLayerBase<xr::Vulkan>> = if frame.should_render {
                layers
                    .iter()
                    .map(|l| match l {
                        Layer::Flat(q) => &**q,
                        Layer::Dome(d) => &**d,
                    })
                    .collect()
            } else {
                vec![]
            };
            stream.end(frame.predicted_display_time, blend, &refs)?;
        }
        running.store(false, Ordering::Relaxed);
        let _ = worker.join();
        let summary = serde_json::json!({"runtime":runtime,"system":properties.system_name,"overlay":environment.overlay,"environment_blend":format!("{blend:?}"),"overlay_placement":placement,"frames":count,"peak_window_layers":peak,"peak_curved_layers":peak_curved,"peak_flat_layers":peak_flat,"peak_stereo_windows":peak_stereo,"peak_shell_layers":peak_shell,"uploads":uploaded,"hand_tracking_extension":extensions.ext_hand_tracking,"hand_trackers":trackers.iter().filter(|t|t.is_some()).count(),"presentation_hidden_frames":hidden_frames,"presentation_fade_frames":fade_frames,"presentation_slide_frames":slide_frames,"clean_exit":true});
        println!("{summary}");
        if let Some(path) = report {
            std::fs::write(path, serde_json::to_vec_pretty(&summary)?)?;
        }
        vk_device.device_wait_idle()?;
        drop(chains);
        drop(trackers);
        drop((stage, view, stream, waiter, session));
        vk_device.destroy_command_pool(pool, None);
        vk_device.destroy_device(None);
        vk_instance.destroy_instance(None);
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
