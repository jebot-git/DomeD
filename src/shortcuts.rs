//! Compositor-owned shortcuts. Ordinary Ctrl shortcuts remain with applications.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    ToggleXrBars,
    ToggleXrTop,
    ToggleXrBottom,
    ToggleXrDesktop,
    Appearance,
    KeyboardSettings,
    XrSettings,
    AnchorHere,
    StartXr,
    StopXr,
    Session(crate::appearance::SessionAction),
    Launch,
    Terminal,
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
    Scale(f32),
    Copy,
    Paste,
    SelectAll,
    Projection,
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
#[derive(Default, Clone, Copy)]
pub struct Mods {
    pub logo: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}
pub fn resolve(key: u32, m: Mods) -> Option<Command> {
    if m.alt && key == 0xffc1 {
        return Some(Command::Close);
    } // Alt+F4
    if key == 0xffc7 && !m.ctrl && !m.alt && !m.logo {
        return Some(Command::Menu);
    } // F10
    if key == 0xff09 && (m.alt || m.logo) {
        return Some(Command::Cycle(m.shift));
    }
    if !m.logo {
        return None;
    }
    if !m.ctrl {
        match char::from_u32(key).map(|c| c.to_ascii_lowercase()) {
            Some('b') if m.shift && !m.alt => return Some(Command::ToggleXrBars),
            Some('d') if m.shift && !m.alt => return Some(Command::ToggleXrDesktop),
            Some('t') if m.alt && !m.shift => return Some(Command::ToggleXrTop),
            Some('b') if m.alt && !m.shift => return Some(Command::ToggleXrBottom),
            _ => (),
        }
    }
    if m.ctrl {
        return match key {
            0x72 | 0x52 => Some(Command::AnchorHere),
            0xff51 => Some(Command::CycleWorkspace(-1)),
            0xff53 => Some(Command::CycleWorkspace(1)),
            0xff52 => Some(Command::MoveLane(-1)),
            0xff54 => Some(Command::MoveLane(1)),
            _ => None,
        };
    }
    if (b'1' as u32..=b'8' as u32).contains(&key) {
        let n = (key - b'1' as u32) as usize;
        return Some(if m.shift {
            Command::MoveWorkspace(n)
        } else {
            Command::Workspace(n)
        });
    }
    match char::from_u32(key).map(|v| v.to_ascii_lowercase()) {
        Some('p') => Some(Command::Projection),
        Some(',') => Some(Command::XrSettings),
        Some('t') if !m.alt && !m.shift => Some(Command::Terminal),
        Some(' ') if m.shift && !m.alt => Some(Command::Launch),
        Some('h') if m.alt => Some(Command::HideOthers),
        Some('h') => Some(Command::HideApp),
        Some('w') => Some(Command::Close),
        Some('q') => Some(Command::QuitApp),
        Some('m') => Some(Command::Minimize),
        Some('n') if m.shift => Some(Command::NewWorkspace),
        Some('r') if m.shift => Some(Command::ResetSpatial),
        Some('c') => Some(Command::Copy),
        Some('v') => Some(Command::Paste),
        Some('a') => Some(Command::SelectAll),
        Some('+') | Some('=') => Some(Command::Scale(1.1)),
        Some('-') => Some(Command::Scale(1. / 1.1)),
        _ if key == 0xff0d => Some(Command::Maximize),
        _ => None,
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
