//! Lowest-placement overlay for the opaque city, independent of desktop layers.
//! A separate instance also works on runtimes with one session per instance.
use super::{backdrop, pose, Panel, VIEW_TYPE};
use ash::vk;
use dome::{xr_backdrop::Presentation, Scene};
use openxr as xr;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Background {
    // Session children are dropped before the session and shared Vulkan device.
    panels: Vec<Panel>,
    stage: xr::Space,
    view: xr::Space,
    stream: xr::FrameStream<xr::Vulkan>,
    waiter: xr::FrameWaiter,
    session: xr::Session<xr::Vulkan>,
    instance: xr::Instance,
    events: xr::EventDataBuffer,
    active: bool,
    visible: bool,
    lost: bool,
    external_primary: bool,
    format: vk::Format,
    blend: xr::EnvironmentBlendMode,
    sizes: Vec<[u32; 2]>,
    pub submitted_frames: u64,
}

impl Background {
    /// The Vulkan binding is shared with the desktop. All queue access and both
    /// sessions' begin/end/acquire/release calls stay on the render thread.
    pub unsafe fn new(
        entry: &xr::Entry,
        graphics: &xr::vulkan::SessionCreateInfo,
        format: vk::Format,
        reference_space_type: xr::ReferenceSpaceType,
    ) -> Result<Self> {
        let mut extensions = xr::ExtensionSet::default();
        extensions.extx_overlay = true;
        extensions.khr_vulkan_enable2 = true;
        let instance = entry.create_instance(
            &xr::ApplicationInfo {
                application_name: "Dome backdrop",
                application_version: 1,
                engine_name: "Dome Rust",
                engine_version: 1,
                api_version: xr::Version::new(1, 0, 0),
            },
            &extensions,
            &[],
        )?;
        let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)?;
        let requirements = instance.graphics_requirements::<xr::Vulkan>(system)?;
        if requirements.min_api_version_supported > xr::Version::new(1, 1, 0) {
            return Err("Backdrop session requires a newer Vulkan version".into());
        }
        if instance.vulkan_graphics_device(system, graphics.instance)? != graphics.physical_device {
            return Err("Backdrop and desktop sessions selected different GPUs".into());
        }
        let modes = instance.enumerate_environment_blend_modes(system, VIEW_TYPE)?;
        let blend = dome::xr_environment::select(true, &modes, false)?.blend;
        let (session, waiter, stream) = dome::xr_environment::create_session(
            &instance,
            system,
            graphics,
            true,
            dome::xr_environment::BACKDROP_PLACEMENT,
        )?;
        if !session
            .enumerate_swapchain_formats()?
            .contains(&(format.as_raw() as u32))
        {
            return Err("Backdrop session does not support the desktop RGBA format".into());
        }
        // Use exactly the desktop's space type; mixing LOCAL and STAGE would
        // move the city relative to the desktop when a stage is unavailable.
        let stage = session.create_reference_space(reference_space_type, xr::Posef::IDENTITY)?;
        let view =
            session.create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)?;
        let recommended = instance.enumerate_view_configuration_views(system, VIEW_TYPE)?;
        if recommended.len() != 2 {
            return Err("Backdrop session requires stereo views".into());
        }
        let sizes = recommended
            .iter()
            .map(|v| {
                let width = v.recommended_image_rect_width;
                let height = v.recommended_image_rect_height;
                let scale = (1536. / width.max(height) as f32).min(1.);
                [
                    (width as f32 * scale).max(1.) as u32,
                    (height as f32 * scale).max(1.) as u32,
                ]
            })
            .collect();
        eprintln!(
            "XR backdrop uses separate overlay session at placement {}",
            dome::xr_environment::BACKDROP_PLACEMENT
        );
        Ok(Self {
            panels: Vec::new(),
            stage,
            view,
            stream,
            waiter,
            session,
            instance,
            events: xr::EventDataBuffer::new(),
            active: false,
            visible: false,
            lost: false,
            external_primary: false,
            format,
            blend,
            sizes,
            submitted_frames: 0,
        })
    }

    fn poll(&mut self) -> Result<()> {
        while let Some(event) = self.instance.poll_event(&mut self.events)? {
            match event {
                xr::Event::SessionStateChanged(event) => {
                    let state = event.state();
                    self.visible =
                        matches!(state, xr::SessionState::VISIBLE | xr::SessionState::FOCUSED);
                    match state {
                        xr::SessionState::READY => {
                            self.session.begin(VIEW_TYPE)?;
                            self.active = true;
                        }
                        xr::SessionState::STOPPING => {
                            self.session.end()?;
                            self.active = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            self.lost = true;
                        }
                        _ => (),
                    }
                }
                xr::Event::MainSessionVisibilityChangedEXTX(event) => {
                    self.external_primary = event.visible();
                }
                xr::Event::InstanceLossPending(_) => self.lost = true,
                _ => (),
            }
        }
        Ok(())
    }

    pub unsafe fn update(
        &mut self,
        renderer: &mut backdrop::Renderer,
        queue: vk::Queue,
        scene: Option<&Scene>,
        mut state: Presentation,
        now: f64,
    ) -> Result<()> {
        self.poll()?;
        if !self.active || self.lost {
            renderer.suspend(now);
            return Ok(());
        }
        // Each session owns its frame timing and tracking; do not reuse the
        // desktop's predicted display time for a different OpenXR instance.
        let frame = self.waiter.wait()?;
        self.stream.begin()?;
        let mut ended = false;
        let result = (|| -> Result<bool> {
            state.runtime_visible &= frame.should_render && self.visible;
            state.external_primary |= self.external_primary;
            let Some(scene) = scene.filter(|s| dome::xr_backdrop::visible(s, state)) else {
                renderer.suspend(now);
                return Ok(false);
            };
            let head = self
                .view
                .locate(&self.stage, frame.predicted_display_time)?;
            let (flags, views) =
                self.session
                    .locate_views(VIEW_TYPE, frame.predicted_display_time, &self.stage)?;
            if !tracking_valid(head.location_flags, flags, views.len()) {
                renderer.suspend(now);
                return Ok(false);
            }
            if self.panels.is_empty() {
                for [width, height] in &self.sizes {
                    self.panels
                        .push(Panel::new(&self.session, self.format, *width, *height)?);
                }
            }
            renderer.activity(
                now,
                true,
                pose(head.pose),
                &scene.config,
                scene.config.xr_backdrop_sound,
            );
            let mut targets = Vec::new();
            for (panel, view) in self.panels.iter_mut().zip(&views) {
                let index = panel.chain.acquire_image()?;
                panel.chain.wait_image(xr::Duration::INFINITE)?;
                targets.push((
                    panel.images[index as usize],
                    panel.width,
                    panel.height,
                    pose(view.pose),
                    [
                        view.fov.angle_left,
                        view.fov.angle_right,
                        view.fov.angle_down,
                        view.fov.angle_up,
                    ],
                ));
            }
            let rendered = renderer.render(queue, &targets, &scene.config);
            for panel in &mut self.panels {
                panel.chain.release_image()?;
            }
            rendered?;
            let views: Vec<_> = self
                .panels
                .iter()
                .zip(&views)
                .map(|(panel, view)| {
                    xr::CompositionLayerProjectionView::new()
                        .pose(view.pose)
                        .fov(view.fov)
                        .sub_image(
                            xr::SwapchainSubImage::new()
                                .swapchain(&panel.chain)
                                .image_array_index(0)
                                .image_rect(xr::Rect2Di {
                                    offset: xr::Offset2Di::default(),
                                    extent: xr::Extent2Di {
                                        width: panel.width as i32,
                                        height: panel.height as i32,
                                    },
                                }),
                        )
                })
                .collect();
            let projection = xr::CompositionLayerProjection::new()
                .space(&self.stage)
                .views(&views);
            ended = true;
            self.stream
                .end(frame.predicted_display_time, self.blend, &[&projection])?;
            self.submitted_frames += 1;
            Ok(true)
        })();
        // Empty submissions remove the opaque layer on suspension, including a
        // disable/lock/native/WebXR transition. Balance begin even on failure.
        if !ended {
            renderer.suspend(now);
            self.stream
                .end(frame.predicted_display_time, self.blend, &[])?;
        }
        result.map(|_| ())
    }
}

fn tracking_valid(head: xr::SpaceLocationFlags, views: xr::ViewStateFlags, count: usize) -> bool {
    count == 2
        && head.contains(
            xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID,
        )
        && views
            .contains(xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_requires_valid_tracking_for_both_eyes() {
        let head =
            xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
        let views = xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID;
        assert!(tracking_valid(head, views, 2));
        assert!(!tracking_valid(head, views, 1));
        assert!(!tracking_valid(
            xr::SpaceLocationFlags::ORIENTATION_VALID,
            views,
            2
        ));
        assert!(!tracking_valid(
            head,
            xr::ViewStateFlags::ORIENTATION_VALID,
            2
        ));
    }
}

#[cfg(test)]
#[path = "backdrop_session_test.rs"]
mod session_tests;
