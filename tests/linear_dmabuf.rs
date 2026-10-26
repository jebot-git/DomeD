//! Opt-in hardware regression: explicit linear buffers must survive EGL import.
#![cfg(feature = "nested")]
use smithay::backend::{
    allocator::{dmabuf::AsDmabuf, gbm::{GbmAllocator, GbmBufferFlags, GbmDevice}, Fourcc, Modifier},
    egl::{EGLDevice, EGLDisplay},
};

#[test]
#[ignore = "requires a DRM render node; set DOME_TEST_RENDER_NODE and Mesa Zink"]
fn explicit_linear_dmabuf_imports_on_its_producer() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(std::env::var("DOME_TEST_RENDER_NODE")?);
    let device = EGLDevice::enumerate()?
        .find(|d| d.render_device_path().ok().as_ref() == Some(&path))
        .ok_or("No matching EGL render device")?;
    let display = unsafe { EGLDisplay::new(device)? };
    let file = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let mut allocator = GbmAllocator::new(GbmDevice::new(file)?, GbmBufferFlags::RENDERING);
    let bo = allocator.create_buffer_with_flags(1280, 720, Fourcc::Argb8888,
        &[Modifier::Linear], GbmBufferFlags::RENDERING)?;
    let dmabuf = bo.export()?;
    // Smithay 0.7.0 omitted the zero modifier here and Zink returned EGL_BAD_ALLOC.
    let image = display.create_image_from_dmabuf(&dmabuf)?;
    assert!(!image.is_null());
    Ok(())
}
