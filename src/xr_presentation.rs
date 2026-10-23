//! XR-only visibility and reversible presentation transitions. Desktop mapping is unaffected.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Presentation {
    pub top: bool,
    pub bottom: bool,
    pub desktop: bool,
}
impl Default for Presentation {
    fn default() -> Self {
        Self {
            top: true,
            bottom: true,
            desktop: true,
        }
    }
}
impl Presentation {
    pub fn interactive(self, id: u64) -> bool {
        self.desktop
            && if id == crate::xr_transport::TOP_BAR || id == crate::xr_transport::NOTIFICATIONS {
                self.top
            } else if id == crate::xr_transport::TASK_BAR {
                self.bottom
            } else {
                true
            }
    }
}
#[derive(Clone, Copy, Debug)]
struct Transition {
    from: f32,
    to: f32,
    start: f64,
    duration: f64,
}
impl Transition {
    fn new(duration: f64) -> Self {
        Self {
            from: 1.,
            to: 1.,
            start: 0.,
            duration,
        }
    }
    fn value(self, time: f64) -> f32 {
        let t = ((time - self.start) / self.duration).clamp(0., 1.) as f32;
        self.from + (self.to - self.from) * (t * t * (3. - 2. * t))
    }
    fn target(&mut self, show: bool, time: f64) {
        let target = if show { 1. } else { 0. };
        if target != self.to {
            self.from = self.value(time);
            self.to = target;
            self.start = time;
        }
    }
}
pub struct Animator {
    top: Transition,
    bottom: Transition,
    desktop: Transition,
}
impl Default for Animator {
    fn default() -> Self {
        Self {
            top: Transition::new(0.25),
            bottom: Transition::new(0.25),
            desktop: Transition::new(0.25),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub top: f32,
    pub bottom: f32,
    pub opacity: f32,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            top: 1.,
            bottom: 1.,
            opacity: 1.,
        }
    }
}
impl Animator {
    pub fn initial(target: Presentation) -> Self {
        let mut animator = Self::default();
        for (transition, show) in [
            (&mut animator.top, target.top),
            (&mut animator.bottom, target.bottom),
            (&mut animator.desktop, target.desktop),
        ] {
            transition.from = if show { 1. } else { 0. };
            transition.to = transition.from;
        }
        animator
    }
    pub fn update(&mut self, time: f64, target: Presentation) -> Frame {
        self.top.target(target.top, time);
        self.bottom.target(target.bottom, time);
        self.desktop.target(target.desktop, time);
        Frame {
            top: self.top.value(time),
            bottom: self.bottom.value(time),
            opacity: self.desktop.value(time),
        }
    }
}
impl Frame {
    /// During slide/fade-out XR retains the seat until its last visible layer disappears.
    pub fn has_surfaces(self, panels: bool) -> bool {
        self.opacity > 0. && (panels || self.top > 0. || self.bottom > 0.)
    }
    pub fn interactive(self, target: Presentation, id: u64) -> bool {
        target.interactive(id)
            && self.opacity >= 1.
            && if id == crate::xr_transport::TOP_BAR || id == crate::xr_transport::NOTIFICATIONS {
                self.top >= 1.
            } else if id == crate::xr_transport::TASK_BAR {
                self.bottom >= 1.
            } else {
                true
            }
    }

