//! Compositor-owned shortcuts. Ordinary Ctrl shortcuts remain with applications.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Command {
    ToggleXrBars,
    ToggleXrTop,
    ToggleXrBottom,
    ToggleXrDesktop,
    Appearance,
    DefaultApplications,
    KeyboardSettings,
    XrSettings,
    AnchorHere,
    StartXr,
    StopXr,
    Session(crate::appearance::SessionAction),
    Launch,
    Terminal,
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
    NewWorkspace,
    ResetSpatial,
    PullOut,
    Scale(f32),
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
            command,
            key,
            mods,
            enabled: true,
        })
    };
    add(
        Close,
        0xffc1,
        Mods {
            alt: true,
            ..Default::default()
        },
    );
    add(Menu, 0xffc7, logo);
    add(PullOut, 'p' as u32, logo);
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
        add(
            Cycle(reverse),
            0xff09,
            Mods {
                shift: reverse,
                ..logo
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
        (Close, 'w'),
        (QuitApp, 'q'),
        (Minimize, 'm'),
        (Copy, 'c'),
        (Paste, 'v'),
        (SelectAll, 'a'),
        (Scale(1.1), '='),
        (Scale(1.1), '+'),
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
        (MoveLane(-1), 0xff52),
        (MoveLane(1), 0xff54),
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
        if !b.enabled {
            continue;
        }
        if b.key == 0 || (0xffe1..=0xffee).contains(&b.key) {
            return Err("Choose a non-modifier key".into());
        }
        // Application function keys (including Ghostty's F12 / Super+F12) stay available.
        if ((0xffbe..=0xffe0).contains(&b.key)
            && !b.mods.logo
            && !(b.key == 0xffc1
                && b.mods
                    == Mods {
                        alt: true,
                        ..Default::default()
                    }))
            || b.key == 0xffc9
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
        ToggleXrDesktop => "Toggle desktop / XR".into(),
        XrSettings => "Settings".into(),
        AnchorHere => "Anchor dome here".into(),
        Launch => "Application launcher".into(),
        Terminal => "Ghostty XR terminal".into(),
        FileManager => "File manager".into(),
        Cycle(false) => "Next window".into(),
        Cycle(true) => "Previous window".into(),
        CycleWorkspace(n) => if n < 0 {
            "Previous desktop"
        } else {
            "Next desktop"
        }
        .into(),
        MoveLane(n) => if n < 0 {
            "Move up a lane"
        } else {
            "Move down a lane"
        }
        .into(),
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

/// Add the new action to saved pre-pullout profiles without taking an existing chord.
pub fn deserialize_bindings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Binding>, D::Error> {
    let mut bindings = <Vec<Binding> as serde::Deserialize>::deserialize(deserializer)?;
    if !bindings.iter().any(|b| b.command == Command::PullOut) {
        let mut binding = defaults()
            .into_iter()
            .find(|b| b.command == Command::PullOut)
            .unwrap();
        binding.enabled = !bindings
            .iter()
            .any(|b| b.enabled && normalized_key(b.key) == binding.key && b.mods == binding.mods);
        bindings.push(binding);
    }
    Ok(bindings)
}
