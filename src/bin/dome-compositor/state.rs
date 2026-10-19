use std::{ffi::OsString, sync::Arc};

use smithay::{
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{Seat, SeatState},
    reexports::{
        calloop::{generic::Generic, EventLoop, Interest, LoopSignal, Mode, PostAction},
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
            Display, DisplayHandle,
        },
    },
    utils::{Logical, Point},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::XdgShellState,
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

use crate::CalloopData;

pub struct Dome {
    pub appearance: dome::appearance::Appearance,
    pub appearance_open: bool,
    pub session: crate::session::Session,
    pub session_confirm: Option<dome::appearance::SessionAction>,
    pub xr_child: Option<std::process::Child>,
    pub control_port: u16,
    pub xr_bridge: dome::xr_transport::Shared,
    pub xr_buttons: std::collections::HashSet<u32>,
    pub xr_target: Option<u64>,
    pub xr_shell_pos: Option<[f64; 2]>,
    pub scene: std::sync::Arc<std::sync::Mutex<dome::Scene>>,
    pub registry: Vec<(u64, Window)>,
    pub applied_revision: u64,
    pub launcher: bool,
    pub menu: Option<usize>,
    pub about: bool,
    pub menu_hover: i32,
    pub chrome_drag: Option<crate::decorations::Drag>,
    pub shell_keys: std::collections::HashSet<smithay::backend::input::Keycode>,
    pub desktop_manager: bool,
    pub launch_query: String,
    pub apps: Vec<dome::service::App>,
    pub hover_app: Option<String>,
    pub task_popup: Option<usize>,
    pub task_context: bool,
    pub tray: crate::tray::Tray,
    pub pins: Vec<String>,
    pub shell_error: String,
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub xr_socket_name: OsString,
    pub input_realm: dome::Realm,
    pub input_xr_active: bool,
    pub focus_history: [Option<Option<u64>>; 2],
    pub desktop_pointer: Option<[f64; 2]>,
    pub desktop_buttons: std::collections::HashSet<u32>,
    pub physical_buttons: std::collections::HashSet<u32>,
    pub ignored_buttons: std::collections::HashSet<u32>,
    pub xr_was_visible: bool,
    pub xr_presentation: dome::xr_presentation::Presentation,
    pub xr_animator: dome::xr_presentation::Animator,
    pub xr_animation: dome::xr_presentation::Frame,
    pub mouse_angles: Option<[f32; 2]>,
    pub mouse_last_absolute: Option<[f64; 2]>,
    pub display_handle: DisplayHandle,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,

    // Smithay State
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub decoration_state: smithay::wayland::shell::xdg::decoration::XdgDecorationState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Dome>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,

    pub seat: Seat<Self>,
}

impl Dome {
    pub fn new(event_loop: &mut EventLoop<CalloopData>, display: Display<Self>) -> Self {
        let start_time = std::time::Instant::now();

        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let popups = PopupManager::default();

        // A seat is a group of keyboards, pointer and touch devices.
        // A seat typically has a pointer and maintains a keyboard focus and a pointer focus.
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "winit");

        // Notify clients that we have a keyboard, for the sake of the example we assume that keyboard is always present.
        // You may want to track keyboard hot-plug in real compositor.
        seat.add_keyboard(Default::default(), 200, 25).unwrap();

        // Notify clients that we have a pointer (mouse)
        // Here we assume that there is always pointer plugged in
        seat.add_pointer();

        // A space represents a two-dimensional plane. Windows and Outputs can be mapped onto it.
        //
        // Windows get a position and stacking order through mapping.
        // Outputs become views of a part of the Space and can be rendered via Space::render_output.
        let space = Space::default();

        let (socket_name, xr_socket_name) = Self::init_wayland_listener(display, event_loop);

        // Get the loop signal, used to stop the event loop
        let loop_signal = event_loop.get_signal();

        Self {
            appearance: dome::appearance::Appearance::load(),
            appearance_open: false,
            session: crate::session::Session::new(),
            session_confirm: None,
            xr_child: None,
            control_port: 49640,
            xr_bridge: Default::default(),
            xr_buttons: Default::default(),
            xr_target: None,
            xr_shell_pos: None,
            scene: std::sync::Arc::new(std::sync::Mutex::new(dome::Scene::new("wayland"))),
            registry: vec![],
            applied_revision: u64::MAX,
            launcher: false,
            menu: None,
            about: false,
            menu_hover: -1,
            chrome_drag: None,
            shell_keys: Default::default(),
            desktop_manager: false,
            launch_query: String::new(),
            apps: dome::service::apps(),
            hover_app: None,
            task_popup: None,
            task_context: false,
            tray: crate::tray::Tray::new(),
            pins: crate::tasks::load_pins(),
            shell_error: String::new(),
            start_time,
            display_handle: dh.clone(),

            space,
            loop_signal,
            socket_name,
            xr_socket_name,
            input_realm: dome::Realm::Desktop,
            input_xr_active: false,
            focus_history: [None, None],
            desktop_pointer: None,
            desktop_buttons: Default::default(),
            physical_buttons: Default::default(),
            ignored_buttons: Default::default(),
            xr_was_visible: false,
            xr_presentation: Default::default(),
            xr_animator: Default::default(),
            xr_animation: Default::default(),
            mouse_angles: None,
            mouse_last_absolute: None,

            compositor_state,
            xdg_shell_state,
            decoration_state: smithay::wayland::shell::xdg::decoration::XdgDecorationState::new::<
                Self,
            >(&dh),
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups,
            seat,
        }
    }

    fn init_wayland_listener(
        display: Display<Dome>,
        event_loop: &mut EventLoop<CalloopData>,
    ) -> (OsString, OsString) {
        let loop_handle = event_loop.handle();
        let mut names = Vec::new();
        for origin in [dome::Realm::Desktop, dome::Realm::Xr] {
            let listener = ListeningSocketSource::new_auto().unwrap();
            names.push(listener.socket_name().to_os_string());
            loop_handle
                .insert_source(listener, move |stream, _, state| {
                    state
                        .display_handle
                        .insert_client(
                            stream,
                            Arc::new(ClientState {
                                origin,
                                ..ClientState::default()
                            }),
                        )
                        .expect("insert Wayland client");
                })
                .expect("Wayland listener");
        }
        // You also need to add the display itself to the event loop, so that client events will be processed by wayland-server.
        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // Safety: we don't drop the display
                    unsafe {
                        display
                            .get_mut()
                            .dispatch_clients(&mut state.state)
                            .unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .unwrap();

        (names.remove(0), names.remove(0))
    }

    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space
            .element_under(pos)
            .and_then(|(window, location)| {
                window
                    .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + location).to_f64()))
            })
    }
}

#[derive(Default)]
pub struct ClientState {
    pub origin: dome::Realm,
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