    /// Shell controls fade in place so no visible part crosses the headset FOV.
    pub fn apply(
        self,
        quads: &mut Vec<crate::xr_input::Quad>,
        _head: crate::Pose,
        _config: &crate::Config,
    ) {
        quads.retain_mut(|q| {
            if self.opacity <= 0. {
                return false;
            }
            let amount = if q.id == crate::xr_transport::TOP_BAR
                || q.id == crate::xr_transport::NOTIFICATIONS
            {
                self.top
            } else if q.id == crate::xr_transport::TASK_BAR {
                self.bottom
            } else {
                return true;
            };
            if amount <= 0. {
                return false;
            }
            true
        });
    }
    pub fn surface_opacity(self, id: u64) -> f32 {
        self.opacity * if id == crate::xr_transport::TOP_BAR || id == crate::xr_transport::NOTIFICATIONS {
            self.top
        } else if id == crate::xr_transport::TASK_BAR { self.bottom } else { 1. }
    }
    /// OpenXR expects premultiplied alpha; attenuate RGB and alpha together.
    pub fn fade_rgba(self, rgba: &mut [u8]) {
        let alpha = (self.opacity.clamp(0., 1.) * 255.).round() as u16;
        for c in rgba {
            *c = ((*c as u16 * alpha + 127) / 255) as u8;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_input_follows_any_visible_panel_or_bar() {
        let all = Frame::default();
        assert!(all.has_surfaces(false));
        let panels_only = Frame {
            top: 0.,
            bottom: 0.,
            opacity: 1.,
        };
        assert!(panels_only.has_surfaces(true));
        assert!(!panels_only.has_surfaces(false));
        assert!(Frame {
            top: 1.,
            bottom: 0.,
            opacity: 1.
        }
        .has_surfaces(false));
        assert!(Frame {
            top: 0.,
            bottom: 1.,
            opacity: 1.
        }
        .has_surfaces(false));
        assert!(Frame {
            top: 0.,
            bottom: 0.,
            opacity: 0.5
        }
        .has_surfaces(true));
        assert!(!Frame {
            top: 1.,
            bottom: 1.,
            opacity: 0.
        }
        .has_surfaces(true));
    }
    #[test]
    fn initial_hidden_state_has_no_visible_flash() {
        let target = Presentation {
            desktop: false,
            top: false,
            bottom: true,
        };
        let mut a = Animator::initial(target);
        let frame = a.update(10., target);
        assert_eq!(frame.opacity, 0.);
        assert_eq!(frame.top, 0.);
        assert_eq!(frame.bottom, 1.);
    }
    #[test]
    fn toggles_animate_independently_and_reverse_continuously() {
        let mut a = Animator::default();
        let mut target = Presentation::default();
        target.top = false;
        let f = a.update(1., target);
        assert_eq!(f.top, 1.);
        let f = a.update(1.125, target);
        assert!((f.top - 0.5).abs() < 0.001);
        assert_eq!(f.bottom, 1.);
        assert_eq!(f.opacity, 1.);
        target.top = true;
        assert_eq!(a.update(1.125, target).top, f.top);
        assert_eq!(a.update(1.5, target).top, 1.);
        target.desktop = false;
        a.update(2., target);
        let f = a.update(2.125, target);
        assert!((f.opacity - 0.5).abs() < 0.001);
        assert_eq!(f.top, 1.);
        let mut pixel = [200, 100, 50, 255];
        f.fade_rgba(&mut pixel);
        assert_eq!(pixel, [100, 50, 25, 128]);
        assert_eq!(a.update(2.5, target).opacity, 0.);
    }
    #[test]
    fn hidden_presentation_never_changes_bar_preferences_or_accepts_input() {
        let mut p = Presentation::default();
        p.top = false;
        assert!(!p.interactive(crate::xr_transport::TOP_BAR));
        assert!(p.interactive(1));
        p.desktop = false;
        assert!(!p.interactive(1));
        assert!(!p.interactive(crate::xr_transport::TASK_BAR));
        p.desktop = true;
        assert!(!p.top && p.bottom);
        assert!(p.interactive(1));
    }
    #[test]
    fn bars_fade_in_place_and_disappear_without_changing_geometry() {
        let s = crate::Scene::demo();
        let head = crate::Pose::default();
        let layout = crate::xr_transport::ShellLayout {
            width: 1280,
            height: 800,
            top: 36,
            bottom: 60,
            notifications: None,
        };
        let mut q = crate::xr_input::shell_quads(&s, head, &layout);
        let original = q.clone();
        Frame {
            top: 0.5,
            bottom: 0.5,
            opacity: 1.,
        }
        .apply(&mut q, head, &s.config);
        assert_eq!(q[0].pose, original[0].pose);
        assert_eq!(q[1].pose, original[1].pose);
        assert_eq!(Frame { top: 0.5, bottom: 0.5, opacity: 1. }.surface_opacity(q[0].id), 0.5);
        Frame {
            top: 0.,
            bottom: 0.,
            opacity: 1.,
        }
        .apply(&mut q, head, &s.config);
        assert!(q.is_empty());
    }
}
