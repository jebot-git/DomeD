//! OpenXR controller actions owned by Dome's existing headset session.
use dome::webxr_bridge::{InputHandedness, Pose, TrackedInput};
use openxr as xr;

type Result<T> = std::result::Result<T, xr::sys::Result>;
const BUTTONS: usize = 7;
const AXES: usize = 4;

struct Profile {
    path: &'static str,
    names: &'static [&'static str],
    buttons: [&'static str; 4],
    touches: [&'static str; 4],
    axes: [&'static str; 4],
    faces: u8, // 0: none, 1: X/Y versus A/B, 2: A/B on both hands.
    thumb_rest: bool,
    menu_left_only: Option<bool>,
}

impl Profile {
    fn button(&self, index: usize, hand: usize) -> &'static str {
        if index < 4 { return self.buttons[index]; }
        match (self.faces, index, hand) {
            (1, 4, 0) => "x/click", (1, 5, 0) => "y/click",
            (1 | 2, 4, _) => "a/click", (1 | 2, 5, _) => "b/click",
            _ => "",
        }
    }
    fn touch(&self, index: usize, hand: usize) -> &'static str {
        if index < 4 { return self.touches[index]; }
        match (self.faces, index, hand) {
            (1, 4, 0) => "x/touch", (1, 5, 0) => "y/touch",
            (1 | 2, 4, _) => "a/touch", (1 | 2, 5, _) => "b/touch",
            (_, 6, _) if self.thumb_rest => "thumbrest/touch",
            _ => "",
        }
    }
    fn button_count(&self, hand: usize) -> usize {
        (0..BUTTONS).rfind(|&i| !self.button(i, hand).is_empty() || !self.touch(i, hand).is_empty())
            .map_or(0, |i| i + 1)
    }
    fn axis_count(&self) -> usize {
        (0..AXES).rfind(|&i| !self.axes[i].is_empty()).map_or(0, |i| i + 1)
    }
}

static PROFILES: &[Profile] = &[
    Profile { path: "/interaction_profiles/khr/simple_controller", names: &["generic-trigger"],
        buttons: ["select/click", "", "", ""], touches: [""; 4], axes: [""; 4], faces: 0, thumb_rest: false, menu_left_only: Some(false) },
    Profile { path: "/interaction_profiles/oculus/touch_controller", names: &["oculus-touch", "generic-trigger-squeeze-thumbstick"],
        buttons: ["trigger/value", "squeeze/value", "", "thumbstick/click"],
        touches: ["trigger/touch", "", "", "thumbstick/touch"],
        axes: ["", "", "thumbstick/x", "thumbstick/y"], faces: 1, thumb_rest: true, menu_left_only: Some(true) },
    Profile { path: "/interaction_profiles/valve/index_controller", names: &["valve-index", "generic-trigger-squeeze-touchpad-thumbstick"],
        buttons: ["trigger/value", "squeeze/value", "trackpad/force", "thumbstick/click"],
        touches: ["trigger/touch", "", "trackpad/touch", "thumbstick/touch"],
        axes: ["trackpad/x", "trackpad/y", "thumbstick/x", "thumbstick/y"], faces: 2, thumb_rest: false, menu_left_only: None },
    Profile { path: "/interaction_profiles/htc/vive_controller", names: &["htc-vive", "generic-trigger-squeeze-touchpad"],
        buttons: ["trigger/value", "squeeze/click", "trackpad/click", ""],
        touches: ["", "", "trackpad/touch", ""], axes: ["trackpad/x", "trackpad/y", "", ""], faces: 0, thumb_rest: false, menu_left_only: Some(false) },
    Profile { path: "/interaction_profiles/microsoft/motion_controller", names: &["microsoft-mixed-reality", "generic-trigger-squeeze-touchpad-thumbstick"],
        buttons: ["trigger/value", "squeeze/click", "trackpad/click", "thumbstick/click"],
        touches: ["", "", "trackpad/touch", ""], axes: ["trackpad/x", "trackpad/y", "thumbstick/x", "thumbstick/y"], faces: 0, thumb_rest: false, menu_left_only: Some(false) },
];

