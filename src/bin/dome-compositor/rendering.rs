//! Vulkan-backed composition through Mesa Zink; Smithay retains its GLES interface.
use smithay::backend::renderer::gles::GlesRenderer;
use std::ffi::{CStr, OsString};
pub fn selection() -> String {
    std::env::var("DOME_RENDERER").unwrap_or_else(|_| "vulkan".into())
}
struct DriverEnvironment(Vec<(&'static str, Option<OsString>)>);
impl Drop for DriverEnvironment {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}
/// Keep the driver override confined to context creation, so launched apps choose their own renderer.
pub fn create<T, E>(create: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let mut driver = DriverEnvironment(Vec::new());
    if selection() == "vulkan" {
        for key in ["MESA_LOADER_DRIVER_OVERRIDE", "GALLIUM_DRIVER"] {
            driver.0.push((key, std::env::var_os(key)));
            std::env::set_var(key, "zink");
        }
    }
    create()
}
pub fn verify(renderer: &mut GlesRenderer) -> Result<String, Box<dyn std::error::Error>> {
    let name = renderer.with_context(|gl| unsafe {
        let raw = gl.GetString(0x1F01); // GL_RENDERER
        if raw.is_null() {
            String::new()
        } else {
            CStr::from_ptr(raw.cast()).to_string_lossy().into_owned()
        }
    })?;
    if selection() == "vulkan" && !name.to_ascii_lowercase().contains("zink") {
        return Err(format!("Vulkan composition requested, but the active renderer is {name:?}. Install Mesa Zink and a Vulkan driver, or select --renderer gles.").into());
    }
    eprintln!("Dome compositor renderer: {} ({name})", selection());
    Ok(name)
}

/// Optional renderer readback for validating GPU output independently of a host window system.
pub fn capture(
    renderer: &mut GlesRenderer,
    framebuffer: &smithay::backend::renderer::gles::GlesTarget<'_>,
    size: smithay::utils::Size<i32, smithay::utils::Physical>,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use smithay::backend::renderer::ExportMem;
    use std::io::Write;
    let mapping = renderer.copy_framebuffer(
        framebuffer,
        smithay::utils::Rectangle::from_size((size.w, size.h).into()),
        smithay::backend::allocator::Fourcc::Abgr8888,
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

pub fn offscreen() -> Result<GlesRenderer, Box<dyn std::error::Error>> {
    use smithay::backend::egl::{EGLContext, EGLDevice, EGLDisplay};
    create(|| {
        let mut errors = Vec::new();
        for device in EGLDevice::enumerate()? {
            let result = (|| -> Result<GlesRenderer, Box<dyn std::error::Error>> {
                let display = unsafe { EGLDisplay::new(device)? };
                let context = EGLContext::new(&display)?;
                let mut renderer = unsafe { GlesRenderer::new(context)? };
                verify(&mut renderer)?;
                Ok(renderer)
            })();
            match result {
                Ok(r) => return Ok(r),
                Err(e) => errors.push(e.to_string()),
            }
        }
        Err(format!(
            "Cannot create Vulkan composition context: {}",
            errors.join("; ")
        )
        .into())
    })
}
