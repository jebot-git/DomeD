use serde::{Deserialize, Serialize};

pub type WindowId = u64;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    InView,
    RoomDome,
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

/// Degrees are used at the configuration boundary; spatial calculations use radians.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub projection: Projection,
    pub dome_vertical_span_deg: f32,
    pub vertical_fov_deg: f32,
    pub upper_bar_height_deg: f32,
    pub taskbar_height_deg: f32,
    pub upper_bar_max_width_deg: f32,
    pub taskbar_max_width_deg: f32,
    pub upper_inset_deg: f32,
    pub lower_inset_deg: f32,
    pub radius_m: f32,
    pub center_m: [f32; 3],
    pub yaw_deg: f32,
    pub lane_gap_deg: f32,
    pub window_gap_deg: f32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            projection: Projection::RoomDome,
            dome_vertical_span_deg: 110.,
            vertical_fov_deg: 100.,
            upper_bar_height_deg: 5.,
            taskbar_height_deg: 7.,
            upper_bar_max_width_deg: 110.,
            taskbar_max_width_deg: 100.,
            upper_inset_deg: 8.,
            lower_inset_deg: 8.,
            radius_m: 2.2,
            center_m: [0., 1.4, 0.],
            yaw_deg: 0.,
            lane_gap_deg: 2.,
            window_gap_deg: 3.,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        let numbers = [
            self.dome_vertical_span_deg,
            self.vertical_fov_deg,
            self.upper_bar_height_deg,
            self.taskbar_height_deg,
            self.upper_bar_max_width_deg,
            self.taskbar_max_width_deg,
            self.upper_inset_deg,
            self.lower_inset_deg,
            self.radius_m,
            self.yaw_deg,
            self.lane_gap_deg,
            self.window_gap_deg,
        ];
        if numbers
            .iter()
            .chain(self.center_m.iter())
            .any(|x| !x.is_finite())
        {
            return Err("All geometry must be finite".into());
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
            Projection::InView => self.view_lane_extent(),
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Window {
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
    pub config: Config,
    pub view_pose: Pose,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
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
            active_workspace: 0,
            focused: None,
            revision: 0,
            source: source.into(),
            next_id: 1,
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
                self.detach(id);
                self.windows.retain(|w| w.id != id);
                if self.focused == Some(id) {
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
                w.desktop.x = x.clamp(-10000., 10000.);
                w.desktop.y = y.clamp(32., 10000.);
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
        self.revision += 1;
        Ok(())
    }
    pub fn frame_pose(&self) -> Pose {
        match self.config.projection {
            Projection::InView => self.view_pose,
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
        // In-view borders are constant screen pitch, keeping peripheral windows
        // inside the flat view-relative trays rather than bowing across them.
        let e = if self.config.projection == Projection::InView {
            (e.tan() * y.cos()).atan()
        } else {
            e
        };
        self.frame_pose().transform([
            radius * e.cos() * y.sin(),
            radius * e.sin(),
            -radius * e.cos() * y.cos(),
        ])
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
            let spans = self.config.lane_spans(space.shares);
            for (lane, band) in space.lanes.iter().enumerate() {
                let windows: Vec<_> = band
                    .windows
                    .iter()
                    .filter_map(|id| self.window(*id))
                    .filter(|w| !w.minimized)
                    .collect();
                let [bottom, top] = spans[lane];
                let height = top - bottom;
                let mut widths: Vec<f32> = windows
                    .iter()
                    .map(|w| {
                        (2. * ((height / 2.).to_radians().tan() * w.desktop.w / w.desktop.h).atan())
                            .to_degrees()
                            .clamp(14., 100.)
                    })
                    .collect();
                let total: f32 = widths.iter().sum();
                let gap = if windows.len() < 2 {
                    0.
                } else {
                    self.config.window_gap_deg.min(180. / windows.len() as f32)
                };
                let scale = ((360. - gap * windows.len() as f32) / total.max(1.)).min(1.);
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
                        let width = (2.
                            * (((t - b) / 2.).to_radians().tan() * w.desktop.w / w.desktop.h)
                                .atan())
                        .to_degrees()
                        .min(170.);
                        (b, t, width)
                    } else {
                        (bottom, top, widths[i])
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
    /// Intersect a room-space ray with active-workspace sphere patches. Hover never changes keyboard focus.
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
        let frame = self.frame_pose();
        let d = frame.inverse_rotate(direction.map(|v| v / len));
        let o = frame.inverse_rotate(std::array::from_fn::<_, 3, _>(|i| {
            origin[i] - frame.position_m[i]
        }));
        let dot = o.iter().zip(d).map(|(a, b)| a * b).sum::<f32>();
        let c = o.iter().map(|v| v * v).sum::<f32>() - self.config.radius_m.powi(2);
        let discriminant = dot * dot - c;
        if discriminant < 0. {
            return None;
        }
        let roots = [-dot - discriminant.sqrt(), -dot + discriminant.sqrt()];
        for distance in roots {
            if distance <= 0. {
                continue;
            }
            let p = std::array::from_fn::<_, 3, _>(|i| o[i] + d[i] * distance);
            let yaw = wrap(p[0].atan2(-p[2]).to_degrees());
            let el = if self.config.projection == Projection::InView {
                p[1].atan2(-p[2]).to_degrees()
            } else {
                (p[1] / self.config.radius_m)
                    .clamp(-1., 1.)
                    .asin()
                    .to_degrees()
            };
            let mut panels = self.panels();
            panels.sort_by_key(|p| {
                (
                    self.focused == Some(p.id),
                    self.window(p.id).is_some_and(|w| w.maximized),
                )
            });
            for panel in panels
                .iter()
                .rev()
                .filter(|p| p.workspace == self.active_workspace)
            {
                let dx = wrap(yaw - panel.yaw_deg);
                if dx.abs() <= panel.width_deg / 2. && el >= panel.bottom_deg && el <= panel.top_deg
                {
                    return Some(RayHit {
                        id: panel.id,
                        uv: [
                            dx / panel.width_deg + 0.5,
                            (panel.top_deg - el) / (panel.top_deg - panel.bottom_deg),
                        ],
                        distance_m: distance,
                    });
                }
            }
        }
        None
    }
    /// Curved mesh for an OpenXR renderer: u maps yaw, v maps elevation.
    pub fn mesh(&self, p: &Panel, segments: u32) -> Vec<[f32; 3]> {
        let count = segments.clamp(1, 256);
        let mut out = Vec::with_capacity((count as usize + 1) * 2);
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
