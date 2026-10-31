//! Read-only geometry for the experimental, same-user GPU eye recorder.
use crate::{xr_transport::Bridge, Scene};
/// Copy just capture metadata so the service never nests scene/bridge locks.
pub struct XrState {
    visible: bool,
    session: crate::xr_transport::Session,
    shell: crate::xr_transport::ShellLayout,
    gpu_socket: Option<String>,
}
impl From<&Bridge> for XrState {
    fn from(bridge: &Bridge) -> Self {
        Self {
            visible: bridge.visible(),
            session: bridge.session.clone(),
            shell: bridge.shell.clone(),
            gpu_socket: bridge.gpu_socket.clone(),
        }
    }
}
pub fn xr_snapshot(scene: &Scene, bridge: &XrState) -> serde_json::Value {
    let unavailable = if scene.locked {
        Some("Session is locked")
    } else if !bridge.visible {
        Some("No live DomeD XR session")
    } else if scene.native_owner().is_some() {
        Some("Native XR owns the headset; use its OBS mirror stream")
    } else if bridge.session.eyes.is_none() || bridge.session.mouse_viewport.is_none() {
        Some("XR helper does not report eye poses and viewport")
    } else if bridge.gpu_socket.is_none() {
        Some("XR DMA-BUF transport is unavailable")
    } else {
        None
    };
    if let Some(reason) = unavailable {
        return serde_json::json!({"version":1,"available":false,"reason":reason});
    }
    let session = &bridge.session;
    let mut quads =
        crate::xr_input::render_quads_in_fov(scene, session.head, session.mouse_viewport);
    quads.extend(crate::xr_input::shell_quads_in_fov(
        scene,
        session.head,
        &bridge.shell,
        session.mouse_viewport.unwrap(),
    ));
    let opacity: std::collections::HashMap<_, _> = scene
        .panels()
        .into_iter()
        .map(|p| (p.id, p.opacity))
        .collect();
    let surfaces: Vec<_> = quads.into_iter().map(|q| {
        let window = scene.window(q.id);
        serde_json::json!({"stereo":window.is_some_and(|w| w.stereo==crate::stereo::Stereo::SideBySide),
            "opacity":opacity.get(&q.id).copied().unwrap_or(1.), "quad":q})
    }).collect();
    serde_json::json!({"version":1,"available":true,"eyes":session.eyes,
        "viewport":session.mouse_viewport,"gpu_socket":bridge.gpu_socket,"surfaces":surfaces,
        "note":"DomeD surfaces only; runtime passthrough, other sessions and the procedural backdrop are excluded"})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locked_and_expired_sessions_expose_no_surfaces_or_socket() {
        let mut scene = Scene::default();
        let mut b = Bridge::default();
        b.session.visible = true;
        b.last_session = Some(std::time::Instant::now());
        b.gpu_socket = Some("private.sock".into());
        scene.locked = true;
        let value = xr_snapshot(&scene, &XrState::from(&b));
        assert_eq!(value["available"], false);
        assert!(value.get("gpu_socket").is_none());
        scene.locked = false;
        b.last_session = None;
        assert_eq!(xr_snapshot(&scene, &XrState::from(&b))["available"], false);
    }
    #[test]
    fn capture_keeps_stereo_identity_and_runtime_eyes_without_a_gpu_readback_request() {
        let mut scene = Scene::new("capture");
        let id = scene.add("Stereo fixture", crate::stereo::FSVR_SBS_APP_ID);
        scene
            .windows
            .iter_mut()
            .find(|w| w.id == id)
            .unwrap()
            .stereo = crate::stereo::Stereo::SideBySide;
        let mut bridge = Bridge::default();
        bridge.session.visible = true;
        bridge.last_session = Some(std::time::Instant::now());
        bridge.gpu_socket = Some("capture.sock".into());
        assert_eq!(
            xr_snapshot(&scene, &XrState::from(&bridge))["available"],
            false
        );
        let eyes = [-0.032, 0.032].map(|x| crate::Pose {
            position_m: [x, 1.4, 0.],
            ..Default::default()
        });
        bridge.session.eyes = Some(eyes);
        bridge.session.mouse_viewport = Some([-0.7, 0.7, -0.7, 0.7]);
        let value = xr_snapshot(&scene, &XrState::from(&bridge));
        assert_eq!(value["available"], true);
        assert_eq!(value["eyes"], serde_json::to_value(eyes).unwrap());
        assert!(value["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["quad"]["id"] == id && s["stereo"] == true));
        assert!(bridge.requested.is_empty());
    }
}
