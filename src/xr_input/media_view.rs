//! Fullscreen stereo media uses the same headset viewport for rendering and input.
use super::Quad;
use crate::{Pose, Scene, Window};
use glam::{Quat, Vec3};

/// Preserve one quarter of head movement relative to the screen on all six axes.
const HEAD_FOLLOW: f32 = 0.75;

impl Window {
    pub fn xr_media(&self) -> bool {
        self.stereo == crate::stereo::Stereo::SideBySide
            && matches!(self.app_id.as_str(), crate::stereo::MPVR_SBS_APP_ID | crate::stereo::QIMGVR_SBS_APP_ID)
    }
}
impl Scene {
    /// Focus changes suspend fullscreen without discarding the saved dome layout.
    pub fn xr_fullscreen(&self) -> Option<&Window> {
        let window = self.window(self.popup_owner(self.focused?))?;
        (window.xr_media() && window.maximized && window.xr_visible && !window.minimized
            && window.workspace == self.active_workspace && self.popup(window.id).is_none())
            .then_some(window)
    }
}

pub(super) fn fullscreen_quad(scene: &Scene, head: Pose, viewport: Option<[f32; 4]>) -> Option<Quad> {
    let window = scene.xr_fullscreen()?;
    let anchor = window.xr_fullscreen_anchor.filter(|p| p.validate().is_ok()).unwrap_or(scene.frame_pose());
    let frame = Pose {
        position_m: Vec3::from_array(anchor.position_m).lerp(Vec3::from_array(head.position_m), HEAD_FOLLOW).to_array(),
        orientation_xyzw: Quat::from_array(anchor.orientation_xyzw).slerp(Quat::from_array(head.orientation_xyzw), HEAD_FOLLOW).to_array(),
    };
    let vertical = scene.config.vertical_fov_deg.clamp(30., 160.).to_radians() / 2.;
    let fallback = [-50_f32.to_radians(), 50_f32.to_radians(), -vertical, vertical];
    let valid = |v: &[f32; 4]| v.iter().all(|x| x.is_finite() && x.abs() < 1.57)
        && v[0] < 0. && v[1] > 0. && v[2] < 0. && v[3] > 0.;
    let [left, right, down, up] = viewport.filter(valid).unwrap_or(fallback).map(f32::tan);
    // The transport is squeezed SBS: each half expands to the canvas aspect.
    // Fit that aspect into the common binocular view without cropping either eye.
    let distance = scene.config.radius_m.max(0.1);
    let aspect = window.desktop.w / window.desktop.h.max(1.);
    let height = (distance * (up - down)).min(distance * (right - left) / aspect);
    Some(Quad {
        id: window.id,
        pose: Pose { position_m: frame.transform([
            distance * (left + right) / 2., distance * (down + up) / 2., -distance,
        ]), ..frame },
        size: [height * aspect, height],
        dome: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, xr_input, xr_transport::ShellLayout};
    use glam::Vec3;
    fn media(app: &str) -> (Scene, u64) {
        let mut scene = Scene::new("fullscreen test");
        let id = scene.add("Stereo media", app);
        scene.apply(Action::Maximize { id }).unwrap();
        (scene, id)
    }
    #[test]
    fn both_players_fill_the_binocular_view_and_share_mouse_geometry() {
        for app in [crate::stereo::MPVR_SBS_APP_ID, crate::stereo::QIMGVR_SBS_APP_ID] {
            let (mut scene, id) = media(app);
            scene.windows[0].desktop.w = 1280.; scene.windows[0].desktop.h = 720.;
            let mut head = Pose::default();
            head.position_m = [0.2, 1.6, -0.3];
            head.orientation_xyzw = glam::Quat::from_euler(glam::EulerRot::YXZ, 0.4, -0.2, 0.1).to_array();
            scene.windows[0].xr_fullscreen_anchor = Some(head);
            let viewport = [-0.7, 0.85, -0.6, 0.7];
            let rendered = xr_input::render_quads_in_fov(&scene, head, Some(viewport));
            let input = xr_input::quads_in_fov(&scene, head, Some(viewport));
            assert_eq!(rendered.len(), 1); assert_eq!(rendered[0].id, id);
            assert!(rendered[0].dome.is_none());
            assert_eq!(serde_json::to_value(&rendered).unwrap(), serde_json::to_value(&input).unwrap());
            let q = &rendered[0];
            assert!((q.size[0] / q.size[1] - 1280. / 720.).abs() < 1e-5);
            let distance = scene.config.radius_m;
            let fills_width = (q.size[0] - distance * (viewport[1].tan() - viewport[0].tan())).abs() < 1e-4;
            let fills_height = (q.size[1] - distance * (viewport[3].tan() - viewport[2].tan())).abs() < 1e-4;
            assert!(fills_width || fills_height);
            let origin = Vec3::from_array(head.position_m);
            for uv in [[0.,0.], [1.,1.], [0.,1.], [1.,0.], [0.5,0.5], [0.25,0.7]] {
                let point = q.point(uv);
                let p = head.inverse_rotate((point - origin).to_array());
                assert!(p[0] / -p[2] >= viewport[0].tan() - 1e-5);
                assert!(p[0] / -p[2] <= viewport[1].tan() + 1e-5);
                assert!(p[1] / -p[2] >= viewport[2].tan() - 1e-5);
                assert!(p[1] / -p[2] <= viewport[3].tan() + 1e-5);
                let hit = input[0].hit(origin, (point-origin).normalize(), false).unwrap();
                assert!((hit[0]-uv[0]).abs() < 1e-5 && (hit[1]-uv[1]).abs() < 1e-5);
            }
            assert!(xr_input::shell_quads(&scene, head, &ShellLayout{width:1280,height:720,top:36,bottom:60,notifications:None}).is_empty());
            assert!(!xr_input::shell_quads(&scene, head, &ShellLayout{width:1280,height:720,top:456,bottom:60,notifications:None}).is_empty());
            // Head movement and anchoring choices cannot strand the fullscreen view.
            let moved = Pose {position_m:[1.,2.,3.], ..head};
            let q = fullscreen_quad(&scene,moved,Some(viewport)).unwrap();
            assert_eq!(q.pose.orientation_xyzw,moved.orientation_xyzw);
            let shift = Vec3::from_array(q.pose.position_m) - Vec3::from_array(rendered[0].pose.position_m);
            let expected = (Vec3::from_array(moved.position_m) - Vec3::from_array(head.position_m)) * HEAD_FOLLOW;
            assert!((shift-expected).length() < 1e-4);
        }
    }
    #[test]
    fn restoring_and_focus_changes_preserve_the_dome_and_other_apps() {
        for app in [crate::stereo::MPVR_SBS_APP_ID, crate::stereo::QIMGVR_SBS_APP_ID] {
            let (mut scene,id)=media(app);
            scene.apply(Action::Maximize{id}).unwrap();
            let before=serde_json::to_value(xr_input::quads(&scene,Pose::default())).unwrap();
            scene.apply(Action::Maximize{id}).unwrap();
            let saved=scene.window(id).unwrap().desktop;
            let other=scene.add("Other","terminal");
            assert!(scene.xr_fullscreen().is_none());
            scene.apply(Action::Focus{id}).unwrap();assert_eq!(scene.xr_fullscreen().unwrap().id,id);
            scene.apply(Action::Minimize{id}).unwrap();assert!(scene.xr_fullscreen().is_none());
            scene.apply(Action::Focus{id}).unwrap();assert_eq!(scene.xr_fullscreen().unwrap().id,id);
            scene.apply(Action::Maximize{id}).unwrap();
            scene.apply(Action::Close{id:other}).unwrap();
            assert_eq!(scene.window(id).unwrap().desktop,saved);
            assert_eq!(serde_json::to_value(xr_input::quads(&scene,Pose::default())).unwrap(),before);
            scene.apply(Action::Maximize{id}).unwrap();
            scene.windows[0].xr_visible=false;assert!(scene.xr_fullscreen().is_none());
            scene.windows[0].xr_visible=true;scene.active_workspace=1;assert!(scene.xr_fullscreen().is_none());
        }
        for app in ["mpv","qimgvr",crate::stereo::GMSH_SBS_APP_ID] {
            let (scene,_)=media(app);assert!(scene.xr_fullscreen().is_none());
        }
    }
}

#[cfg(test)]
mod movement_tests {
    use super::*;
    #[test]
    fn all_six_head_axes_keep_partial_relative_motion() {
        let mut scene = Scene::new("6dof");
        let id=scene.add("Media",crate::stereo::MPVR_SBS_APP_ID);
        scene.apply(crate::Action::Maximize{id}).unwrap();
        let initial=fullscreen_quad(&scene,Pose::default(),None).unwrap();
        for axis in [Vec3::X,Vec3::Y,Vec3::Z] {
            let head=Pose{position_m:(Vec3::from_array(Pose::default().position_m)+axis*0.1).to_array(),..Pose::default()};
            let moved=fullscreen_quad(&scene,head,None).unwrap();
            let delta=Vec3::from_array(moved.pose.position_m)-Vec3::from_array(initial.pose.position_m);
            assert!((delta-axis*0.1*HEAD_FOLLOW).length()<1e-5);
            let relative=delta-(Vec3::from_array(head.position_m)-Vec3::from_array(Pose::default().position_m));
            assert!((relative+axis*0.025).length()<1e-5);
            let rotation=Quat::from_axis_angle(axis,0.2);
            let head=Pose{orientation_xyzw:rotation.to_array(),..Pose::default()};
            let moved=fullscreen_quad(&scene,head,None).unwrap();
            let orientation=Quat::from_array(moved.pose.orientation_xyzw);
            assert!((orientation.angle_between(Quat::IDENTITY)-0.15).abs()<1e-4);
            assert!((orientation.angle_between(rotation)-0.05).abs()<1e-4);
        }
    }
}
