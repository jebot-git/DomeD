//! Compositor-owned shortcuts. Ordinary Ctrl shortcuts remain with applications.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Command {
    ToggleXrBars,
    ToggleXrTop,
    ToggleXrBottom,
    ToggleXrDesktop,
    SendToOtherRealm,
    SwitchInputRealm,
    PresentWindow { desktop: bool, xr: bool },
    Appearance,
    DefaultApplications,
    KeyboardSettings,
    XrSettings,
    KeybindingsHelp,
    AnchorHere,
    StartXr,
    StopXr,
    Session(crate::appearance::SessionAction),
    Launch,
    Terminal,
    Screenshot,
    FileManager,
    Desktops,
    Close,
    QuitApp,
    Minimize,
    Maximize,
    Cycle(bool),
    Workspace(usize),
    MoveWorkspace(usize),
    CycleWorkspace(i32),
    MoveLane(i32),
    CycleLane(i32),
    MoveHorizontal(i32),
    NewWorkspace,
    ResetSpatial,
    PullOut,
    Scale(f32),
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    About,
    Menu,
    Separator,
    AppInfo,
    HideApp,
    HideOthers,
    ShowAll,
    DesktopAction { app: usize, action: usize },
    LaunchApp(usize),
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mods {
    pub logo: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}