fn actions<T: xr::ActionTy, const N: usize>(set: &xr::ActionSet, prefix: &str, hands: &[xr::Path]) -> Result<[xr::Action<T>; N]> {
    let mut values = Vec::with_capacity(N);
    for i in 0..N {
        values.push(set.create_action(&format!("browser_{prefix}_{i}"), &format!("Browser {prefix} {i}"), hands)?);
    }
    values.try_into().map_err(|_| xr::sys::Result::ERROR_RUNTIME_FAILURE)
}

pub struct TimedInputs {
    pub time: xr::Time,
    pub inputs: Vec<TrackedInput>,
}
pub struct Sample {
    pub inputs: Vec<TrackedInput>,
    pub events: Vec<TimedInputs>,
}
#[derive(Clone, Copy)]
struct Transition { hand: usize, squeeze: bool, down: bool, time: xr::Time }

pub struct Controllers {
    // Spaces must be released before their actions/action set.
    aim_spaces: [xr::Space; 2],
    grip_spaces: [xr::Space; 2],
    aim: xr::Action<xr::Posef>,
    grip: xr::Action<xr::Posef>,
    select: xr::Action<bool>,
    squeeze: xr::Action<bool>,
    exit: xr::Action<bool>,
    buttons: [xr::Action<f32>; BUTTONS],
    touches: [xr::Action<bool>; BUTTONS],
    axes: [xr::Action<f32>; AXES],
    hands: [xr::Path; 2],
    set: xr::ActionSet,
    active: bool,
    cached: Vec<TrackedInput>,
    exit_down: [bool; 2],
    exit_requested: bool,
}

impl Controllers {
    pub fn new<G>(instance: &xr::Instance, session: &xr::Session<G>) -> Result<Self> {
        let hands = [instance.string_to_path("/user/hand/left")?, instance.string_to_path("/user/hand/right")?];
        let set = instance.create_action_set("dome_browser", "Dome Browser", 0)?;
        let aim = set.create_action("browser_aim", "Browser pointing pose", &hands)?;
        let grip = set.create_action("browser_grip", "Browser grip pose", &hands)?;
        let select = set.create_action("browser_select", "Browser primary action", &hands)?;
        let squeeze = set.create_action("browser_squeeze", "Browser squeeze action", &hands)?;
        let exit = set.create_action("browser_exit", "Leave immersive browsing", &hands)?;
        let buttons = actions(&set, "button", &hands)?;
        let touches = actions(&set, "touch", &hands)?;
        let axes = actions(&set, "axis", &hands)?;
        let mut suggested = 0;
        for profile in PROFILES {
            let mut bindings = Vec::new();
            for hand in 0..2 {
                let root = if hand == 0 { "/user/hand/left/input/" } else { "/user/hand/right/input/" };
                let path = |suffix: &str| instance.string_to_path(&format!("{root}{suffix}"));
                bindings.push(xr::Binding::new(&aim, path("aim/pose")?));
                bindings.push(xr::Binding::new(&grip, path("grip/pose")?));
                bindings.push(xr::Binding::new(&select, path(profile.buttons[0])?));
                if profile.menu_left_only.is_some_and(|left_only| !left_only || hand == 0) {
                    bindings.push(xr::Binding::new(&exit, path("menu/click")?));
                }
                if !profile.buttons[1].is_empty() { bindings.push(xr::Binding::new(&squeeze, path(profile.buttons[1])?)); }
                for i in 0..BUTTONS {
                    if !profile.button(i, hand).is_empty() { bindings.push(xr::Binding::new(&buttons[i], path(profile.button(i, hand))?)); }
                    if !profile.touch(i, hand).is_empty() { bindings.push(xr::Binding::new(&touches[i], path(profile.touch(i, hand))?)); }
                }
                for (i, axis) in profile.axes.iter().enumerate() {
                    if !axis.is_empty() { bindings.push(xr::Binding::new(&axes[i], path(axis)?)); }
                }
            }
            if instance.suggest_interaction_profile_bindings(instance.string_to_path(profile.path)?, &bindings).is_ok() { suggested += 1; }
        }
        if suggested == 0 { return Err(xr::sys::Result::ERROR_PATH_UNSUPPORTED); }
        session.attach_action_sets(&[&set])?;
        let aim_spaces = [aim.create_space(session, hands[0], xr::Posef::IDENTITY)?, aim.create_space(session, hands[1], xr::Posef::IDENTITY)?];
        let grip_spaces = [grip.create_space(session, hands[0], xr::Posef::IDENTITY)?, grip.create_space(session, hands[1], xr::Posef::IDENTITY)?];
        Ok(Self { aim_spaces, grip_spaces, aim, grip, select, squeeze, exit, buttons, touches, axes, hands, set, active: false, cached: vec![], exit_down: [false; 2], exit_requested: false })
    }

