//! Local GPU frame transport. Pixel storage stays in DMA-BUF; a socket lease
//! prevents the producer from reusing a buffer while the consumer samples it.
use nix::sys::socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, IoSlice, IoSliceMut, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    path::Path,
};

pub const VERSION: u32 = 1;
pub const MAX_DIMENSION: u32 = 4096;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameInfo {
    pub version: u32,
    pub serial: u64,
    /// DRM render-node device number; direct import is restricted to this GPU.
    pub device: u64,
    pub width: u32,
    pub height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
    pub y_inverted: bool,
    pub viewport: Option<crate::context_menu::Viewport>,
}
impl FrameInfo {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != VERSION
            || self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
            || self.logical_width == 0
            || self.logical_height == 0
            || self.logical_width > MAX_DIMENSION
            || self.logical_height > MAX_DIMENSION
            || self.stride < self.width * 4
            || self.stride > MAX_DIMENSION * 16
            || self.offset > 256 * 1024 * 1024
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid GPU frame",
            ));
        }
        if let Some(v) = self.viewport {
            if ![v.height, v.content_height, v.offset]
                .iter()
                .all(|x| x.is_finite())
                || v.height < 1.
                || v.height > v.content_height
                || v.content_height > self.logical_height as f32
                || v.height > self.logical_height as f32
                || v.offset < 0.
                || v.offset > v.max_offset()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid GPU viewport",
                ));
            }
        }
        Ok(())
    }
    pub fn output_size(&self) -> (u32, u32) {
        let h = self.viewport.map_or(self.height, |v| {
            (v.height * self.height as f32 / self.logical_height as f32).round() as u32
        });
        (self.width, h.max(1))
    }
}
/// The socket must outlive every GPU command using this frame. EOF releases it.
#[derive(Debug)]
pub struct Received {
    pub info: FrameInfo,
    pub fd: OwnedFd,
    _lease: UnixStream,
}
pub fn receive(path: &Path, id: u64) -> io::Result<Received> {
    let mut stream = UnixStream::connect(path)?;
    let credentials =
        nix::sys::socket::getsockopt(&stream, nix::sys::socket::sockopt::PeerCredentials)?;
    if credentials.uid() != nix::unistd::getuid().as_raw() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "GPU server uid mismatch",
        ));
    }
    stream.set_read_timeout(Some(std::time::Duration::from_millis(250)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_millis(250)))?;
    stream.write_all(&id.to_le_bytes())?;
    let (byte, fd) = receive_fd(&stream)?;
    if byte != 1 {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "GPU frame not ready",
        ));
    }
    let fd = fd.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing DMA-BUF"))?;
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Oversize GPU header",
        ));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    let info: FrameInfo = serde_json::from_slice(&bytes)?;
    info.validate()?;
    Ok(Received {
        info,
        fd,
        _lease: stream,
    })
}
fn receive_fd(stream: &UnixStream) -> io::Result<(u8, Option<OwnedFd>)> {
    let mut byte = [0];
    let mut space = nix::cmsg_space!([std::os::fd::RawFd; 4]);
    let mut fds = Vec::new();
    let (size, flags) = {
        let mut iov = [IoSliceMut::new(&mut byte)];
        let msg = recvmsg::<()>(
            stream.as_raw_fd(),
            &mut iov,
            Some(&mut space),
            MsgFlags::MSG_CMSG_CLOEXEC,
        )?;
        for c in msg.cmsgs()? {
            if let ControlMessageOwned::ScmRights(raw) = c {
                for fd in raw {
                    fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
                }
            }
        }
        (msg.bytes, msg.flags)
    };
    if size != 1 || flags.contains(MsgFlags::MSG_CTRUNC) || fds.len() > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid GPU descriptor message",
        ));
    }
    Ok((byte[0], fds.pop()))
}
pub fn send_frame(
    stream: &mut UnixStream,
    info: &FrameInfo,
    fd: std::os::fd::RawFd,
) -> io::Result<()> {
    info.validate()?;
    let fds = [fd];
    sendmsg::<()>(
        stream.as_raw_fd(),
        &[IoSlice::new(&[1])],
        &[ControlMessage::ScmRights(&fds)],
        MsgFlags::MSG_NOSIGNAL,
        None,
    )?;
    let json = serde_json::to_vec(info)?;
    stream.write_all(&(json.len() as u32).to_le_bytes())?;
    stream.write_all(&json)
}

/// Allocate a renderable, explicitly described single-plane image. Prefer tiled
/// layouts: Smithay 0.7 omits LINEAR modifier attributes on EGL import, which
/// Zink cannot infer. Check every candidate on the producer before publishing.
#[cfg(feature = "nested")]
pub fn allocate_capture(
    renderer: &mut smithay::backend::renderer::gles::GlesRenderer,
    allocator: &mut smithay::backend::allocator::gbm::GbmAllocator<std::fs::File>,
    width: u32,
    height: u32,
) -> io::Result<smithay::backend::allocator::dmabuf::Dmabuf> {
    use smithay::backend::{
        allocator::{dmabuf::AsDmabuf, gbm::GbmBufferFlags, Buffer, Fourcc, Modifier},
        renderer::Bind,
    };
    let mut modifiers: Vec<_> = renderer
        .egl_context()
        .display()
        .dmabuf_render_formats()
        .iter()
        .filter(|f| f.code == Fourcc::Argb8888 && f.modifier != Modifier::Invalid)
        .map(|f| f.modifier)
        .collect();
    modifiers.sort_by_key(|m| (u8::from(*m == Modifier::Linear), u64::from(*m)));
    for modifier in modifiers {
        let flags = GbmBufferFlags::RENDERING;
        let Ok(bo) =
            allocator.create_buffer_with_flags(width, height, Fourcc::Argb8888, &[modifier], flags)
        else {
            continue;
        };
        let Ok(mut buffer) = bo.export() else {
            continue;
        };
        if buffer.num_planes() != 1 || buffer.format().modifier == Modifier::Invalid {
            continue;
        }
        if renderer.bind(&mut buffer).is_ok() {
            return Ok(buffer);
        }
    }
    Err(io::Error::other(
        "No shared single-plane ARGB DMA-BUF layout can be rendered",
    ))
}

