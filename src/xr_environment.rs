//! Transparent XR presentation without replacing the main application's session.
use openxr as xr;

#[derive(Clone, Copy, Debug)]
pub struct Environment {
    pub overlay: bool,
    pub blend: xr::EnvironmentBlendMode,
}

pub fn select(
    overlay_available: bool,
    modes: &[xr::EnvironmentBlendMode],
    standalone: bool,
) -> Result<Environment, &'static str> {
    let overlay = overlay_available && !standalone;
    let blend = [
        xr::EnvironmentBlendMode::ALPHA_BLEND,
        xr::EnvironmentBlendMode::ADDITIVE,
        xr::EnvironmentBlendMode::OPAQUE,
    ]
    .into_iter()
    .find(|mode| modes.contains(mode))
    .ok_or("Runtime reports no supported environment blend mode")?;
    if !overlay && blend == xr::EnvironmentBlendMode::OPAQUE && !standalone {
        return Err("Transparent Dome XR requires XR_EXTX_overlay or an alpha/additive environment blend mode; --standalone explicitly permits an opaque diagnostic session");
    }
    Ok(Environment { overlay, blend })
}

/// Keep all next-chain structures alive until xrCreateSession returns.
fn with_overlay_info<T>(
    system: xr::SystemId,
    graphics: &xr::vulkan::SessionCreateInfo,
    placement: u32,
    submit: impl FnOnce(&xr::sys::SessionCreateInfo) -> T,
) -> T {
    let overlay = xr::sys::SessionCreateInfoOverlayEXTX {
        ty: xr::sys::SessionCreateInfoOverlayEXTX::TYPE,
        next: std::ptr::null(),
        create_flags: Default::default(),
        session_layers_placement: placement,
    };
    let binding = xr::sys::GraphicsBindingVulkanKHR {
        ty: xr::sys::GraphicsBindingVulkanKHR::TYPE,
        next: &overlay as *const _ as *const _,
        instance: graphics.instance,
        physical_device: graphics.physical_device,
        device: graphics.device,
        queue_family_index: graphics.queue_family_index,
        queue_index: graphics.queue_index,
    };
    submit(&xr::sys::SessionCreateInfo {
        ty: xr::sys::SessionCreateInfo::TYPE,
        next: &binding as *const _ as *const _,
        create_flags: Default::default(),
        system_id: system,
    })
}

/// Safety: graphics handles must belong to this instance and satisfy Vulkan enable2.
pub unsafe fn create_session(
    instance: &xr::Instance,
    system: xr::SystemId,
    graphics: &xr::vulkan::SessionCreateInfo,
    overlay: bool,
    placement: u32,
) -> xr::Result<(
    xr::Session<xr::Vulkan>,
    xr::FrameWaiter,
    xr::FrameStream<xr::Vulkan>,
)> {
    if !overlay {
        return unsafe { instance.create_session::<xr::Vulkan>(system, graphics) };
    }
    use xr::sys::Handle;
    let mut handle = xr::sys::Session::NULL;
    let result = with_overlay_info(system, graphics, placement, |info| unsafe {
        (instance.fp().create_session)(instance.as_raw(), info, &mut handle)
    });
    if result.into_raw() < 0 {
        return Err(result);
    }
    Ok(unsafe { xr::Session::<xr::Vulkan>::from_raw(instance.clone(), handle, Box::new(())) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use xr::EnvironmentBlendMode as B;
    #[test]
    fn transparency_is_preferred_regardless_of_runtime_order() {
        let mode = select(true, &[B::OPAQUE, B::ADDITIVE, B::ALPHA_BLEND], false).unwrap();
        assert!(mode.overlay);
        assert_eq!(mode.blend, B::ALPHA_BLEND);
        assert_eq!(
            select(false, &[B::OPAQUE, B::ADDITIVE], false)
                .unwrap()
                .blend,
            B::ADDITIVE
        );
        assert!(!select(false, &[B::ALPHA_BLEND], false).unwrap().overlay);
    }
    #[test]
    fn opaque_only_requires_overlay_or_explicit_standalone() {
        assert!(select(true, &[B::OPAQUE], false).unwrap().overlay);
        assert!(select(false, &[B::OPAQUE], false).is_err());
        assert!(!select(true, &[B::OPAQUE], true).unwrap().overlay);
        assert!(select(true, &[], false).is_err());
    }
    #[test]
    fn overlay_chain_preserves_vulkan_binding_and_positive_placement() {
        let graphics = xr::vulkan::SessionCreateInfo {
            instance: 11usize as _,
            physical_device: 12usize as _,
            device: 13usize as _,
            queue_family_index: 4,
            queue_index: 0,
        };
        with_overlay_info(xr::SystemId::from_raw(1), &graphics, 10, |info| unsafe {
            let binding = &*(info.next as *const xr::sys::GraphicsBindingVulkanKHR);
            assert_eq!(binding.ty, xr::sys::GraphicsBindingVulkanKHR::TYPE);
            assert_eq!(
                (
                    binding.instance as usize,
                    binding.physical_device as usize,
                    binding.device as usize,
                    binding.queue_family_index
                ),
                (11, 12, 13, 4)
            );
            let overlay = &*(binding.next as *const xr::sys::SessionCreateInfoOverlayEXTX);
            assert_eq!(overlay.ty, xr::sys::SessionCreateInfoOverlayEXTX::TYPE);
            assert_eq!(overlay.session_layers_placement, 10);
            assert!(overlay.next.is_null());
        });
    }
}
