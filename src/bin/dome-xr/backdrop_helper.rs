//! One OpenXR instance per process; desktop rendering never waits for this client.
use super::{backdrop, backdrop_session, xr};
use ash::vk;
use std::{
    io::{BufRead, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct Retry {
    next: Option<Instant>,
    failures: u32,
}
impl Retry {
    fn ready(&self, now: Instant) -> bool {
        self.next.is_none_or(|t| now >= t)
    }
    fn failed(&mut self, now: Instant) {
        self.next = Some(now + Duration::from_secs((1u64 << self.failures.min(5)).min(30)));
        self.failures = self.failures.saturating_add(1);
    }
    fn reset(&mut self) {
        *self = Self::default();
    }
}

pub struct Supervisor {
    child: Option<Child>,
    started: Option<Instant>,
    retry: Retry,
    executable: std::path::PathBuf,
    base: String,
    report: Option<String>,
    pub starts: u64,
    last_state: String,
    last_sent: Option<Instant>,
}
impl Supervisor {
    pub fn new(base: String, report: Option<String>) -> Result<Self> {
        Ok(Self {
            child: None,
            started: None,
            retry: Retry::default(),
            executable: std::env::current_exe()?,
            base,
            report,
            starts: 0,
            last_state: String::new(),
            last_sent: None,
        })
    }
    pub fn tick(&mut self, enabled: bool, now: Instant) {
        if !enabled {
            self.stop();
            self.retry.reset();
            return;
        }
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => {
                    if self
                        .started
                        .is_some_and(|t| now.duration_since(t) >= Duration::from_secs(30))
                    {
                        self.retry.reset();
                    }
                    return;
                }
                Ok(Some(status)) => eprintln!("XR backdrop helper exited ({status}); will retry"),
                Err(error) => {
                    eprintln!("XR backdrop helper status: {error}");
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            self.child = None;
            self.retry.failed(now);
        }
        if !self.retry.ready(now) {
            return;
        }
        let mut command = Command::new(&self.executable);
        command
            .args(["--backdrop-only", "--server", &self.base])
            .stdin(Stdio::piped());
        if let Some(report) = &self.report {
            command.args(["--report", report]);
        }
        match command.spawn() {
            Ok(child) => {
                if let Some(stdin) = child.stdin.as_ref() {
                    let _ = nix::fcntl::fcntl(
                        stdin,
                        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
                    );
                }
                self.last_state.clear();
                self.last_sent = None;
                self.child = Some(child);
                self.started = Some(now);
                self.starts += 1;
            }
            Err(error) => {
                eprintln!("XR backdrop helper startup: {error}; will retry");
                self.retry.failed(now);
            }
        }
    }
    pub fn presentation(&mut self, state: dome::xr_backdrop::Presentation) {
        let Ok(mut value) = serde_json::to_string(&state) else {
            return;
        };
        value.push('\n');
        if value == self.last_state
            && self
                .last_sent
                .is_some_and(|t| t.elapsed() < Duration::from_millis(250))
        {
            return;
        }
        if let Some(stdin) = self.child.as_mut().and_then(|c| c.stdin.as_mut()) {
            if stdin.write(value.as_bytes()).ok() == Some(value.len()) {
                self.last_state = value;
                self.last_sent = Some(Instant::now());
            }
        }
    }
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.started = None;
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct Feed {
    scene: Option<dome::Scene>,
    state: dome::xr_backdrop::Presentation,
    received: Option<Instant>,
}

/// Owns a single instance/session at placement zero. No desktop heartbeats,
/// input actions, window downloads or WebXR socket are created in this process.
pub unsafe fn run(
    instance: xr::Instance,
    graphics: &xr::vulkan::SessionCreateInfo,
    vk_instance: &ash::Instance,
    device: &ash::Device,
    physical: vk::PhysicalDevice,
    queue_family: u32,
    queue: vk::Queue,
    base: String,
    running: Arc<AtomicBool>,
    frames: u64,
    report: Option<String>,
) -> Result<()> {
    let shared = Arc::new(Mutex::new(Feed::default()));
    let feed = shared.clone();
    let alive = running.clone();
    // Parent keeps the pipe's write end open. EOF also stops an orphaned child.
    let parent_alive = running.clone();
    let parent_state = Arc::new(Mutex::new((
        dome::xr_backdrop::Presentation::default(),
        None::<Instant>,
    )));
    let parent_feed = parent_state.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if let Ok(state) = serde_json::from_str(&line) {
                *parent_feed.lock().unwrap() = (state, Some(Instant::now()));
            }
        }
        parent_alive.store(false, Ordering::Relaxed);
    });
    let worker = std::thread::spawn(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(250))
            .build();
        while alive.load(Ordering::Relaxed) {
            let sample = (|| -> Result<Feed> {
                let value: serde_json::Value = agent
                    .get(&format!("{base}/api/state"))
                    .call()?
                    .into_json()?;
                let scene: dome::Scene = serde_json::from_value(value["scene"].clone())?;
                let control: dome::xr_transport::Control = agent
                    .get(&format!("{base}/api/xr-control"))
                    .call()?
                    .into_json()?;
                if control.exit_requested {
                    alive.store(false, Ordering::Relaxed);
                }

                Ok(Feed {
                    scene: Some(scene),
                    state: dome::xr_backdrop::Presentation {
                        runtime_visible: control.visible,
                        native_active: control.native_xr_active,
                        webxr_active: false,
                        external_primary: false,
                    },
                    received: Some(Instant::now()),
                })
            })();
            if let Ok(sample) = sample {
                *feed.lock().unwrap() = sample;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let result = (|| -> Result<()> {
        let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)?;
        let reference = xr::ReferenceSpaceType::STAGE;
        // Same STAGE -> LOCAL fallback as the desktop.
        let supported = instance.enumerate_view_configurations(system)?;
        if !supported.contains(&super::VIEW_TYPE) {
            return Err("Backdrop requires stereo views".into());
        }
        let format = vk::Format::R8G8B8A8_SRGB;
        let mut background =
            backdrop_session::Background::new(instance, graphics, format, reference)?;
        let directory =
            dome::xr_backdrop::directory().ok_or("Precipice backdrop is not imported")?;
        let mut renderer = backdrop::Renderer::new(
            vk_instance,
            device,
            physical,
            queue_family,
            queue,
            format,
            &directory,
        )?;
        let start = Instant::now();
        let mut count = 0u64;
        while running.load(Ordering::Relaxed) && count < frames {
            let (scene, mut state, fresh) = {
                let feed = shared.lock().unwrap();
                (
                    feed.scene.clone(),
                    feed.state,
                    feed.received
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(750)),
                )
            };
            let (parent, parent_time) = *parent_state.lock().unwrap();
            state.runtime_visible &= fresh
                && parent.runtime_visible
                && parent_time.is_some_and(|t| t.elapsed() < Duration::from_millis(750));
            state.external_primary |= parent.external_primary;
            state.native_active |= parent.native_active;
            state.webxr_active |= parent.webxr_active;
            background.update(
                &mut renderer,
                queue,
                scene.as_ref(),
                state,
                start.elapsed().as_secs_f64(),
            )?;
            count += 1;
            if count % 120 == 1 {
                if let Some(path) = &report {
                    std::fs::write(
                        path,
                        serde_json::to_vec_pretty(&serde_json::json!({
                            "backdrop_overlay_placement":0,"backdrop_overlay_frames":background.submitted_frames,
                            "backdrop_frames":renderer.frames,"backdrop_suspended_frames":renderer.suspended_frames
                        }))?,
                    )?;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let summary = serde_json::json!({"backdrop_overlay_placement":0,"backdrop_overlay_frames":background.submitted_frames,
            "backdrop_frames":renderer.frames,"backdrop_suspended_frames":renderer.suspended_frames,"clean_exit":true});
        if let Some(path) = report {
            std::fs::write(path, serde_json::to_vec_pretty(&summary)?)?;
        }
        device.device_wait_idle()?;
        Ok(())
    })();
    running.store(false, Ordering::Relaxed);
    let _ = worker.join();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_failures_back_off_and_explicit_disable_allows_immediate_recovery() {
        let now = Instant::now();
        let mut retry = Retry::default();
        assert!(retry.ready(now));
        for _ in 0..50 {
            retry.failed(now);
            assert!(!retry.ready(now));
        }
        assert!(retry.ready(now + Duration::from_secs(30)));
        retry.reset();
        assert!(retry.ready(now));
    }
    #[test]
    fn failed_start_can_recover_to_a_running_child() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "dome-backdrop-retry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&root)?;
        let script = root.join("helper");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nif mkdir '{}'/first 2>/dev/null; then exit 1; fi\nexec sleep 30\n",
                root.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
        let mut supervisor = Supervisor::new("unused".into(), None)?;
        supervisor.executable = script;
        let now = Instant::now();
        supervisor.tick(true, now);
        supervisor.child.as_mut().unwrap().wait()?;
        supervisor.tick(true, now);
        assert_eq!(supervisor.starts, 1);
        supervisor.tick(true, now + Duration::from_secs(1));
        assert_eq!(supervisor.starts, 2);
        assert!(supervisor.child.as_mut().unwrap().try_wait()?.is_none());
        supervisor.stop();
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
    #[test]
    fn child_exit_does_not_latch_failure_or_leave_an_orphan() -> Result<()> {
        let mut supervisor = Supervisor::new("unused".into(), None)?;
        supervisor.executable = "/bin/false".into();
        let now = Instant::now();
        supervisor.tick(true, now);
        let mut child = supervisor.child.take().unwrap();
        child.wait()?;
        supervisor.child = Some(child);
        supervisor.tick(true, now);
        assert_eq!(supervisor.starts, 1);
        supervisor.tick(true, now + Duration::from_secs(1));
        assert_eq!(supervisor.starts, 2);
        supervisor.tick(false, now + Duration::from_secs(1));
        assert!(supervisor.child.is_none());
        supervisor.tick(true, now + Duration::from_secs(1));
        assert_eq!(supervisor.starts, 3);
        Ok(())
    }
}