    pub fn neutral(&self) -> Vec<TrackedInput> {
        self.cached.iter().cloned().map(|mut input| {
            input.aim = None; input.grip = None; input.select = false; input.squeeze = false;
            input.buttons.fill(0.); input.axes.fill(0.); input.touches.fill(false); input
        }).collect()
    }

    pub fn idle<G>(&mut self, session: &xr::Session<G>) -> Result<()> {
        if self.active { session.sync_actions(&[])?; self.active = false; }
        self.cached.clear();
        self.exit_down = [false; 2]; self.exit_requested = false;
        Ok(())
    }

    pub fn take_exit_request(&mut self) -> bool { std::mem::take(&mut self.exit_requested) }

    pub fn sample<G>(&mut self, instance: &xr::Instance, session: &xr::Session<G>, stage: &xr::Space, time: xr::Time, focused: bool) -> Result<Sample> {
        if !focused { return Ok(Sample { inputs: self.neutral(), events: vec![] }); }
        session.sync_actions(&[(&self.set).into()])?;
        self.active = true;
        let mut inputs = Vec::new();
        let mut transitions = Vec::new();
        for hand in 0..2 {
            let path = self.hands[hand];
            if !self.aim.is_active(session, path)? { continue; }
            let name = instance.path_to_string(session.current_interaction_profile(path)?)?;
            let profile = PROFILES.iter().find(|profile| profile.path == name);
            let select = self.select.state(session, path)?;
            let squeeze = self.squeeze.state(session, path)?;
            let exit = self.exit.state(session, path)?;
            let exit_down = exit.is_active && exit.current_state;
            self.exit_requested |= exit_down && !self.exit_down[hand];
            self.exit_down[hand] = exit_down;
            let buttons = self.buttons.iter().map(|action| action.state(session, path)).collect::<Result<Vec<_>>>()?;
            let touches = self.touches.iter().map(|action| action.state(session, path)).collect::<Result<Vec<_>>>()?;
            let axes = self.axes.iter().map(|action| action.state(session, path)).collect::<Result<Vec<_>>>()?;
            let button_count = profile.map_or_else(|| (0..BUTTONS).rfind(|&i| buttons[i].is_active || touches[i].is_active).map_or(1, |i| i + 1), |p| p.button_count(hand));
            let axis_count = profile.map_or_else(|| (0..AXES).rfind(|&i| axes[i].is_active).map_or(0, |i| i + 1), Profile::axis_count);
            let values: Vec<_> = buttons.iter().take(button_count).map(|state| value(state.is_active, state.current_state, 0., 1.)).collect();
            let touched: Vec<_> = touches.iter().take(button_count).enumerate().map(|(i, state)| (state.is_active && state.current_state) || values[i] > 0.).collect();
            let axis_values = axes.iter().take(axis_count).enumerate()
                .map(|(i, state)| axis_value(state.is_active, state.current_state, i, touched.get(2).copied().unwrap_or(false))).collect();
            let supports_grip = profile.is_some() || self.grip.is_active(session, path)?;
            let mut input = TrackedInput {
                id: hand as u32 + 1,
                handedness: if hand == 0 { InputHandedness::Left } else { InputHandedness::Right },
                profiles: profile.map_or_else(|| vec!["generic-trigger".into()], |p| p.names.iter().map(|s| (*s).into()).collect()),
                supports_grip,
                aim: located(&self.aim_spaces[hand], stage, time)?,
                grip: if supports_grip { located(&self.grip_spaces[hand], stage, time)? } else { None },
                select: select.is_active && select.current_state,
                squeeze: squeeze.is_active && squeeze.current_state,
                select_active: select.is_active,
                squeeze_active: squeeze.is_active,
                buttons: values, touches: touched, axes: axis_values,
            };
            if let Some(previous) = self.cached.iter().find(|old| same_source(old, &input)) {
                for (squeeze, before, down, state) in [(false, previous.select, input.select, select), (true, previous.squeeze, input.squeeze, squeeze)] {
                    let (transition, active) = action_transition(hand, squeeze, before, down, &state, time);
                    if let Some(transition) = transition { transitions.push(transition); }
                    if squeeze { input.squeeze_active = active; } else { input.select_active = active; }
                }
            }
            inputs.push(input);
        }
        let mut events = transition_samples(&self.cached, &inputs, transitions);
        for event in &mut events {
            for input in &mut event.inputs {
                let hand = (input.id - 1) as usize;
                input.aim = located(&self.aim_spaces[hand], stage, event.time)?;
                input.grip = if input.supports_grip { located(&self.grip_spaces[hand], stage, event.time)? } else { None };
            }
        }
        self.cached = inputs.clone();
        Ok(Sample { inputs, events })
    }
}

