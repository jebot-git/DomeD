use dome::spatial_popup::Popup;
use dome::{Action, Pose, Realm, Scene};

#[test]
fn transfer_is_exclusive_preserves_identity_and_both_placements() {
    let mut scene = Scene::new("test");
    let id = scene.add("document", "editor");
    let other = scene.add("another document", "editor");
    scene
        .apply(Action::PullOut {
            id,
            head: Pose::default(),
        })
        .unwrap();
    let before = scene.window(id).unwrap().clone();
    scene
        .apply(Action::SendToRealm {
            id,
            realm: Realm::Desktop,
        })
        .unwrap();
    assert!(!scene.panels().iter().any(|p| p.id == id));
    assert!(!dome::xr_input::quads(&scene, Pose::default())
        .iter()
        .any(|p| p.id == id));
    assert!(scene.desktop_scene().window(id).is_some());
    assert!(!dome::input_routing::cycle_windows(&scene, Realm::Xr).contains(&id));
    assert_eq!(
        dome::input_routing::restore_focus(&scene, Some(Some(id)), Realm::Xr),
        Some(other)
    );
    scene
        .apply(Action::SendToRealm {
            id,
            realm: Realm::Xr,
        })
        .unwrap();
    assert!(scene.desktop_scene().window(id).is_none());
    assert!(scene.panels().iter().any(|p| p.id == id));
    assert!(!dome::input_routing::cycle_windows(&scene, Realm::Desktop).contains(&id));
    assert_eq!(
        dome::input_routing::restore_focus(&scene, Some(Some(id)), Realm::Desktop),
        Some(other)
    );
    let after = scene.window(id).unwrap();
    assert_eq!(after.desktop, before.desktop);
    assert_eq!(after.origin, before.origin);
    assert_eq!(after.xr_pose, before.xr_pose);
    assert_eq!(after.xr_scale, before.xr_scale);
    assert_eq!(after.pullout_slot, before.pullout_slot);
    assert_eq!(after.dome_offset_deg, before.dome_offset_deg);
    assert!(scene.window(other).unwrap().desktop_visible);
    assert!(scene.window(other).unwrap().xr_visible);
}

#[test]
fn nested_dialogs_follow_owner_even_when_transfer_starts_on_dialog() {
    let mut scene = Scene::new("test");
    let root = scene.add("owner", "app");
    let dialog = scene.add("dialog", "app");
    let nested = scene.add("nested", "app");
    for (id, parent) in [(dialog, root), (nested, dialog)] {
        let anchor = scene.window(id).unwrap().desktop;
        scene.popups.push(Popup { rollout: false,
            id,
            parent,
            root,
            anchor,
            context_at: None,
            viewport: None,
        });
    }
    scene
        .apply(Action::SendToRealm {
            id: nested,
            realm: Realm::Desktop,
        })
        .unwrap();
    for id in [root, dialog, nested] {
        assert!(scene.window(id).unwrap().desktop_visible);
        assert!(!scene.window(id).unwrap().xr_visible);
    }
    assert!(!scene.popup_visible(scene.popup(dialog).unwrap(), false));
    assert!(scene.popup_visible(scene.popup(dialog).unwrap(), true));
    scene
        .apply(Action::SendToRealm {
            id: dialog,
            realm: Realm::Xr,
        })
        .unwrap();
    assert!(!scene.popup_visible(scene.popup(nested).unwrap(), true));
    assert!(scene.popup_visible(scene.popup(nested).unwrap(), false));
    assert!(scene
        .apply(Action::SendToRealm {
            id: 9999,
            realm: Realm::Xr
        })
        .is_err());
}

