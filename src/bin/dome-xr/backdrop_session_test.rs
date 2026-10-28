//! API-level fixture: no loader, graphics device, display or headset is opened.
use super::*;
use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::{c_char, CStr},
};
use xr::sys::{self, Handle};

#[derive(Default)]
struct Runtime {
    instances: u64,
    placements: Vec<(sys::Instance, u32)>,
    events: VecDeque<xr::SessionState>,
    begun: u32,
    ended: u32,
    destroyed: u32,
    reject: bool,
}
thread_local! { static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default()); }

unsafe extern "system" fn create_instance(
    _: *const sys::InstanceCreateInfo,
    out: *mut sys::Instance,
) -> sys::Result {
    RUNTIME.with_borrow_mut(|r| {
        r.instances += 1;
        *out = sys::Instance::from_raw(r.instances);
    });
    sys::Result::SUCCESS
}
unsafe extern "system" fn destroy_instance(_: sys::Instance) -> sys::Result {
    sys::Result::SUCCESS
}
unsafe extern "system" fn get_system(
    _: sys::Instance,
    _: *const sys::SystemGetInfo,
    out: *mut sys::SystemId,
) -> sys::Result {
    *out = sys::SystemId::from_raw(1);
    sys::Result::SUCCESS
}
unsafe extern "system" fn requirements(
    _: sys::Instance,
    _: sys::SystemId,
    out: *mut sys::GraphicsRequirementsVulkanKHR,
) -> sys::Result {
    (*out).min_api_version_supported = xr::Version::new(1, 1, 0);
    (*out).max_api_version_supported = xr::Version::new(1, 3, 0);
    sys::Result::SUCCESS
}
unsafe extern "system" fn physical(
    _: sys::Instance,
    _: *const sys::VulkanGraphicsDeviceGetInfoKHR,
    out: *mut sys::platform::VkPhysicalDevice,
) -> sys::Result {
    *out = 12usize as _;
    sys::Result::SUCCESS
}
unsafe extern "system" fn blends(
    _: sys::Instance,
    _: sys::SystemId,
    _: xr::ViewConfigurationType,
    capacity: u32,
    count: *mut u32,
    out: *mut xr::EnvironmentBlendMode,
) -> sys::Result {
    *count = 1;
    if capacity > 0 {
        *out = xr::EnvironmentBlendMode::OPAQUE;
    }
    sys::Result::SUCCESS
}
unsafe extern "system" fn create_session(
    instance: sys::Instance,
    info: *const sys::SessionCreateInfo,
    out: *mut sys::Session,
) -> sys::Result {
    let binding = &*((*info).next as *const sys::GraphicsBindingVulkanKHR);
    if binding.ty != sys::GraphicsBindingVulkanKHR::TYPE || binding.next.is_null() {
        return sys::Result::ERROR_VALIDATION_FAILURE;
    }
    let overlay = &*(binding.next as *const sys::SessionCreateInfoOverlayEXTX);
    RUNTIME.with_borrow_mut(|r| {
        if r.reject || r.placements.iter().any(|(i, _)| *i == instance) {
            return sys::Result::ERROR_LIMIT_REACHED;
        }
        r.placements
            .push((instance, overlay.session_layers_placement));
        *out = sys::Session::from_raw(instance.into_raw());
        sys::Result::SUCCESS
    })
}
unsafe extern "system" fn destroy_session(_: sys::Session) -> sys::Result {
    RUNTIME.with_borrow_mut(|r| r.destroyed += 1);
    sys::Result::SUCCESS
}
unsafe extern "system" fn formats(
    _: sys::Session,
    capacity: u32,
    count: *mut u32,
    out: *mut i64,
) -> sys::Result {
    *count = 1;
    if capacity > 0 {
        *out = vk::Format::R8G8B8A8_SRGB.as_raw() as i64;
    }
    sys::Result::SUCCESS
}
unsafe extern "system" fn space(
    _: sys::Session,
    _: *const sys::ReferenceSpaceCreateInfo,
    out: *mut sys::Space,
) -> sys::Result {
    *out = sys::Space::from_raw(1);
    sys::Result::SUCCESS
}
unsafe extern "system" fn destroy_space(_: sys::Space) -> sys::Result {
    sys::Result::SUCCESS
}
unsafe extern "system" fn view_sizes(
    _: sys::Instance,
    _: sys::SystemId,
    _: xr::ViewConfigurationType,
    capacity: u32,
    count: *mut u32,
    out: *mut sys::ViewConfigurationView,
) -> sys::Result {
    *count = 2;
    if capacity >= 2 {
        for i in 0..2 {
            (*out.add(i)).recommended_image_rect_width = 2048;
            (*out.add(i)).recommended_image_rect_height = 1024;
        }
    }
    sys::Result::SUCCESS
}
unsafe extern "system" fn poll(
    instance: sys::Instance,
    out: *mut sys::EventDataBuffer,
) -> sys::Result {
    RUNTIME.with_borrow_mut(|r| {
        if let Some(state) = r.events.pop_front() {
            out.cast::<sys::EventDataSessionStateChanged>().write(
                sys::EventDataSessionStateChanged {
                    ty: sys::EventDataSessionStateChanged::TYPE,
                    next: std::ptr::null(),
                    session: sys::Session::from_raw(instance.into_raw()),
                    state,
                    time: xr::Time::from_nanos(1),
                },
            );
            sys::Result::SUCCESS
        } else {
            sys::Result::EVENT_UNAVAILABLE
        }
    })
}
unsafe extern "system" fn begin(_: sys::Session, _: *const sys::SessionBeginInfo) -> sys::Result {
    RUNTIME.with_borrow_mut(|r| r.begun += 1);
    sys::Result::SUCCESS
}
unsafe extern "system" fn end(_: sys::Session) -> sys::Result {
    RUNTIME.with_borrow_mut(|r| r.ended += 1);
    sys::Result::SUCCESS
}
// Required dispatch entries that are loaded but never invoked by this fixture.
unsafe extern "system" fn unused() -> sys::Result {
    sys::Result::ERROR_FUNCTION_UNSUPPORTED
}
unsafe extern "system" fn get_proc(
    _: sys::Instance,
    name: *const c_char,
    out: *mut Option<sys::pfn::VoidFunction>,
) -> sys::Result {
    let function = match CStr::from_ptr(name).to_bytes() {
        b"xrCreateInstance" => create_instance as *const (),
        b"xrDestroyInstance" => destroy_instance as *const (),
        b"xrGetSystem" => get_system as *const (),
        b"xrGetVulkanGraphicsRequirements2KHR" => requirements as *const (),
        b"xrGetVulkanGraphicsDevice2KHR" => physical as *const (),
        b"xrEnumerateEnvironmentBlendModes" => blends as *const (),
        b"xrCreateSession" => create_session as *const (),
        b"xrDestroySession" => destroy_session as *const (),
        b"xrEnumerateSwapchainFormats" => formats as *const (),
        b"xrCreateReferenceSpace" => space as *const (),
        b"xrDestroySpace" => destroy_space as *const (),
        b"xrEnumerateViewConfigurationViews" => view_sizes as *const (),
        b"xrPollEvent" => poll as *const (),
        b"xrBeginSession" => begin as *const (),
        b"xrEndSession" => end as *const (),
        _ => unused as *const (),
    };
    *out = Some(std::mem::transmute(function));
    sys::Result::SUCCESS
}

