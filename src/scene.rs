use serde::{Deserialize, Serialize};

pub type WindowId = u64;
fn default_scale() -> f32 {
    1.
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    #[serde(alias = "in_view")]
    RoomDome,
}
/// XR pointing source; click gestures use the same tracked hands in either mode.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum XrPointerMode {
    #[default]
    HandRay,
    Gaze,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pose {
    pub position_m: [f32; 3],
    pub orientation_xyzw: [f32; 4],
}
impl Default for Pose {
    fn default() -> Self {
        Self {
            position_m: [0., 1.4, 0.],
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
}
impl Pose {
    pub fn validate(&self) -> Result<(), String> {
        let q = self.orientation_xyzw.iter().map(|x| x * x).sum::<f32>();
        if self
            .position_m
            .iter()
            .chain(self.orientation_xyzw.iter())
            .any(|v| !v.is_finite())
            || self.position_m.iter().any(|v| v.abs() > 10000.)
            || (q - 1.).abs() > 0.001
        {
            Err("Pose requires finite position and a unit quaternion".into())
        } else {
            Ok(())
        }
    }
    pub fn rotate(&self, v: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.orientation_xyzw;
        let t = [
            2. * (y * v[2] - z * v[1]),
            2. * (z * v[0] - x * v[2]),
            2. * (x * v[1] - y * v[0]),
        ];
        [
            v[0] + w * t[0] + y * t[2] - z * t[1],
            v[1] + w * t[1] + z * t[0] - x * t[2],
            v[2] + w * t[2] + x * t[1] - y * t[0],
        ]
    }
    pub fn transform(&self, v: [f32; 3]) -> [f32; 3] {
        let r = self.rotate(v);
        std::array::from_fn(|i| r[i] + self.position_m[i])
    }
    pub fn inverse_rotate(&self, v: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.orientation_xyzw;
        Self {
            position_m: [0.; 3],
            orientation_xyzw: [-x, -y, -z, w],
        }
        .rotate(v)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AnchorMode {
    #[default]
    Room,
    Head,
}

/// Persistent connector preferences; names include the GPU's PCI address.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    pub name: String,
    pub enabled: bool,
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
    pub x: Option<i32>,
    pub y: i32,
    pub primary: bool,
}
impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            width: 0,
            height: 0,
            refresh: 0,
            x: None,
            y: 0,
            primary: false,
        }
    }
}
/// Degrees are used at the configuration boundary; spatial calculations use radians.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub displays: Vec<DisplayConfig>,
    pub projection: Projection,
    pub xr_pointer_mode: XrPointerMode,
    pub gaze_smoothing: bool,
    pub keybindings: Vec<crate::shortcuts::Binding>,
    pub anchor_mode: AnchorMode,
    pub upper_bar_x_deg: f32,
    pub taskbar_x_deg: f32,
    pub bar_distance_m: f32,
    pub dome_vertical_span_deg: f32,
    pub dome_upper_tilt_deg: f32,
    pub vertical_fov_deg: f32,
    pub upper_bar_height_deg: f32,
    pub taskbar_height_deg: f32,
    pub upper_bar_max_width_deg: f32,
    pub taskbar_max_width_deg: f32,
    pub upper_inset_deg: f32,
    pub lower_inset_deg: f32,
    pub radius_m: f32,
    pub pullout_distance_m: f32,
    pub center_m: [f32; 3],
    pub yaw_deg: f32,
    pub lane_gap_deg: f32,
    /// Legacy angular preference, retained when reading older profiles.
    pub window_gap_deg: f32,
    pub window_gap_px: f32,
    pub window_border_px: u8,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            displays: Vec::new(),
            projection: Projection::RoomDome,
            xr_pointer_mode: XrPointerMode::HandRay,
            gaze_smoothing: true,
            keybindings: crate::shortcuts::defaults(),
            anchor_mode: AnchorMode::Room,
            upper_bar_x_deg: 0.,
            taskbar_x_deg: 0.,
            bar_distance_m: 0.6,
            dome_vertical_span_deg: 110.,
            dome_upper_tilt_deg: 8.,
            vertical_fov_deg: 100.,
            upper_bar_height_deg: 5.,
            taskbar_height_deg: 7.,
            upper_bar_max_width_deg: 110.,
            taskbar_max_width_deg: 100.,
            upper_inset_deg: 8.,
            lower_inset_deg: 8.,
            radius_m: 2.2,
            pullout_distance_m: 1.0,
            center_m: [0., 1.4, 0.],
            yaw_deg: 0.,
            lane_gap_deg: 2.,
            window_gap_deg: 0.5,
            window_gap_px: 8.,
            window_border_px: 1,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        crate::shortcuts::validate(&self.keybindings)?;
        let mut names = std::collections::HashSet::new();
        for display in &self.displays {
            if display.name.is_empty()
                || !names.insert(&display.name)
                || display.width < 0
                || display.height < 0
                || display.width > 16384
                || display.height > 16384
                || !(0..=1000000).contains(&display.refresh)
                || display.x.is_some_and(|x| x.unsigned_abs() > 65536)
                || display.y.unsigned_abs() > 65536
            {
                return Err("Invalid display configuration".into());
            }
        }
        let numbers = [
            self.upper_bar_x_deg,
            self.taskbar_x_deg,
            self.bar_distance_m,
            self.dome_vertical_span_deg,
            self.dome_upper_tilt_deg,
            self.vertical_fov_deg,
            self.upper_bar_height_deg,
            self.taskbar_height_deg,
            self.upper_bar_max_width_deg,
            self.taskbar_max_width_deg,
            self.upper_inset_deg,
            self.lower_inset_deg,
            self.radius_m,
            self.pullout_distance_m,
            self.yaw_deg,
            self.lane_gap_deg,
            self.window_gap_deg,
            self.window_gap_px,
        ];
        if self.upper_bar_x_deg.abs() > 40.
            || self.taskbar_x_deg.abs() > 40.
            || !(0.3..=2.).contains(&self.bar_distance_m)
        {
            return Err(
                "Bar horizontal offsets must be within ±40°; distance must be 0.3–2 m".into(),
            );
        }
        if !(0. ..=8.).contains(&self.window_gap_px) || self.window_border_px > 2 {
            return Err("XR spacing must be 0–8 logical pixels; border must be 0–2 pixels".into());
        }
        if numbers
            .iter()
            .chain(self.center_m.iter())
            .any(|x| !x.is_finite())
        {
            return Err("All geometry must be finite".into());
        }
        if !(0. ..=20.).contains(&self.dome_upper_tilt_deg) {
            return Err("Upper dome lean must be 0–20 degrees".into());
        }
        if !(30. ..=170.).contains(&self.dome_vertical_span_deg) {
            return Err("Dome span must be 30–170 degrees".into());
        }
        if !(30. ..=170.).contains(&self.vertical_fov_deg) {
            return Err("Vertical FOV must be 30–170 degrees".into());
        }
        if !(0.6..=8.).contains(&self.radius_m) {
            return Err("Radius must be 0.6–8 metres".into());
        }
        if !(0.6..=1.6).contains(&self.pullout_distance_m) {
            return Err("Pull-out distance must be 0.6–1.6 metres".into());
        }
        if self.center_m.iter().any(|x| x.abs() > 10000.) {
            return Err("Anchor position is outside the supported room range".into());
        }
        if !(1. ..=20.).contains(&self.upper_bar_height_deg)
            || !(1. ..=20.).contains(&self.taskbar_height_deg)
        {
            return Err("Bar heights must be 1–20 degrees".into());
        }
        if !(10. ..=360.).contains(&self.upper_bar_max_width_deg)
            || !(10. ..=360.).contains(&self.taskbar_max_width_deg)
        {
            return Err("Bar widths must be 10–360 degrees".into());
        }
        if self.upper_inset_deg < 0.
            || self.lower_inset_deg < 0.
            || !(0. ..=8.).contains(&self.lane_gap_deg)
            || !(0. ..=12.).contains(&self.window_gap_deg)
        {
            return Err(
                "Insets and gaps must be nonnegative; lane/window gaps are limited to 8/12 degrees"
                    .into(),
            );
        }
        let (bottom, top) = self.view_lane_extent();
        if top - bottom - 2. * self.lane_gap_deg < 18. {
            return Err(
                "FOV, bar heights and insets must leave at least 18 degrees for three lanes".into(),
            );
        }
        Ok(())
    }
    pub fn lane_extent(&self) -> (f32, f32) {
        match self.projection {
            Projection::RoomDome => (
                -self.dome_vertical_span_deg / 2.,
                self.dome_vertical_span_deg / 2.,
            ),
        }
    }
    pub fn view_lane_extent(&self) -> (f32, f32) {
        (
            -self.vertical_fov_deg / 2. + self.lower_inset_deg + self.taskbar_height_deg,
            self.vertical_fov_deg / 2. - self.upper_inset_deg - self.upper_bar_height_deg,
        )
    }
    pub fn lane_spans(&self, shares: [f32; 3]) -> [[f32; 2]; 3] {
        let (bottom, top) = self.lane_extent();
        let available = top - bottom - 2. * self.lane_gap_deg;
        let mut edge = top;
        std::array::from_fn(|i| {
            let end = edge;
            edge -= available * shares[i];
            let start = edge;
            edge -= self.lane_gap_deg;
            [start, end]
        })
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Realm {
    #[default]
    Desktop,
    Xr,
}
fn default_visible() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Window {
    #[serde(default)]
    pub stereo: crate::stereo::Stereo,
    #[serde(default)]
    pub origin: Realm,
    #[serde(default = "default_visible")]
    pub desktop_visible: bool,
    #[serde(default)]
    pub pullout_slot: Option<u8>,
    #[serde(default)]
    pub pullout_restore_scale: Option<f32>,
    #[serde(default)]
    pub dome_offset_deg: [f32; 2],
    #[serde(default)]
    pub xr_pose: Option<Pose>,
    #[serde(default = "default_scale")]
    pub xr_scale: f32,
    pub id: WindowId,
    pub title: String,
    pub app_id: String,
    pub workspace: usize,
    pub lane: usize,
    pub minimized: bool,
    pub maximized: bool,
    pub desktop: Rect,
    pub restore: Option<Rect>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lane {
    pub windows: Vec<WindowId>,
    pub rotation_deg: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub shares: [f32; 3],
    pub lanes: [Lane; 3],
}
impl Workspace {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            shares: [0.23, 0.54, 0.23],
            lanes: std::array::from_fn(|_| Lane {
                windows: vec![],
                rotation_deg: 0.,
            }),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Panel {
    pub id: WindowId,
    pub workspace: usize,
    pub lane: usize,
    pub yaw_deg: f32,
    pub bottom_deg: f32,
    pub top_deg: f32,
    pub width_deg: f32,
    pub radius_m: f32,
    pub opacity: f32,
    pub center_m: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RayHit {
    pub id: WindowId,
    pub uv: [f32; 2],
    pub distance_m: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default)]
    pub presentation: crate::xr_presentation::Presentation,
    #[serde(default)]
    pub pullout_anchor: Option<Pose>,
    pub config: Config,
    pub view_pose: Pose,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    #[serde(default)]
    pub popups: Vec<crate::spatial_popup::Popup>,
    pub active_workspace: usize,
    pub focused: Option<WindowId>,
    pub revision: u64,
    pub source: String,
    #[serde(skip)]
    next_id: WindowId,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    XrPresentation {
        top: Option<bool>,
        bottom: Option<bool>,
        desktop: Option<bool>,
    },
    PullOut {
        id: WindowId,
        head: Pose,
    },
    ReturnToDome {
        id: WindowId,
    },
    DomeOffset {
        id: WindowId,
        yaw_deg: f32,
        pitch_deg: f32,
    },
    Defocus,
    SpatialTransform {
        id: WindowId,
        pose: Option<Pose>,
        scale: f32,
    },
    Add {
        title: String,
        app_id: String,
    },
    Close {
        id: WindowId,
    },
    Focus {
        id: WindowId,
    },
    Minimize {
        id: WindowId,
    },
    Maximize {
        id: WindowId,
    },
    Move {
        id: WindowId,
        lane: usize,
        workspace: usize,
    },
    Swap {
        a: WindowId,
        b: WindowId,
    },
    SwapLanes {
        a: usize,
        b: usize,
    },
    ResizeBoundary {
        boundary: usize,
        delta_deg: f32,
    },
    SwitchWorkspace {
        workspace: usize,
    },
    Recenter {
        id: WindowId,
    },
    Configure {
        config: Config,
    },
    ViewPose {
        pose: Pose,
    },
    AddWorkspace {
        name: String,
    },
    RenameWorkspace {
        workspace: usize,
        name: String,
    },
    RemoveWorkspace {
        workspace: usize,
        move_to: usize,
    },
    DesktopMove {
        id: WindowId,
        x: f32,
        y: f32,
    },
    DesktopResize {
        id: WindowId,
        w: f32,
        h: f32,
    },
}
impl Default for Scene {
    fn default() -> Self {
        Self::new("preview")
    }
}
impl Scene {
    pub fn new(source: &str) -> Self {
        Self {
            config: Config::default(),
            view_pose: Pose::default(),
            workspaces: vec![
                Workspace::new("Studio"),
                Workspace::new("Research"),
                Workspace::new("Quiet"),
            ],
            windows: vec![],
            popups: vec![],
            active_workspace: 0,
            focused: None,
            revision: 0,
            source: source.into(),
            presentation: Default::default(),
            next_id: 1,
            pullout_anchor: None,
        }
    }
    pub fn demo() -> Self {
        let mut s = Self::default();
        for (title, app, lane, workspace) in [
            ("Project notes", "Notes", 1, 0),
            ("Terminal", "Terminal", 1, 0),
            ("Spatial design", "Browser", 1, 0),
            ("Music", "Music", 2, 0),
            ("Performance", "Monitor", 0, 0),
            ("Protocol reference", "Browser", 1, 1),
            ("Materials", "Files", 2, 1),
            ("Reading room", "Reader", 1, 2),
        ] {
            let id = s.add(title, app);
            s.relocate(id, lane, workspace).unwrap();
        }
        s.active_workspace = 0;
        s.focused = Some(3);
        s.recenter(3).unwrap();
        s
    }
    pub fn add(&mut self, title: &str, app_id: &str) -> WindowId {
        let id = self.next_id;
        self.next_id += 1;
        let n = self.windows.len() as f32;
        self.windows.push(Window {
            stereo: crate::stereo::Stereo::for_app_id(app_id),
            origin: Realm::Desktop,
            desktop_visible: true,
            pullout_slot: None,
            pullout_restore_scale: None,
            dome_offset_deg: [0., 0.],
            xr_pose: None,
            xr_scale: 1.,
            id,
            title: title.chars().take(160).collect(),
            app_id: app_id.chars().take(160).collect(),
            workspace: self.active_workspace,
            lane: 1,
            minimized: false,
            maximized: false,
            desktop: Rect {
                x: 100. + (n * 32.) % 250.,
                y: 80. + (n * 28.) % 160.,
                w: 720.,
                h: 460.,
            },
            restore: None,
        });
        // Insert at the default position; send the previous central window towards the less populated side.
        let lane = &mut self.workspaces[self.active_workspace].lanes[1];
        lane.windows.insert(lane.windows.len() / 2, id);
        self.focused = Some(id);
        self.recenter(id).ok();
        self.revision += 1;
        id
    }
    /// Preserve clients and promote hidden XR windows to the primary desktop after loss.
    pub fn desktop_fallback(&mut self, width: f32, height: f32) -> usize {
        let ids: Vec<_> = self
            .windows
            .iter()
            .filter(|w| !w.desktop_visible)
            .map(|w| w.id)
            .collect();
        for (n, id) in ids.iter().enumerate() {
            let lane = self.window(*id).unwrap().lane;
            self.relocate(*id, lane, 0).unwrap();
            let w = self.window_mut(*id).unwrap();
            w.desktop_visible = true;
            w.minimized = false;
            w.maximized = false;
            w.desktop.w = w.desktop.w.min((width - 32.).max(100.));
            w.desktop.h = w.desktop.h.min((height - 132.).max(100.));
            w.desktop.x = (24. + (n % 8) as f32 * 28.).min((width - w.desktop.w - 8.).max(8.));
            w.desktop.y = (72. + (n % 8) as f32 * 28.).min((height - w.desktop.h - 68.).max(36.));
        }
        if !ids.is_empty() {
            self.active_workspace = 0;
            self.focused = ids.last().copied();
            self.revision += 1;
        }
        ids.len()
    }
    /// A desktop shell view keeps XR-only clients out of its task list and focus menus.
    pub fn desktop_scene(&self) -> Self {
        let mut scene = self.clone();
        scene.windows.retain(|w| w.desktop_visible);
        if scene.focused.is_some_and(|id| scene.window(id).is_none()) {
            scene.focused = None;
        }
        scene
    }
    pub fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }
    fn window_mut(&mut self, id: WindowId) -> Result<&mut Window, String> {
        self.windows
            .iter_mut()
            .find(|w| w.id == id)
            .ok_or_else(|| "Unknown window".into())
    }
    fn detach(&mut self, id: WindowId) {
        for ws in &mut self.workspaces {
            for lane in &mut ws.lanes {
                lane.windows.retain(|v| *v != id);
            }
        }
    }
    fn relocate(&mut self, id: WindowId, lane: usize, workspace: usize) -> Result<(), String> {
        if lane >= 3 || workspace >= self.workspaces.len() {
            return Err("Invalid lane or workspace".into());
        }
        self.window(id).ok_or("Unknown window")?;
        self.detach(id);
        let w = self.window_mut(id)?;
        w.lane = lane;
        w.workspace = workspace;
        self.workspaces[workspace].lanes[lane].windows.push(id);
        self.recenter(id)?;
        Ok(())
    }
    fn recenter(&mut self, id: WindowId) -> Result<(), String> {
        let w = self.window(id).ok_or("Unknown window")?;
        let (ws, lane) = (w.workspace, w.lane);
        if let Some(p) = self.panels().iter().find(|p| p.id == id) {
            self.workspaces[ws].lanes[lane].rotation_deg =
                wrap(self.workspaces[ws].lanes[lane].rotation_deg - p.yaw_deg);
        }
        Ok(())
    }
    fn choose_focus(&mut self) {
        self.focused = self
            .windows
            .iter()
            .rev()
            .find(|w| !w.minimized && w.workspace == self.active_workspace)
            .map(|w| w.id);
    }
    pub fn apply(&mut self, action: Action) -> Result<(), String> {
        match action {
            Action::XrPresentation {
                top,
                bottom,
                desktop,
            } => {
                if let Some(v) = top {
                    self.presentation.top = v;
                }
                if let Some(v) = bottom {
                    self.presentation.bottom = v;
                }
                if let Some(v) = desktop {
                    self.presentation.desktop = v;
                }
            }
            Action::PullOut { id, head } => {
                head.validate()?;
                let w = self.window(id).ok_or("Unknown window")?;
                if w.minimized || w.workspace != self.active_workspace {
                    return Err("Panel is not visible".into());
                }
                if w.pullout_slot.is_some() {
                    return Ok(());
                }
                let slot = (0..2)
                    .find(|slot| !self.windows.iter().any(|w| w.pullout_slot == Some(*slot)))
                    .ok_or("Both pull-out slots are occupied")?;
                if self.pullout_anchor.is_none() {
                    let forward = glam::Vec3::from_array(head.rotate([0., 0., -1.]));
                    let yaw = if forward.x * forward.x + forward.z * forward.z > 0.001 {
                        (-forward.x).atan2(-forward.z)
                    } else {
                        0.
                    };
                    self.pullout_anchor = Some(Pose {
                        position_m: head.position_m,
                        orientation_xyzw: glam::Quat::from_rotation_y(yaw).to_array(),
                    });
                }
                let w = self.window_mut(id)?;
                w.pullout_slot = Some(slot);
                w.pullout_restore_scale = Some(w.xr_scale);
                w.xr_scale = 1.;
                w.xr_pose = None;
            }
            Action::ReturnToDome { id } => {
                let w = self.window_mut(id)?;
                w.pullout_slot = None;
                w.xr_pose = None;
                w.xr_scale = w.pullout_restore_scale.take().unwrap_or(1.);
            }
            Action::DomeOffset {
                id,
                yaw_deg,
                pitch_deg,
            } => {
                if !yaw_deg.is_finite() || !pitch_deg.is_finite() {
                    return Err("Dome offset must be finite".into());
                }
                let w = self.window_mut(id)?;
                w.dome_offset_deg = [wrap(yaw_deg), pitch_deg.clamp(-70., 70.)];
            }
            Action::SpatialTransform { id, pose, scale } => {
                if let Some(p) = pose {
                    p.validate()?;
                }
                if !scale.is_finite() || !(0.25..=4.).contains(&scale) {
                    return Err("XR scale must be 0.25–4".into());
                }
                let w = self.window_mut(id)?;
                w.xr_pose = pose;
                w.xr_scale = scale;
            }
            Action::Defocus => {
                self.focused = None;
            }
            Action::Add { title, app_id } => {
                if self.source != "preview" {
                    return Err("Live windows must come from Wayland clients".into());
                }
                if self.windows.len() >= 64 {
                    return Err("Preview limit is 64 windows".into());
                }
                self.add(&title, &app_id);
            }
            Action::Close { id } => {
                self.window(id).ok_or("Unknown window")?;
                let mut closing = vec![id];
                for _ in 0..self.popups.len() {
                    for popup in &self.popups {
                        if closing.contains(&popup.parent) && !closing.contains(&popup.id) {
                            closing.push(popup.id);
                        }
                    }
                }
                for id in &closing {
                    self.detach(*id);
                }
                self.windows.retain(|w| !closing.contains(&w.id));
                self.popups.retain(|p| !closing.contains(&p.id));
                if self.focused.is_some_and(|id| closing.contains(&id)) {
                    self.choose_focus();
                }
            }
            Action::Focus { id } => {
                let w = self.window_mut(id)?;
                w.minimized = false;
                self.active_workspace = w.workspace;
                self.focused = Some(id);
            }
            Action::Minimize { id } => {
                let (ws, lane) = {
                    let w = self.window_mut(id)?;
                    w.minimized = true;
                    (w.workspace, w.lane)
                };
                if self.focused == Some(id) {
                    self.choose_focus();
                }
                if let Some(next) = self.workspaces[ws].lanes[lane]
                    .windows
                    .iter()
                    .rev()
                    .find(|v| self.window(**v).is_some_and(|w| !w.minimized))
                    .copied()
                {
                    self.recenter(next)?;
                }
            }
            Action::Maximize { id } => {
                let w = self.window_mut(id)?;
                w.maximized = !w.maximized;
                w.minimized = false;
                self.active_workspace = w.workspace;
                self.focused = Some(id);
            }
            Action::Move {
                id,
                lane,
                workspace,
            } => {
                self.relocate(id, lane, workspace)?;
                if self.window(id).unwrap().workspace != self.active_workspace
                    && self.focused == Some(id)
                {
                    self.choose_focus();
                }
            }
            Action::Swap { a, b } => {
                let wa = self.window(a).ok_or("Unknown first window")?.clone();
                let wb = self.window(b).ok_or("Unknown second window")?.clone();
                for ws in &mut self.workspaces {
                    for lane in &mut ws.lanes {
                        for id in &mut lane.windows {
                            if *id == a {
                                *id = b;
                            } else if *id == b {
                                *id = a;
                            }
                        }
                    }
                }
                let w = self.window_mut(a)?;
                w.workspace = wb.workspace;
                w.lane = wb.lane;
                w.desktop = wb.desktop;
                let w = self.window_mut(b)?;
                w.workspace = wa.workspace;
                w.lane = wa.lane;
                w.desktop = wa.desktop;
            }
            Action::SwapLanes { a, b } => {
                if a >= 3 || b >= 3 {
                    return Err("Invalid lane".into());
                }
                let ws = &mut self.workspaces[self.active_workspace];
                ws.lanes.swap(a, b);
                for w in &mut self.windows {
                    if w.workspace == self.active_workspace {
                        if w.lane == a {
                            w.lane = b;
                        } else if w.lane == b {
                            w.lane = a;
                        }
                    }
                }
            }
            Action::ResizeBoundary {
                boundary,
                delta_deg,
            } => {
                if boundary >= 2 || !delta_deg.is_finite() {
                    return Err("Invalid boundary".into());
                }
                let (bottom, top) = self.config.lane_extent();
                let available = top - bottom - 2. * self.config.lane_gap_deg;
                let before = self.panels();
                let shares = &mut self.workspaces[self.active_workspace].shares;
                let d = delta_deg / available;
                let min = 4. / available;
                if shares[boundary] + d < min || shares[boundary + 1] - d < min {
                    return Err("Each lane must retain at least 4 degrees".into());
                }
                shares[boundary] += d;
                shares[boundary + 1] -= d;
                self.preserve_lane_centres(&before);
            }
            Action::SwitchWorkspace { workspace } => {
                if workspace >= self.workspaces.len() {
                    return Err("Invalid workspace".into());
                }
                self.active_workspace = workspace;
                self.choose_focus();
            }
            Action::Recenter { id } => {
                self.recenter(id)?;
            }
            Action::Configure { config } => {
                config.validate()?;
                let before = self.panels();
                self.config = config;
                self.preserve_lane_centres(&before);
            }
            Action::ViewPose { pose } => {
                pose.validate()?;
                self.view_pose = pose;
            }
            Action::AddWorkspace { name } => {
                if self.workspaces.len() >= 8 {
                    return Err("At most eight desktops".into());
                }
                let name = name.trim();
                if name.is_empty() || name.chars().count() > 32 {
                    return Err("Desktop name must contain 1–32 characters".into());
                }
                self.workspaces.push(Workspace::new(name));
            }
            Action::RenameWorkspace { workspace, name } => {
                let name = name.trim();
                if name.is_empty() || name.chars().count() > 32 {
                    return Err("Desktop name must contain 1–32 characters".into());
                }
                self.workspaces
                    .get_mut(workspace)
                    .ok_or("Invalid workspace")?
                    .name = name.into();
            }
            Action::RemoveWorkspace { workspace, move_to } => {
                if self.workspaces.len() <= 1
                    || workspace == move_to
                    || workspace >= self.workspaces.len()
                    || move_to >= self.workspaces.len()
                {
                    return Err("Choose a different existing destination desktop".into());
                }
                let ids: Vec<_> = self
                    .windows
                    .iter()
                    .filter(|w| w.workspace == workspace)
                    .map(|w| (w.id, w.lane))
                    .collect();
                for (id, lane) in ids {
                    self.relocate(id, lane, move_to)?;
                }
                self.workspaces.remove(workspace);
                for w in &mut self.windows {
                    if w.workspace > workspace {
                        w.workspace -= 1;
                    }
                }
                if self.active_workspace == workspace {
                    self.active_workspace = move_to;
                }
                if self.active_workspace > workspace {
                    self.active_workspace -= 1;
                }
                self.choose_focus();
            }
            Action::DesktopMove { id, x, y } => {
                if !x.is_finite() || !y.is_finite() {
                    return Err("Invalid position".into());
                }
                let w = self.window_mut(id)?;
                w.desktop.x = x.clamp(-65536., 65536.);
                w.desktop.y = y.clamp(-65536., 65536.);
            }
            Action::DesktopResize { id, w, h } => {
                if !w.is_finite() || !h.is_finite() {
                    return Err("Invalid size".into());
                }
                let r = &mut self.window_mut(id)?.desktop;
                r.w = w.clamp(160., 4096.);
                r.h = h.clamp(100., 4096.);
            }
        }
        if self.focused.is_some_and(|id| {
            self.window(id)
                .is_none_or(|w| w.minimized || w.workspace != self.active_workspace)
        }) {
            self.choose_focus();
        }
        if !self.windows.iter().any(|w| w.pullout_slot.is_some()) {
            self.pullout_anchor = None;
        }
        self.revision += 1;
        Ok(())
    }
    pub fn frame_pose(&self) -> Pose {
        match self.config.projection {
            Projection::RoomDome if self.config.anchor_mode == AnchorMode::Head => self.view_pose,
            Projection::RoomDome => {
                let a = self.config.yaw_deg.to_radians() / 2.;
                Pose {
                    position_m: self.config.center_m,
                    orientation_xyzw: [0., -a.sin(), 0., a.cos()],
                }
            }
        }
    }
    pub fn point(&self, yaw: f32, elevation: f32, radius: f32) -> [f32; 3] {
        let y = yaw.to_radians();
        let e = elevation.to_radians();
        self.frame_pose()
            .transform([radius * y.sin(), radius * e, -radius * y.cos()])
    }
    fn preserve_lane_centres(&mut self, before: &[Panel]) {
        let after = self.panels();
        for ws in 0..self.workspaces.len() {
            for lane in 0..3 {
                if let Some(old) = before
                    .iter()
                    .filter(|p| p.workspace == ws && p.lane == lane)
                    .min_by(|a, b| a.yaw_deg.abs().total_cmp(&b.yaw_deg.abs()))
                {
                    if let Some(new) = after.iter().find(|p| p.id == old.id) {
                        let r = &mut self.workspaces[ws].lanes[lane].rotation_deg;
                        *r = wrap(*r + old.yaw_deg - new.yaw_deg);
                    }
                }
            }
        }
    }
    pub fn panels(&self) -> Vec<Panel> {
        let mut panels = vec![];
        for (ws, space) in self.workspaces.iter().enumerate() {
            let rank = if ws == self.active_workspace {
                0
            } else {
                1 + (ws + self.workspaces.len() - self.active_workspace) % self.workspaces.len()
            };
            let radius = self.config.radius_m + rank as f32 * 0.55;
            let raw_spans = self.config.lane_spans(space.shares);
            // Pixels mean Wayland logical content pixels, independent of headset resolution.
            let height_at = |bottom: f32, top: f32| {
                let angular = top - bottom;
                {
                    angular * (2.2 / radius).min(1.)
                }
            };
            let heights = raw_spans.map(|[b, t]| height_at(b, t));
            let pixel_pitch = (0..3)
                .flat_map(|lane| {
                    space.lanes[lane]
                        .windows
                        .iter()
                        .filter_map(|id| self.window(*id))
                        .filter(|w| !w.minimized && self.popup(w.id).is_none())
                        .map(move |w| heights[lane] / w.desktop.h)
                })
                .fold(f32::INFINITY, f32::min);
            let pixel_pitch = if pixel_pitch.is_finite() {
                pixel_pitch
            } else {
                heights.iter().copied().fold(f32::INFINITY, f32::min) / 460.
            };
            let lane_gap = self
                .config
                .lane_gap_deg
                .min(self.config.window_gap_px * pixel_pitch);
            let mut mid = (raw_spans[1][0] + raw_spans[1][1]) / 2.;
            if radius > 2.2 {
                mid *= 2.2 / radius;
            }
            let middle = [mid - heights[1] / 2., mid + heights[1] / 2.];
            let spans = [
                [middle[1] + lane_gap, middle[1] + lane_gap + heights[0]],
                middle,
                [middle[0] - lane_gap - heights[2], middle[0] - lane_gap],
            ];
            for (lane, band) in space.lanes.iter().enumerate() {
                let windows: Vec<_> = band
                    .windows
                    .iter()
                    .filter_map(|id| self.window(*id))
                    .filter(|w| !w.minimized && self.popup(w.id).is_none())
                    .collect();
                let [bottom, top] = spans[lane];
                let height = top - bottom;
                let mut widths: Vec<f32> = windows
                    .iter()
                    .map(|w| height * w.desktop.w / w.desktop.h)
                    .collect();
                let total: f32 = widths.iter().sum();
                let gap = if windows.len() < 2 {
                    0.
                } else {
                    self.config.window_gap_px
                        * widths
                            .iter()
                            .zip(&windows)
                            .map(|(width, w)| width / w.desktop.w)
                            .fold(f32::INFINITY, f32::min)
                };
                let mut scale = (360. / (total + gap * windows.len() as f32).max(1.)).min(1.);
                {
                    scale = scale.min(175. / widths.iter().copied().fold(1., f32::max));
                }
                let gap = gap * scale;
                for width in &mut widths {
                    *width *= scale;
                }
                let mut edge = if widths.is_empty() {
                    0.
                } else {
                    -widths[0] / 2.
                };
                for (i, w) in windows.iter().enumerate() {
                    let yaw = wrap(edge + widths[i] / 2. + band.rotation_deg);
                    edge += widths[i] + gap;
                    let (b, t, width) = if w.maximized {
                        let (b, t) = self.config.lane_extent();
                        let h = height_at(b, t);
                        let (b, t) = ((b + t - h) / 2., (b + t + h) / 2.);
                        {
                            let width = ((t - b) * w.desktop.w / w.desktop.h).min(175.);
                            let height = width * w.desktop.h / w.desktop.w;
                            ((b + t - height) / 2., (b + t + height) / 2., width)
                        }
                    } else {
                        let mid = (bottom + top) / 2.;
                        (
                            mid - height * scale / 2.,
                            mid + height * scale / 2.,
                            widths[i],
                        )
                    };
                    panels.push(Panel {
                        id: w.id,
                        workspace: ws,
                        lane,
                        yaw_deg: yaw,
                        bottom_deg: b,
                        top_deg: t,
                        width_deg: width,
                        radius_m: radius,
                        opacity: if rank == 0 {
                            1.
                        } else {
                            0.16 / (rank as f32).sqrt()
                        },
                        center_m: self.point(yaw, (b + t) / 2., radius),
                    });
                }
            }
        }
        panels
    }
    /// Intersect a room-space ray with the same surfaces submitted to OpenXR. Hover never changes keyboard focus.
    pub fn hit_test(&self, origin: [f32; 3], direction: [f32; 3]) -> Option<RayHit> {
        if origin
            .iter()
            .chain(direction.iter())
            .any(|x| !x.is_finite())
        {
            return None;
        }
        let len = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        if len < 1e-6 {
            return None;
        }
        {
            let origin = glam::Vec3::from_array(origin);
            let direction = glam::Vec3::from_array(direction) / len;
            return crate::xr_input::quads(self, self.view_pose)
                .iter()
                .filter_map(|q| {
                    q.hit(origin, direction, false).map(|uv| RayHit {
                        id: q.id,
                        uv,
                        distance_m: q.point(uv).distance(origin),
                    })
                })
                .min_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
        }
    }
    /// Mesh using the native cylinder and upper-lane lean; no pole convergence.
    pub fn mesh(&self, p: &Panel, segments: u32) -> Vec<[f32; 3]> {
        let count = segments.clamp(1, 256);
        let mut out = Vec::with_capacity((count as usize + 1) * 2);
        {
            let mut nominal = self.clone();
            nominal.focused = None;
            nominal.active_workspace = p.workspace;
            if let Some(q) = crate::xr_input::quads(&nominal, self.view_pose)
                .iter()
                .find(|q| q.id == p.id)
            {
                for i in 0..=count {
                    for v in [0., 1.] {
                        out.push(q.point([i as f32 / count as f32, v]).to_array());
                    }
                }
                return out;
            }
        }
        for i in 0..=count {
            let yaw = p.yaw_deg - p.width_deg / 2. + p.width_deg * i as f32 / count as f32;
            for el in [p.top_deg, p.bottom_deg] {
                out.push(self.point(yaw, el, p.radius_m));
            }
        }
        out
    }
}
pub fn wrap(angle: f32) -> f32 {
    (angle + 180.).rem_euclid(360.) - 180.
}