#[test]
fn old_layouts_remain_shared_and_new_layouts_persist_exclusive_presentation() {
    let mut scene = Scene::new("test");
    let id = scene.add("saved", "app");
    let mut old = serde_json::to_value(scene.window(id).unwrap()).unwrap();
    old.as_object_mut().unwrap().remove("xr_visible");
    let old: dome::Window = serde_json::from_value(old).unwrap();
    assert!(old.desktop_visible && old.xr_visible);
    scene
        .apply(Action::SendToRealm {
            id,
            realm: Realm::Desktop,
        })
        .unwrap();
    let layout = dome::session_state::Layout::capture(&scene, &Default::default());
    let layout: dome::session_state::Layout =
        serde_json::from_value(serde_json::to_value(layout).unwrap()).unwrap();
    let mut restored = Scene::new("test");
    layout.prepare(&mut restored);
    let new_id = restored.add("saved", "app");
    layout.place(
        &mut restored,
        &layout.windows[0],
        new_id,
        &Default::default(),
    );
    assert!(restored.window(new_id).unwrap().desktop_visible);
    assert!(!restored.window(new_id).unwrap().xr_visible);
    restored
        .apply(Action::SendToRealm {
            id: new_id,
            realm: Realm::Xr,
        })
        .unwrap();
    assert_eq!(restored.desktop_fallback(1280., 800.), 1);
    assert!(restored.window(new_id).unwrap().desktop_visible);
}

#[test]
fn shortcut_migration_preserves_conflicts_remaps_and_disabled_binding() {
    use dome::shortcuts::{self, Command, Mods};
    let chord = Mods {
        logo: true,
        shift: true,
        ..Default::default()
    };
    assert_eq!(
        shortcuts::resolve('X' as u32, chord),
        Some(Command::SendToOtherRealm)
    );
    let mut old = shortcuts::defaults();
    old.retain(|b| b.command != Command::SendToOtherRealm);
    let other = old.iter_mut().find(|b| b.command == Command::Menu).unwrap();
    other.key = 'x' as u32;
    other.mods = chord;
    let config: dome::Config =
        serde_json::from_value(serde_json::json!({"keybindings":old})).unwrap();
    shortcuts::validate(&config.keybindings).unwrap();
    assert!(
        !config
            .keybindings
            .iter()
            .find(|b| b.command == Command::SendToOtherRealm)
            .unwrap()
            .enabled
    );
    let mut bindings = shortcuts::defaults();
    let binding = bindings
        .iter_mut()
        .find(|b| b.command == Command::SendToOtherRealm)
        .unwrap();
    binding.key = 'z' as u32;
    binding.enabled = false;
    let config: dome::Config =
        serde_json::from_value(serde_json::json!({"keybindings":bindings})).unwrap();
    let binding = config
        .keybindings
        .iter()
        .find(|b| b.command == Command::SendToOtherRealm)
        .unwrap();
    assert_eq!(binding.key, 'z' as u32);
    assert!(!binding.enabled);
}

#[test]
fn explicit_both_restores_window_and_dialogs_without_changing_geometry() {
    let mut scene = Scene::new("test");
    let id = scene.add("owner", "app");
    let dialog = scene.add("dialog", "app");
    scene.popups.push(Popup { rollout: false, id: dialog, parent: id, root: id,
        anchor: scene.window(dialog).unwrap().desktop, context_at: None, viewport: None });
    let desktop = scene.window(id).unwrap().desktop;
    let pose = scene.window(id).unwrap().xr_pose;
    scene.apply(Action::SendToRealm { id, realm: Realm::Xr }).unwrap();
    scene.apply(Action::WindowVisibility { id: dialog, desktop: true, xr: true }).unwrap();
    for id in [id, dialog] {
        let w = scene.window(id).unwrap();
        assert!(w.desktop_visible && w.xr_visible);
    }
    assert_eq!(scene.window(id).unwrap().desktop, desktop);
    assert_eq!(scene.window(id).unwrap().xr_pose, pose);
    assert!(scene.desktop_scene().window(id).is_some());
    assert!(scene.panels().iter().any(|p| p.id == id));
    let before = scene.revision;
    assert!(scene.apply(Action::WindowVisibility { id, desktop: false, xr: false }).is_err());
    assert_eq!(scene.revision, before);
    let layout = dome::session_state::Layout::capture(&scene, &Default::default());
    let saved = serde_json::to_value(layout).unwrap();
    assert!(saved["windows"].as_array().unwrap().iter().all(|w| w["desktop_visible"] == true && w["xr_visible"] == true));
}