#[test]
fn separate_background_session_preserves_desktop_placement_and_lifecycle() -> Result<()> {
    unsafe {
        RUNTIME.with_borrow_mut(|r| *r = Runtime::default());
        let entry = xr::Entry::from_get_instance_proc_addr(get_proc)?;
        let mut extensions = xr::ExtensionSet::default();
        extensions.extx_overlay = true;
        extensions.khr_vulkan_enable2 = true;
        let desktop = entry.create_instance(&xr::ApplicationInfo::default(), &extensions, &[])?;
        let graphics = xr::vulkan::SessionCreateInfo {
            instance: 11usize as _,
            physical_device: 12usize as _,
            device: 13usize as _,
            queue_family_index: 4,
            queue_index: 0,
        };
        let (session, waiter, stream) = dome::xr_environment::create_session(
            &desktop,
            sys::SystemId::from_raw(1),
            &graphics,
            true,
            10,
        )?;
        let mut background = Background::new(
            &entry,
            &graphics,
            vk::Format::R8G8B8A8_SRGB,
            xr::ReferenceSpaceType::LOCAL,
        )?;
        assert_ne!(desktop.as_raw(), background.instance.as_raw());
        RUNTIME.with_borrow(|r| {
            assert_eq!(
                r.placements,
                [(desktop.as_raw(), 10), (background.instance.as_raw(), 0)]
            );
        });
        assert_eq!(background.sizes, [[1536, 768], [1536, 768]]);
        RUNTIME.with_borrow_mut(|r| {
            r.events
                .extend([xr::SessionState::READY, xr::SessionState::VISIBLE])
        });
        background.poll()?;
        assert!(background.active && background.visible);
        RUNTIME.with_borrow_mut(|r| r.events.push_back(xr::SessionState::STOPPING));
        background.poll()?;
        assert!(!background.active && !background.visible);
        RUNTIME.with_borrow_mut(|r| {
            r.events
                .extend([xr::SessionState::READY, xr::SessionState::FOCUSED])
        });
        background.poll()?;
        assert!(background.active && background.visible);
        RUNTIME.with_borrow_mut(|r| r.events.push_back(xr::SessionState::LOSS_PENDING));
        background.poll()?;
        assert!(background.lost && !background.visible);
        drop(background);
        // Runtime rejection is propagated, never replaced with a high-placement
        // background session which would cover WayVR again.
        RUNTIME.with_borrow_mut(|r| r.reject = true);
        assert!(Background::new(
            &entry,
            &graphics,
            vk::Format::R8G8B8A8_SRGB,
            xr::ReferenceSpaceType::LOCAL
        )
        .is_err());
        drop((stream, waiter, session, desktop));
        RUNTIME.with_borrow(|r| {
            assert_eq!(r.begun, 2);
            assert_eq!(r.ended, 1);
            assert_eq!(r.destroyed, 2);
        });
    }
    Ok(())
}