#[cfg(feature = "nested")]
pub struct Published {
    pub info: FrameInfo,
    pub buffer: smithay::backend::allocator::dmabuf::Dmabuf,
    pub stamp: std::time::Instant,
}

#[cfg(feature = "nested")]
pub struct Server(std::path::PathBuf);
#[cfg(feature = "nested")]
impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
/// Each accepted connection holds one immutable frame until EOF. Limit leases so
/// a stalled/malicious local client cannot create unlimited threads or buffers.
#[cfg(feature = "nested")]
pub fn serve(bridge: crate::xr_transport::Shared, port: u16) -> io::Result<Server> {
    use std::{
        os::unix::{fs::PermissionsExt, net::UnixListener},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    let root = std::path::PathBuf::from(
        std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR missing"))?,
    );
    let path = root.join(format!("dome-xr-gpu-{port}-{}.sock", std::process::id()));
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    bridge.lock().unwrap().gpu_socket = Some(path.to_string_lossy().into_owned());
    std::thread::spawn(move || {
        let active = Arc::new(AtomicUsize::new(0));
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let Ok(peer) =
                nix::sys::socket::getsockopt(&stream, nix::sys::socket::sockopt::PeerCredentials)
            else {
                continue;
            };
            if peer.uid() != nix::unistd::getuid().as_raw() || active.load(Ordering::Relaxed) >= 128
            {
                continue;
            }
            active.fetch_add(1, Ordering::Relaxed);
            let (bridge, active) = (bridge.clone(), active.clone());
            std::thread::spawn(move || {
                let _ = (|| -> io::Result<()> {
                    stream.set_read_timeout(Some(std::time::Duration::from_millis(250)))?;
                    stream.set_write_timeout(Some(std::time::Duration::from_millis(250)))?;
                    let mut id = [0; 8];
                    stream.read_exact(&mut id)?;
                    let id = u64::from_le_bytes(id);
                    let frame = {
                        let mut b = bridge.lock().unwrap();
                        b.gpu_requested.insert(id, std::time::Instant::now());
                        b.gpu_frames
                            .get(&id)
                            .cloned()
                    };
                    if let Some(frame) = frame {
                        let fd = frame
                            .buffer
                            .handles()
                            .next()
                            .ok_or_else(|| io::Error::other("Empty DMA-BUF"))?;
                        send_frame(&mut stream, &frame.info, fd.as_raw_fd())?;
                        // No timeout: expiring a lease while the GPU samples it
                        // could permit a producer overwrite. EOF is the release.
                        stream.set_read_timeout(None)?;
                        let _ = stream.read(&mut [0]);
                        drop(frame);
                    } else {
                        stream.write_all(&[0])?;
                    }
                    Ok(())
                })();
                active.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Ok(Server(path))
}

/// Bound texture size while preserving density equally on both axes.
pub fn capture_size(width: u32, height: u32, scale: f32) -> (u32, u32, f64) {
    let scale = scale
        .clamp(1., 2.)
        .min(MAX_DIMENSION as f32 / width.max(height).max(1) as f32);
    (
        (width as f32 * scale)
            .ceil()
            .clamp(1., MAX_DIMENSION as f32) as u32,
        (height as f32 * scale)
            .ceil()
            .clamp(1., MAX_DIMENSION as f32) as u32,
        scale as f64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info() -> FrameInfo {
        FrameInfo {
            version: VERSION,
            serial: 4,
            device: 0,
            width: 1080,
            height: 690,
            logical_width: 720,
            logical_height: 460,
            fourcc: 875713089,
            modifier: 0,
            stride: 4320,
            offset: 0,
            y_inverted: false,
            viewport: None,
        }
    }
    #[test]
    fn density_preserves_aspect_and_caps_large_windows() {
        assert_eq!(capture_size(720, 460, 1.5), (1080, 690, 1.5));
        let (w, h, s) = capture_size(3840, 2160, 2.);
        assert_eq!((w, h), (4096, 2304));
        assert!(s < 2.);
        let mut f = info();
        f.width = 5000;
        assert!(f.validate().is_err());
        f = info();
        f.stride = 1;
        assert!(f.validate().is_err());
        f = info();
        f.viewport = Some(crate::context_menu::Viewport::new(460., 200., 30.));
        assert_eq!(f.output_size(), (1080, 300));
    }
    #[test]
    fn descriptor_and_metadata_round_trip_and_lease() {
        use std::os::unix::net::UnixListener;
        let root = std::env::temp_dir().join(format!("dome-gpu-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&root);
        let listener = UnixListener::bind(&root).unwrap();
        let thread = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut id = [0; 8];
            s.read_exact(&mut id).unwrap();
            assert_eq!(u64::from_le_bytes(id), 123);
            let file = std::fs::File::open("/dev/null").unwrap();
            send_frame(&mut s, &info(), file.as_raw_fd()).unwrap();
            assert_eq!(s.read(&mut [0]).unwrap(), 0);
        });
        let f = receive(&root, 123).unwrap();
        assert_eq!(f.info.serial, 4);
        assert!(nix::fcntl::fcntl(&f.fd, nix::fcntl::FcntlArg::F_GETFD).is_ok());
        drop(f);
        thread.join().unwrap();
        std::fs::remove_file(root).unwrap();
    }
}