fn action_transition(hand: usize, squeeze: bool, before: bool, down: bool, state: &xr::ActionState<bool>, display: xr::Time) -> (Option<Transition>, bool) {
    if !state.is_active || before == down { return (None, state.is_active); }
    if state.changed_since_last_sync && state.last_change_time.as_nanos() > 0 && state.last_change_time.as_nanos() <= display.as_nanos() {
        (Some(Transition { hand, squeeze, down, time: state.last_change_time }), true)
    } else {
        // An absent action timestamp cannot be replaced with predicted display
        // time. Cancel/rearm this action while other controls remain available.
        (None, false)
    }
}

fn same_source(a: &TrackedInput, b: &TrackedInput) -> bool {
    a.id == b.id && a.handedness == b.handedness && a.profiles == b.profiles
        && a.supports_grip == b.supports_grip && a.buttons.len() == b.buttons.len() && a.axes.len() == b.axes.len()
}

fn transition_samples(previous: &[TrackedInput], current: &[TrackedInput], mut transitions: Vec<Transition>) -> Vec<TimedInputs> {
    transitions.sort_by_key(|event| event.time.as_nanos());
    let mut state = current.to_vec();
    for input in &mut state {
        if let Some(old) = previous.iter().find(|old| same_source(old, input)) {
            input.select = old.select; input.squeeze = old.squeeze;
            for i in 0..2.min(input.buttons.len()) {
                input.buttons[i] = old.buttons[i];
                if let Some(touch) = input.touches.get_mut(i) { *touch = old.touches.get(i).copied().unwrap_or(false); }
            }
        }
    }
    let mut events: Vec<TimedInputs> = vec![];
    for transition in transitions {
        let Some(input) = state.iter_mut().find(|input| input.id == transition.hand as u32 + 1) else { continue; };
        if transition.squeeze { input.squeeze = transition.down; } else { input.select = transition.down; }
        let slot = usize::from(transition.squeeze);
        if let Some(latest) = current.iter().find(|latest| latest.id == input.id) {
            if let (Some(value), Some(latest)) = (input.buttons.get_mut(slot), latest.buttons.get(slot)) { *value = *latest; }
            if let (Some(touch), Some(latest)) = (input.touches.get_mut(slot), latest.touches.get(slot)) { *touch = *latest; }
        }
        // Simultaneous actions share one complete snapshot, so processing one
        // control cannot temporarily undo another control at that same instant.
        if let Some(last) = events.last_mut().filter(|event| event.time == transition.time) { last.inputs = state.clone(); }
        else { events.push(TimedInputs { time: transition.time, inputs: state.clone() }); }
    }
    events
}

fn value(active: bool, value: f32, min: f32, max: f32) -> f32 {
    if active && value.is_finite() { value.clamp(min, max) } else { 0. }
}

fn axis_value(active: bool, raw: f32, index: usize, touchpad_touched: bool) -> f32 {
    if index < 2 && !touchpad_touched { return 0.; }
    let normalized = value(active, raw, -1., 1.);
    if normalized == 0. { 0. } else { normalized * if index % 2 == 1 { -1. } else { 1. } }
}

