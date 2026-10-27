//! Native input ownership for the browser's immersive controller stream.
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const POLICY_VERSION: u32 = 1;
pub const MAX_POLICY_AGE: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub version: u32,
    pub blocked: bool,
    #[serde(default)]
    pub visible: bool,
    pub focused_pid: Option<u32>,
}

impl Policy {
    pub fn for_scene(scene: &crate::Scene, realm: crate::Realm, modal: bool) -> Self {
        let focused_pid = scene.focused.and_then(|id| scene.window(id))
            .filter(|window| !window.minimized && window.workspace == scene.active_workspace
                && window.visible_in(realm) && window.client_pid != 0)
            .map(|window| window.client_pid);
        Self {
            version: POLICY_VERSION,
            focused_pid,
            visible: !scene.locked,
            blocked: modal || scene.locked || scene.popups.iter().any(|popup|
                scene.popup_visible(popup, realm == crate::Realm::Desktop)
                    && (Some(popup.id) == scene.focused || popup.context_at.is_some())),
        }
    }
    pub fn allows(&self, owner: Option<u32>, age: Option<Duration>) -> bool {
        self.version == POLICY_VERSION
            && self.visible
            && !self.blocked
            && age.is_some_and(|age| age <= MAX_POLICY_AGE)
            && owner.is_some_and(|pid| pid != 0 && self.focused_pid == Some(pid))
    }
}

pub fn control_age(control: &crate::xr_transport::Control, received: Option<Duration>) -> Option<Duration> {
    received?.checked_add(Duration::from_millis(control.webxr_input_age_ms?))
}

pub fn control_visible(control: &crate::xr_transport::Control, received: Option<Duration>) -> bool {
    control.webxr_input.as_ref().is_some_and(|policy| policy.version == POLICY_VERSION && policy.visible)
        && control_age(control, received).is_some_and(|age| age <= MAX_POLICY_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn window(scene: &mut crate::Scene, id: u64) -> &mut crate::Window {
        scene.windows.iter_mut().find(|window| window.id == id).unwrap()
    }
    #[test]
    fn page_input_requires_fresh_native_focus_and_no_modal_owner() {
        let mut policy = Policy { version: 1, blocked: false, visible: true, focused_pid: Some(42) };
        assert!(policy.allows(Some(42), Some(Duration::ZERO)));
        assert!(!policy.allows(Some(43), Some(Duration::ZERO)));
        assert!(!policy.allows(None, Some(Duration::ZERO)));
        assert!(!policy.allows(Some(42), None));
        assert!(!policy.allows(Some(42), Some(MAX_POLICY_AGE + Duration::from_nanos(1))));
        policy.blocked = true;
        assert!(!policy.allows(Some(42), Some(Duration::ZERO)));
        policy.blocked = false; policy.focused_pid = Some(0);
        assert!(!policy.allows(Some(0), Some(Duration::ZERO)));
        policy.focused_pid = Some(42); policy.version = 2;
        assert!(!policy.allows(Some(42), Some(Duration::ZERO)));
    }

    #[test]
    fn focus_tracks_the_native_realm_workspace_and_window_visibility() {
        let mut scene = crate::Scene::new("test");
        let browser = scene.add("Browser", "org.domed.Browser");
        window(&mut scene, browser).client_pid = 42;
        scene.focused = Some(browser);
        let allows = |scene: &crate::Scene, realm| Policy::for_scene(scene, realm, false).allows(Some(42), Some(Duration::ZERO));
        assert!(allows(&scene, crate::Realm::Desktop));
        window(&mut scene, browser).xr_visible = false;
        assert!(!allows(&scene, crate::Realm::Xr));
        window(&mut scene, browser).xr_visible = true;
        assert!(allows(&scene, crate::Realm::Xr));
        window(&mut scene, browser).minimized = true;
        assert!(!allows(&scene, crate::Realm::Xr));
        window(&mut scene, browser).minimized = false;
        scene.active_workspace = 1;
        assert!(!allows(&scene, crate::Realm::Desktop));
        scene.active_workspace = 0; scene.locked = true;
        assert!(!allows(&scene, crate::Realm::Desktop));
        scene.locked = false;
        assert!(!Policy::for_scene(&scene, crate::Realm::Desktop, true).allows(Some(42), Some(Duration::ZERO)));
    }

    #[test]
    fn focused_native_dialogs_block_page_input_but_passive_tooltips_do_not() {
        let mut scene = crate::Scene::new("test");
        let browser = scene.add("Browser", "org.domed.Browser");
        let dialog = scene.add("Native UI", "org.domed.Browser");
        for window in &mut scene.windows { window.client_pid = 42; }
        scene.popups.push(crate::spatial_popup::Popup { id: dialog, parent: browser, root: browser,
            anchor: crate::Rect { x: 0., y: 0., w: 100., h: 100. }, rollout: false, context_at: None, viewport: None });
        scene.focused = Some(browser);
        assert!(Policy::for_scene(&scene, crate::Realm::Desktop, false).allows(Some(42), Some(Duration::ZERO)));
        scene.popups[0].context_at = Some([0., 0.]);
        assert!(!Policy::for_scene(&scene, crate::Realm::Desktop, false).allows(Some(42), Some(Duration::ZERO)));
        scene.popups[0].context_at = None;
        scene.focused = Some(dialog);
        assert!(!Policy::for_scene(&scene, crate::Realm::Desktop, false).allows(Some(42), Some(Duration::ZERO)));
        scene.focused = Some(browser);
        window(&mut scene, browser).client_pid = 43;
        assert!(!Policy::for_scene(&scene, crate::Realm::Desktop, false).allows(Some(42), Some(Duration::ZERO)));
    }

    #[test]
    fn old_control_replies_do_not_acquire_controller_permission() {
        let mut value = serde_json::to_value(crate::xr_transport::Control::default()).unwrap();
        value.as_object_mut().unwrap().remove("webxr_input");
        let control: crate::xr_transport::Control = serde_json::from_value(value).unwrap();
        assert!(control.webxr_input.is_none());
    }

    #[test]
    fn a_fresh_http_reply_does_not_refresh_stale_native_ui_state() {
        let mut control = crate::xr_transport::Control::default();
        control.webxr_input = Some(Policy { version: 1, blocked: false, visible: true, focused_pid: Some(42) });
        assert!(!control_visible(&control, Some(Duration::ZERO)));
        control.webxr_input_age_ms = Some(300);
        assert!(!control_visible(&control, Some(Duration::ZERO)));
        control.webxr_input_age_ms = Some(100);
        assert!(control_visible(&control, Some(Duration::from_millis(100))));
        assert!(!control_visible(&control, Some(Duration::from_millis(200))));
        control.webxr_input.as_mut().unwrap().visible = false;
        assert!(!control_visible(&control, Some(Duration::ZERO)));
    }
}