/// A user-editable chord. Disabled bindings retain their action in settings.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    // Stable identity lets profile migration preserve remapped/disabled aliases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub command: Command,
    pub key: u32,
    pub mods: Mods,
    pub enabled: bool,
}
impl Binding {
    pub fn label(&self) -> String {
        if !self.enabled {
            return "Disabled".into();
        }
        let key = match self.key {
            0xff61 => "PrintScreen".into(),
            0xff09 => "Tab".into(),
            0xff0d => "Enter".into(),
            0xff51 => "Left".into(),
            0xff52 => "Up".into(),
            0xff53 => "Right".into(),
            0xff54 => "Down".into(),
            0x20 => "Space".into(),
            k if (0xffbe..=0xffe0).contains(&k) => format!("F{}", k - 0xffbe + 1),
            k => char::from_u32(k)
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| format!("0x{k:x}")),
        };
        format!(
            "{}{}{}{}{}",
            if self.mods.logo { "Super+" } else { "" },
            if self.mods.ctrl { "Ctrl+" } else { "" },
            if self.mods.alt { "Alt+" } else { "" },
            if self.mods.shift { "Shift+" } else { "" },
            key
        )
    }
}
pub fn defaults() -> Vec<Binding> {
    use Command::*;
    let logo = Mods {
        logo: true,
        ..Default::default()
    };
    let mut bindings = Vec::new();
    let mut add = |command, key, mods| {
        bindings.push(Binding {
            id: None,
            command,
            key,
            mods,
            enabled: true,
        })
    };
    add(Close, 'w' as u32, logo);
    add(Screenshot, 0xff61, Mods::default());
    add(Menu, 0xffc7, logo);
    add(PullOut, 'p' as u32, logo);
    add(SendToOtherRealm, 'x' as u32, Mods { shift: true, ..logo });
    add(SwitchInputRealm, 'x' as u32, logo);
    for reverse in [false, true] {
        add(
            Cycle(reverse),
            0xff09,
            Mods {
                alt: true,
                shift: reverse,
                ..Default::default()
            },
        );
    }
    for (command, key, mods) in [
        (
            ToggleXrBars,
            'b',
            Mods {
                shift: true,
                ..logo
            },
        ),
        (
            ToggleXrDesktop,
            'd',
            Mods {
                shift: true,
                ..logo
            },
        ),
        (ToggleXrTop, 't', Mods { alt: true, ..logo }),
        (ToggleXrBottom, 'b', Mods { alt: true, ..logo }),
        (AnchorHere, 'r', Mods { ctrl: true, ..logo }),
        (
            Launch,
            ' ',
            Mods {
                shift: true,
                ..logo
            },
        ),
        (HideOthers, 'h', Mods { alt: true, ..logo }),
        (
            NewWorkspace,
            'n',
            Mods {
                shift: true,
                ..logo
            },
        ),
        (
            ResetSpatial,
            'r',
            Mods {
                shift: true,
                ..logo
            },
        ),
    ] {
        add(command, key as u32, mods);
    }
    for (command, key) in [
        (XrSettings, ','),
        (FileManager, 'e'),
        (Terminal, 't'),
        (HideApp, 'h'),
        (QuitApp, 'q'),
        (Minimize, 'm'),
        (Copy, 'c'),
        (Paste, 'v'),
        (SelectAll, 'a'),
        (Scale(1.1), '='),
        (Scale(1. / 1.1), '-'),
    ] {
        add(
            command,
            key as u32,
            Mods {
                shift: key == '+',
                ..logo
            },
        );
    }
    add(Maximize, 0xff0d, logo);
    for (command, key) in [
        (CycleWorkspace(-1), 0xff51),
        (CycleWorkspace(1), 0xff53),
    ] {
        add(command, key, Mods { ctrl: true, ..logo });
    }
    for n in 0..8 {
        add(Workspace(n), '1' as u32 + n as u32, logo);
        add(
            MoveWorkspace(n),
            '1' as u32 + n as u32,
            Mods {
                shift: true,
                ..logo
            },
        );
    }
    for (id, command, key) in [
        ("window-lane-up", MoveLane(-1), 0xff52),
        ("window-lane-down", MoveLane(1), 0xff54),
        ("window-right", MoveHorizontal(1), 0xff53),
        ("window-left", MoveHorizontal(-1), 0xff51),
    ] {
        bindings.push(Binding {
            id: Some(id.into()), command, key,
            mods: Mods { alt: true, ..logo }, enabled: true,
        });
    }
    for (id, command, key) in [
        ("focus-lane-up", CycleLane(-1), 0xff52),
        ("focus-lane-down", CycleLane(1), 0xff54),
    ] {
        bindings.push(Binding { id: Some(id.into()), command, key,
            mods: Mods { ctrl: true, alt: true, ..Default::default() }, enabled: true });
    }
    bindings
}
fn normalized_key(key: u32) -> u32 {
    if key == 0xfe20 {
        0xff09
    } else {
        char::from_u32(key)
            .map(|c| c.to_ascii_lowercase() as u32)
            .unwrap_or(key)
    }
}
pub fn resolve_with(bindings: &[Binding], key: u32, mods: Mods) -> Option<Command> {
    bindings
        .iter()
        .find(|b| b.enabled && normalized_key(b.key) == normalized_key(key) && b.mods == mods)
        .map(|b| b.command)
}
pub fn resolve(key: u32, mods: Mods) -> Option<Command> {
    resolve_with(&defaults(), key, mods)
}
pub fn validate(bindings: &[Binding]) -> Result<(), String> {
    if bindings.len() > 128 {
        return Err("Too many keybindings".into());
    }
    let available = defaults();
    for (i, b) in bindings.iter().enumerate() {
        if !available.iter().any(|d| d.command == b.command) {
            return Err("Unknown keybinding action".into());
        }
        if let Some(id) = &b.id {
            if !available.iter().any(|d| d.id.as_ref() == Some(id) && d.command == b.command)
                || bindings[..i].iter().any(|d| d.id.as_ref() == Some(id)) {
                return Err("Unknown or duplicate keybinding identity".into());
            }
        }
        if !b.enabled {
            continue;
        }
        if b.key == 0 || (0xffe1..=0xffee).contains(&b.key) {
            return Err("Choose a non-modifier key".into());
        }
        // Application function keys (including Ghostty's F12 / Super+F12) stay available.
        if ((0xffbe..=0xffe0).contains(&b.key) && !b.mods.logo) || b.key == 0xffc9
        {
            return Err("Function keys belong to applications; use a Super chord (F12 is reserved for Ghostty XR)".into());
        }
        if bindings[..i].iter().any(|a| {
            a.enabled && normalized_key(a.key) == normalized_key(b.key) && a.mods == b.mods
        }) {
            return Err(format!("{} is already assigned", b.label()));
        }
    }
    Ok(())
}
pub fn action_label(command: Command) -> String {
    use Command::*;
    match command {
        ToggleXrBars => "Toggle XR bars".into(),
        ToggleXrTop => "Toggle upper XR bar".into(),
        ToggleXrBottom => "Toggle XR taskbar".into(),
        SendToOtherRealm => "Send window to other desktop / XR".into(),
        SwitchInputRealm => "Switch desktop / XR input focus".into(),
        PresentWindow { desktop, xr } => format!("Window presentation: desktop={desktop}, XR={xr}"),
        ToggleXrDesktop => "Toggle desktop / XR".into(),
        XrSettings => "Settings".into(),
        KeybindingsHelp => "Keybinding help".into(),
        AnchorHere => "Anchor dome here".into(),
        Launch => "Application launcher".into(),
        Terminal => "Terminal (desktop / XR)".into(),
        Screenshot => "Screenshot (region / XR SBS)".into(),
        FileManager => "File manager".into(),
        Cycle(false) => "Next window".into(),
        Cycle(true) => "Previous window".into(),
        CycleWorkspace(n) => if n < 0 {
            "Previous desktop"
        } else {
            "Next desktop"
        }
        .into(),
        CycleLane(n) => if n < 0 { "Focus previous dome lane" } else { "Focus next dome lane" }.into(),
        MoveLane(n) => if n < 0 {
            "Move up a lane"
        } else {
            "Move down a lane"
        }
        .into(),
        MoveHorizontal(n) => if n < 0 { "Move left in lane" } else { "Move right in lane" }.into(),
        Workspace(n) => format!("Switch to desktop {}", n + 1),
        MoveWorkspace(n) => format!("Move to desktop {}", n + 1),
        NewWorkspace => "New desktop".into(),
        ResetSpatial => "Reset XR placement".into(),
        PullOut => "Pull window out / return to dome".into(),
        Scale(n) => if n > 1. {
            "Increase XR size"
        } else {
            "Decrease XR size"
        }
        .into(),
        HideApp => "Hide application".into(),
        HideOthers => "Hide other applications".into(),
        Close => "Close window".into(),
        QuitApp => "Quit application".into(),
        SelectAll => "Select all".into(),
        _ => format!("{command:?}"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xr_visibility_shortcuts_are_separate_from_app_hiding() {
        let shift = Mods {
            logo: true,
            shift: true,
            ..Default::default()
        };
        let alt = Mods {
            logo: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(resolve('B' as u32, shift), Some(Command::ToggleXrBars));
        assert_eq!(resolve('D' as u32, shift), Some(Command::ToggleXrDesktop));
        assert_eq!(resolve('t' as u32, alt), Some(Command::ToggleXrTop));
        assert_eq!(resolve('b' as u32, alt), Some(Command::ToggleXrBottom));
        assert_eq!(
            resolve(
                'b' as u32,
                Mods {
                    ctrl: true,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(resolve('h' as u32, alt), Some(Command::HideOthers));
    }
    #[test]
    fn ordinary_editing_is_not_reserved() {
        assert_eq!(
            resolve(
                'c' as u32,
                Mods {
                    ctrl: true,
                    ..Mods::default()
                }
            ),
            None
        );
    }
    #[test]
    fn desktop_and_window_chords() {
        let m = Mods {
            logo: true,
            ..Mods::default()
        };
        assert_eq!(resolve('w' as u32, m), Some(Command::Close));
        assert_eq!(resolve(0xffc1, Mods { alt: true, ..Default::default() }), None);
        assert_eq!(resolve('e' as u32, m), Some(Command::FileManager));
        assert_eq!(resolve('3' as u32, m), Some(Command::Workspace(2)));
        assert_eq!(
            resolve('3' as u32, Mods { shift: true, ..m }),
            Some(Command::MoveWorkspace(2))
        );
        assert_eq!(
            resolve(
                0xff09,
                Mods {
                    alt: true,
                    shift: true,
                    ..Mods::default()
                }
            ),
            Some(Command::Cycle(true))
        );
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    #[test]
    fn application_function_and_navigation_keys_are_forwarded() {
        let bindings = defaults();
        validate(&bindings).unwrap();
        for key in 0xffbe..=0xffe0 {
            for mods in [
                Mods::default(),
                Mods {
                    shift: true,
                    ..Default::default()
                },
                Mods {
                    ctrl: true,
                    ..Default::default()
                },
            ] {
                assert_eq!(
                    resolve_with(&bindings, key, mods),
                    None,
                    "application function key {key:x}"
                );
            }
        }
        assert_eq!(
            resolve(
                0xffc7,
                Mods {
                    logo: true,
                    ..Default::default()
                }
            ),
            Some(Command::Menu)
        );
        assert_eq!(
            resolve(
                0xffc9,
                Mods {
                    logo: true,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(
            resolve(
                0xff09,
                Mods {
                    ctrl: true,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(
            resolve(
                0xffc1,
                Mods {
                    alt: true,
                    ctrl: true,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(
            resolve(
                'p' as u32,
                Mods {
                    logo: true,
                    ..Default::default()
                }
            ),
            Some(Command::PullOut)
        );
        assert_eq!(
            resolve(
                0xfe20,
                Mods {
                    alt: true,
                    shift: true,
                    ..Default::default()
                }
            ),
            Some(Command::Cycle(true))
        );
    }
    #[test]
    fn rebindings_disable_old_chords_and_reject_conflicts() {
        let mut bindings = defaults();
        let index = bindings
            .iter()
            .position(|b| b.command == Command::Launch)
            .unwrap();
        let old = bindings[index].clone();
        bindings[index].key = 'l' as u32;
        validate(&bindings).unwrap();
        assert_eq!(resolve_with(&bindings, old.key, old.mods), None);
        assert_eq!(
            resolve_with(&bindings, 'L' as u32, old.mods),
            Some(Command::Launch)
        );
        bindings[index].enabled = false;
        assert_eq!(resolve_with(&bindings, 'l' as u32, old.mods), None);
        bindings[index] = bindings[0].clone();
        assert!(validate(&bindings).is_err());
        bindings[index] = old;
        bindings[index].key = 0xffc7;
        bindings[index].mods = Mods::default();
        assert!(validate(&bindings).is_err());
    }
}

/// Add new bindings without stealing chords or reviving disabled actions.
pub fn deserialize_bindings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Binding>, D::Error> {
    let mut bindings = <Vec<Binding> as serde::Deserialize>::deserialize(deserializer)?;
    // Retire stock aliases. Custom remaps keep their chord and replace the
    // canonical entry for the same action rather than producing another alias.
    let logo = Mods { logo: true, ..Default::default() };
    let alt = Mods { alt: true, ..logo };
    let ctrl = Mods { ctrl: true, ..logo };
    let canonical = defaults();
    let disabled: Vec<_> = bindings.iter().filter(|b| !bindings.iter().any(|other| other.command == b.command && other.enabled)).map(|b| b.command).collect();
    bindings.retain(|b| !(
        (b.command == Command::Close && b.key == 0xffc1 && b.mods == Mods { alt: true, ..Default::default() })
        || (matches!(b.command, Command::Cycle(_)) && b.key == 0xff09 && b.mods.logo && !b.mods.ctrl && !b.mods.alt)
        || (matches!(b.command, Command::MoveLane(_)) && b.mods == ctrl && [0xff52, 0xff54].contains(&b.key))
        || (matches!(b.id.as_deref(), Some("window-grow" | "window-shrink")) && b.mods == alt && [0xff51, 0xff53].contains(&b.key))
        || (b.command == Command::Scale(1.1) && b.key == '+' as u32 && b.mods == Mods { shift: true, ..logo })
    ));
    let mut unique: Vec<Binding> = Vec::new();
    for mut binding in bindings {
        if let Some(default) = canonical.iter().find(|b| b.command == binding.command) { binding.id = default.id.clone(); }
        if let Some(index) = unique.iter().position(|b| b.command == binding.command) {
            let stock = canonical.iter().any(|b| b.command == unique[index].command && b.key == unique[index].key && b.mods == unique[index].mods);
            if stock { unique[index] = binding; }
        } else { unique.push(binding); }
    }
    let mut bindings = unique;
    for mut binding in defaults().into_iter().filter(|b| b.id.is_some() || matches!(b.command, Command::Close | Command::PullOut | Command::SendToOtherRealm | Command::SwitchInputRealm | Command::Screenshot)) {
        let present = bindings.iter().any(|b| match &binding.id {
            Some(id) => b.id.as_ref() == Some(id),
            None => b.command == binding.command,
        });
        if present { continue; }
        let action_disabled = bindings.iter().any(|b| b.command == binding.command)
            && !bindings.iter().any(|b| b.command == binding.command && b.enabled);
        binding.enabled = !disabled.contains(&binding.command) && !action_disabled && !bindings.iter().any(|b| {
            b.enabled && normalized_key(b.key) == normalized_key(binding.key) && b.mods == binding.mods
        });
        bindings.push(binding);
    }
    Ok(bindings)
}

#[cfg(test)]
mod window_binding_tests {
    use super::*;
    fn restored(bindings: Vec<Binding>) -> Vec<Binding> {
        let c: crate::Config = serde_json::from_value(serde_json::json!({"keybindings": bindings})).unwrap();
        validate(&c.keybindings).unwrap();
        c.keybindings
    }
    #[test]
    fn window_arrows_and_existing_chords_resolve() {
        let alt = Mods { logo: true, alt: true, ..Default::default() };
        for (key, command) in [(0xff52, Command::MoveLane(-1)), (0xff54, Command::MoveLane(1)),
            (0xff53, Command::MoveHorizontal(1)), (0xff51, Command::MoveHorizontal(-1))] {
            assert_eq!(resolve(key, alt), Some(command));
        }
        assert_eq!(resolve(0xff52, Mods { logo: true, ctrl: true, ..Default::default() }), None);
        assert_eq!(resolve('=' as u32, Mods { logo: true, ..Default::default() }), Some(Command::Scale(1.1)));
        validate(&defaults()).unwrap();
    }
    #[test]
    fn old_profiles_gain_window_bindings_without_chord_conflicts() {
        let mut old: Vec<_> = defaults().into_iter().filter(|b| b.id.is_none()).collect();
        let menu = old.iter_mut().find(|b| b.command == Command::Menu).unwrap();
        menu.key = 0xff52;
        menu.mods = Mods { logo: true, alt: true, ..Default::default() };
        let old = restored(old);
        let added: Vec<_> = old.iter().filter(|b| b.id.is_some()).collect();
        assert_eq!(added.len(), 6);
        assert!(!added.iter().find(|b| b.command == Command::MoveLane(-1)).unwrap().enabled);
        assert!(added.iter().find(|b| b.command == Command::MoveHorizontal(1)).unwrap().enabled);
    }
    #[test]
    fn migration_preserves_remapped_and_disabled_window_bindings() {
        let mut bindings = defaults();
        let up = bindings.iter_mut().find(|b| b.id.as_deref() == Some("window-lane-up")).unwrap();
        up.key = 'u' as u32;
        let grow = bindings.iter_mut().find(|b| b.id.as_deref() == Some("window-right")).unwrap();
        grow.enabled = false;
        let restored = restored(restored(bindings.clone()));
        assert_eq!(restored, bindings);
        assert_eq!(resolve_with(&restored, 0xff52, Mods { logo: true, alt: true, ..Default::default() }), None);
    }
    #[test]
    fn migration_does_not_revive_a_disabled_action() {
        let mut old = defaults();
        old.iter_mut().filter(|b| b.command == Command::MoveLane(-1)).for_each(|b| b.enabled = false);
        assert!(!restored(old).iter().find(|b| b.id.as_deref() == Some("window-lane-up")).unwrap().enabled);
    }
}

#[cfg(test)]
mod retired_alias_tests {
    use super::*;
    #[test]
    fn defaults_have_one_chord_per_action_and_old_aliases_are_reclaimed() {
        let mut old = defaults();
        for i in 0..old.len() { assert!(!old[..i].iter().any(|b| b.command == old[i].command)); }
        old.push(Binding { id: Some("window-grow".into()), command: Command::Scale(1.1),
            key: 0xff53, mods: Mods { logo: true, alt: true, ..Default::default() }, enabled: true });
        old.push(Binding { id: None, command: Command::Close, key: 0xffc1,
            mods: Mods { alt: true, ..Default::default() }, enabled: true });
        let config: crate::Config = serde_json::from_value(serde_json::json!({"keybindings":old})).unwrap();
        validate(&config.keybindings).unwrap();
        assert_eq!(config.keybindings, defaults());
    }
}

#[cfg(test)]
mod input_switch_tests {
    use super::*;
    #[test]
    fn input_focus_and_window_transfer_have_distinct_chords() {
        let logo=Mods { logo:true,..Default::default() };
        assert_eq!(resolve('x' as u32,logo),Some(Command::SwitchInputRealm));
        assert_eq!(resolve('X' as u32,Mods {shift:true,..logo}),Some(Command::SendToOtherRealm));
        assert_eq!(resolve('x' as u32,Mods {ctrl:true,..logo}),None);
    }
    #[test]
    fn saved_profiles_receive_switch_without_stealing_a_custom_chord() {
        let mut old=defaults();old.retain(|b|b.command!=Command::SwitchInputRealm);
        let profile:crate::Config=serde_json::from_value(serde_json::json!({"keybindings":old})).unwrap();
        assert!(profile.keybindings.iter().any(|b|b.command==Command::SwitchInputRealm&&b.enabled));
        let terminal=old.iter_mut().find(|b|b.command==Command::Terminal).unwrap();terminal.key='x' as u32;
        let profile:crate::Config=serde_json::from_value(serde_json::json!({"keybindings":old})).unwrap();
        validate(&profile.keybindings).unwrap();
        assert!(profile.keybindings.iter().any(|b|b.command==Command::SwitchInputRealm&&!b.enabled));
    }
}
#[cfg(test)]
mod screenshot_tests {
    use super::*;
    #[test]
    fn print_screen_is_global_editable_and_keeps_application_keys_free() {
        assert_eq!(resolve(0xff61,Mods::default()),Some(Command::Screenshot));
        assert_eq!(resolve(0xffc9,Mods::default()),None);
        assert_eq!(defaults().iter().find(|b|b.command==Command::Screenshot).unwrap().label(),"PrintScreen");
        validate(&defaults()).unwrap();
    }
    #[test]
    fn migration_preserves_remaps_disabled_actions_and_existing_print_owner() {
        let mut old=defaults();old.retain(|b|b.command!=Command::Screenshot);
        let p:crate::Config=serde_json::from_value(serde_json::json!({"keybindings":old})).unwrap();
        assert_eq!(resolve_with(&p.keybindings,0xff61,Mods::default()),Some(Command::Screenshot));
        let terminal=old.iter_mut().find(|b|b.command==Command::Terminal).unwrap();
        terminal.key=0xff61;terminal.mods=Mods::default();
        let p:crate::Config=serde_json::from_value(serde_json::json!({"keybindings":old})).unwrap();
        validate(&p.keybindings).unwrap();
        assert!(p.keybindings.iter().any(|b|b.command==Command::Screenshot&&!b.enabled));
        let mut custom=defaults();let shot=custom.iter_mut().find(|b|b.command==Command::Screenshot).unwrap();
        shot.enabled=false;shot.key='s' as u32;shot.mods.logo=true;
        let p:crate::Config=serde_json::from_value(serde_json::json!({"keybindings":custom})).unwrap();
        assert_eq!(p.keybindings.iter().filter(|b|b.command==Command::Screenshot).count(),1);
        let shot=p.keybindings.iter().find(|b|b.command==Command::Screenshot).unwrap();
        assert!(!shot.enabled&&shot.key=='s' as u32&&shot.mods.logo);
    }
}

#[cfg(test)]
mod close_binding_tests {
    use super::*;
    fn restored(bindings: Vec<Binding>) -> Vec<Binding> {
        let config: crate::Config = serde_json::from_value(serde_json::json!({"keybindings": bindings})).unwrap();
        validate(&config.keybindings).unwrap();
        config.keybindings
    }
    #[test]
    fn window_close_leaves_control_w_and_alt_f4_to_clients() {
        assert_eq!(resolve('w' as u32, Mods { ctrl: true, ..Default::default() }), None);
        assert_eq!(resolve('w' as u32, Mods { logo: true, ..Default::default() }), Some(Command::Close));
        assert_eq!(resolve(0xffc1, Mods { alt: true, ..Default::default() }), None);
    }
    #[test]
    fn stock_close_migrates_idempotently_and_preserves_disabled_state() {
        for enabled in [true, false] {
            let mut old = defaults();
            let close = old.iter_mut().find(|b| b.command == Command::Close).unwrap();
            close.key = 0xffc1; close.mods = Mods { alt: true, ..Default::default() }; close.enabled = enabled;
            let migrated = restored(old);
            let close = migrated.iter().find(|b| b.command == Command::Close).unwrap();
            assert_eq!(close.key, 'w' as u32); assert!(close.mods.logo); assert_eq!(close.enabled, enabled);
            assert_eq!(restored(migrated.clone()), migrated);
        }
    }
    #[test]
    fn close_migration_keeps_custom_chords_and_avoids_conflicts() {
        let mut custom = defaults();
        custom.iter_mut().find(|b| b.command == Command::Close).unwrap().key = 'k' as u32;
        assert_eq!(restored(custom.clone()), custom);
        let mut old = defaults();
        old.retain(|b| b.command != Command::Close);
        old.iter_mut().find(|b| b.command == Command::Terminal).unwrap().key = 'w' as u32;
        let migrated = restored(old);
        assert_eq!(resolve_with(&migrated, 'w' as u32, Mods { logo: true, ..Default::default() }), Some(Command::Terminal));
        assert!(!migrated.iter().find(|b| b.command == Command::Close).unwrap().enabled);
    }
}