fn located(space: &xr::Space, stage: &xr::Space, time: xr::Time) -> Result<Option<Pose>> {
    let location = space.locate(stage, time)?;
    if !location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID) { return Ok(None); }
    let pose = super::bridge_pose(location.pose);
    Ok(pose.valid().then_some(pose))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn controller(id: u32) -> TrackedInput {
        TrackedInput { id, handedness: InputHandedness::Left, profiles: vec!["generic-trigger-squeeze".into()],
            supports_grip: true, aim: Some(Pose::default()), grip: Some(Pose::default()), select: false, squeeze: false,
            select_active: true, squeeze_active: true, buttons: vec![0.,0.], touches: vec![false;2], axes: vec![] }
    }
    #[test]
    fn action_events_use_runtime_occurrence_time_and_missing_times_cancel_instead_of_predicting() {
        let mut state = xr::ActionState {current_state:true,changed_since_last_sync:true,last_change_time:xr::Time::from_nanos(15),is_active:true};
        let (event,active)=action_transition(0,false,false,true,&state,xr::Time::from_nanos(20));
        assert!(active);assert_eq!(event.unwrap().time.as_nanos(),15);
        for time in [0,21] {state.last_change_time=xr::Time::from_nanos(time);let(e,a)=action_transition(0,false,false,true,&state,xr::Time::from_nanos(20));assert!(e.is_none());assert!(!a);}
        state.is_active=false;assert!(!action_transition(0,false,true,false,&state,xr::Time::from_nanos(20)).1);
    }
    #[test]
    fn transitions_are_chronological_and_do_not_expose_later_actions_in_an_earlier_snapshot() {
        let old=controller(1);let mut current=old.clone();current.select=true;current.squeeze=true;current.buttons=vec![1.,1.];current.touches=vec![true;2];
        let changes=vec![Transition{hand:0,squeeze:true,down:true,time:xr::Time::from_nanos(20)},Transition{hand:0,squeeze:false,down:true,time:xr::Time::from_nanos(10)}];
        let samples=transition_samples(&[old],&[current],changes);
        assert_eq!(samples.len(),2);assert_eq!(samples[0].time.as_nanos(),10);assert_eq!(samples[0].inputs[0].buttons,[1.,0.]);
        assert!(samples[0].inputs[0].select);assert!(!samples[0].inputs[0].squeeze);assert!(samples[1].inputs[0].squeeze);
    }
    #[test]
    fn simultaneous_transitions_share_one_complete_snapshot_across_hands() {
        let old=[controller(1),controller(2)];let mut current=old.clone();current[0].select=true;current[1].squeeze=true;
        let changes=vec![Transition{hand:0,squeeze:false,down:true,time:xr::Time::from_nanos(10)},Transition{hand:1,squeeze:true,down:true,time:xr::Time::from_nanos(10)}];
        let samples=transition_samples(&old,&current,changes);assert_eq!(samples.len(),1);assert!(samples[0].inputs[0].select);assert!(samples[0].inputs[1].squeeze);
    }
    #[test]
    fn profiles_preserve_reserved_slots_trim_trailing_placeholders_and_exclude_system_buttons() {
        let expected = [(1, 0), (7, 4), (6, 4), (3, 2), (4, 4)];
        for (profile, (buttons, axes)) in PROFILES.iter().zip(expected) {
            for hand in 0..2 {
                assert_eq!(profile.button_count(hand), buttons); assert_eq!(profile.axis_count(), axes);
                for i in 0..BUTTONS {
                    assert!(!profile.button(i, hand).contains("menu"));
                    assert!(!profile.button(i, hand).contains("system"));
                }
            }
        }
        assert_eq!(PROFILES[1].button(2, 0), "");
        assert_eq!(PROFILES[1].button(4, 0), "x/click");
        assert_eq!(PROFILES[1].button(4, 1), "a/click");
        assert_eq!(PROFILES[1].touch(6, 0), "thumbrest/touch");
    }
    #[test]
    fn inactive_and_nonfinite_analog_values_are_neutral() {
        assert_eq!(value(false, 0.8, 0., 1.), 0.);
        assert_eq!(value(true, f32::NAN, 0., 1.), 0.);
        assert_eq!(value(true, 2., -1., 1.), 1.);
        assert_eq!(value(true, -2., -1., 1.), -1.);
        assert_eq!(axis_value(true, 0.7, 0, false), 0.);
        assert_eq!(axis_value(true, 0.7, 1, true), -0.7);
        assert_eq!(axis_value(true, 0., 3, false).to_bits(), 0.0f32.to_bits());
    }
}
