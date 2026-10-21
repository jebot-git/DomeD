//! DMA-BUF capture pool, owned and rendered exclusively on the compositor thread.
use dome::xr_gpu::{FrameInfo, Published};
use smithay::{
    backend::{
        allocator::{
            dmabuf::Dmabuf,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Buffer,
        },
        egl::EGLDevice,
        renderer::{
            damage::OutputDamageTracker,
            element::{
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                RenderElement, Id, Kind,
            },
            gles::GlesRenderer,
            sync::SyncPoint,
            utils::CommitCounter,
            Bind,
        },
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Physical, Rectangle, Transform},
};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    sync::{Arc, Weak},
    time::Instant,
};

type Signature = Vec<(Id, CommitCounter, Rectangle<i32, Physical>)>;

struct Slot {
    buffer: Dmabuf,
    lease: Weak<Published>,
    bytes: u64,
}
struct Pending {
    id: u64,
    frame: Arc<Published>,
    sync: SyncPoint,
}
#[derive(Default)]
pub struct Capture {
    allocator: Option<GbmAllocator<std::fs::File>>,
    disabled: bool,
    device: u64,
    slots: HashMap<u64, Vec<Slot>>,
    pending: Vec<Pending>,
    serial: u64,
    shell_sources: HashMap<u64, Arc<Vec<u8>>>,
    signatures: HashMap<u64, (String, Signature)>,
}
impl Capture {
    pub fn publish(&mut self, bridge: &dome::xr_transport::Shared, live: &[u64]) {
        let mut b = bridge.lock().unwrap();
        b.gpu_requested
            .retain(|id, t| live.contains(id) && t.elapsed().as_secs_f32() < 0.5);
        b.gpu_frames.retain(|id, _| live.contains(id));
        self.pending.retain(|p| {
            if !live.contains(&p.id) {
                return false;
            }
            if p.sync.is_reached() {
                b.gpu_frames.insert(p.id, p.frame.clone());
                false
            } else {
                true
            }
        });
        self.slots.retain(|id, _| live.contains(id));
        self.shell_sources.retain(|id, _| live.contains(id));
        self.signatures.retain(|id, _| live.contains(id));
    }
    pub fn render(
        &mut self,
        renderer: &mut GlesRenderer,
        id: u64,
        surface: &WlSurface,
        geo: Rectangle<i32, Logical>,
        scale: f32,
        viewport: Option<dome::context_menu::Viewport>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (width, height, scale) =
            dome::xr_gpu::capture_size(geo.size.w as u32, geo.size.h as u32, scale);
        let elements = render_elements_from_surface_tree::<
            GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>,
        >(renderer, surface,
            ((-geo.loc.x as f64 * scale).round() as i32,
             (-geo.loc.y as f64 * scale).round() as i32),
            scale, 1., Kind::Unspecified);
        self.render_elements(renderer, id, (width, height),
            (geo.size.w as u32, geo.size.h as u32), scale, viewport, &elements)?;
        Ok(())
    }
    /// Rasterize the changed shell once, then share its GPU buffer by lease.
    /// Unchanged shell pixels never cross the compositor/XR boundary again.
    pub fn render_shell(&mut self, renderer: &mut GlesRenderer, id: u64,
        source: &dome::xr_transport::Surface) -> Result<(), Box<dyn std::error::Error>> {
        if self.shell_sources.get(&id).is_some_and(|old| Arc::ptr_eq(old, &source.rgba)) {
            return Ok(());
        }
        let buffer = MemoryRenderBuffer::from_slice(&source.rgba,
            smithay::backend::allocator::Fourcc::Abgr8888,
            (source.width as i32, source.height as i32), 1, Transform::Normal, None);
        let element = MemoryRenderBufferRenderElement::from_buffer(renderer, (0., 0.),
            &buffer, None, None, None, Kind::Unspecified)?;
        if self.render_elements(renderer, id, (source.width, source.height),
            (source.width, source.height), 1., None, &[element])? {
            self.shell_sources.insert(id, source.rgba.clone());
        }
        Ok(())
    }
    fn render_elements<E: RenderElement<GlesRenderer>>(&mut self,
        renderer: &mut GlesRenderer, id: u64, (width, height): (u32, u32),
        logical: (u32, u32), scale: f64, viewport: Option<dome::context_menu::Viewport>,
        elements: &[E]) -> Result<bool, Box<dyn std::error::Error>> {
        if self.disabled || self.pending.iter().any(|p| p.id == id) { return Ok(false); }
        let key = format!("{width}:{height}:{logical:?}:{scale}:{viewport:?}");
        let signature: Signature = elements.iter().map(|element| (
            element.id().clone(), element.current_commit(), element.geometry(scale.into()))).collect();
        if self.signatures.get(&id).is_some_and(|old| old.0 == key && old.1 == signature) {
            return Ok(false);
        }

        if self.allocator.is_none() {
            let device = EGLDevice::device_for_display(renderer.egl_context().display())?;
            let path = device.render_device_path()?;
            let file = OpenOptions::new().read(true).write(true).open(&path)?;
            use std::os::unix::fs::MetadataExt;
            self.device = file.metadata()?.rdev();
            self.allocator = Some(GbmAllocator::new(GbmDevice::new(file)?, GbmBufferFlags::RENDERING));
            eprintln!("XR GPU capture allocator: {}", path.display());
        }
        let used: u64 = self.slots.values().flatten().map(|s| s.bytes).sum();
        let slots = self.slots.entry(id).or_default();
        slots.retain(|s| {
            s.lease.strong_count() != 0
                || (s.buffer.width() == width && s.buffer.height() == height)
        });
        let index = if let Some(i) = slots.iter().position(|s| {
            s.lease.strong_count() == 0 && s.buffer.width() == width && s.buffer.height() == height
        }) {
            i
        } else {
            let bytes = width as u64 * height as u64 * 4;
            if slots.len() >= 3 || used + bytes > 256 * 1024 * 1024 {
                return Ok(false);
            }
            let buffer = dome::xr_gpu::allocate_capture(
                renderer,
                self.allocator.as_mut().unwrap(),
                width,
                height,
            )?;
            slots.push(Slot {
                buffer,
                lease: Weak::new(),
                bytes,
            });
            slots.len() - 1
        };
        let slot = &mut slots[index];
        let mut target = renderer.bind(&mut slot.buffer)?;
        let mut tracker =
            OutputDamageTracker::new((width as i32, height as i32), scale, Transform::Normal);
        let sync = tracker
            .render_output(renderer, &mut target, 0, elements, [0., 0., 0., 0.])?
            .sync;
        drop(target);
        if self.serial == 0 {
            self.serial = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos() as u64;
        }
        self.serial += 1;
        let frame = Arc::new(Published {
            info: FrameInfo {
                version: dome::xr_gpu::VERSION,
                serial: self.serial,
                device: self.device,
                width,
                height,
                logical_width: logical.0,
                logical_height: logical.1,
                fourcc: slot.buffer.format().code as u32,
                modifier: slot.buffer.format().modifier.into(),
                stride: slot.buffer.strides().next().unwrap(),
                offset: slot.buffer.offsets().next().unwrap(),
                y_inverted: slot.buffer.y_inverted(),
                viewport,
            },
            buffer: slot.buffer.clone(),
            stamp: Instant::now(),
        });
        slot.lease = Arc::downgrade(&frame);
        self.pending.push(Pending { id, frame, sync });
        self.signatures.insert(id, (key, signature));
        Ok(true)
    }
    pub fn disable(&mut self, error: &dyn std::fmt::Display) {
        eprintln!("XR GPU capture unavailable; using CPU transport: {error}");
        self.disabled = true;
    }
}
