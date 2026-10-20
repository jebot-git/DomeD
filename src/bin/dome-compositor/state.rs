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
    pub dmabuf_state: smithay::wayland::dmabuf::DmabufState,
    pub dmabuf_importer:
        Option<Box<dyn FnMut(&smithay::backend::allocator::dmabuf::Dmabuf) -> bool>>,
    pub native_session_active: bool,
    pub native_ignored_keys: std::collections::HashSet<smithay::backend::input::Keycode>,
    pub cursor_status: smithay::input::pointer::CursorImageStatus,
    pub appearance: dome::appearance::Appearance,
    pub toolkit_appearance: Option<crate::toolkit_appearance::ToolkitAppearance>,
    pub appearance_open: bool,
    pub default_apps: crate::default_apps::Defaults,
    pub default_apps_open: bool,
    pub default_apps_row: usize,
    pub default_apps_choice: Option<usize>,
    pub ime: crate::ime::Ime,
    pub ime_popups: Vec<smithay::wayland::input_method::PopupSurface>,
    #[cfg(feature = "standalone")]
    pub keyboard_devices: Vec<smithay::reexports::input::Device>,
    pub keyboard_config: dome::keyboard::Keyboard,
    pub keyboard_draft: Option<dome::keyboard::Keyboard>,
    pub keyboard_field: usize,
    pub keyboard_error: String,
    pub keyboard_switch: i32,
    pub keyboard_apply: bool,
    pub xr_settings_open: bool,
    pub xr_settings_row: usize,
    pub display_modes: std::collections::BTreeMap<String, Vec<smithay::output::Mode>>,
    pub geometry_saved: dome::Config,
    pub geometry_error: String,
    pub session: crate::session::Session,
    pub session_persistence: Option<crate::session_restore::Persistence>,
    pub restore_stacking: bool,
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
    pub launch_index: usize,
    pub launch_navigation: crate::launcher::Navigation,
    pub binding_capture: Option<usize>,
    pub appmenus: crate::appmenu::Menus,
    pub appmenu_target: (Option<u64>, Option<crate::appmenu::Address>),
    pub appmenu_path: Vec<i32>,
    pub menu: Option<usize>,
    pub about: bool,
    pub menu_hover: i32,
    pub snap_preview: Option<(u64, dome::Rect)>,
    pub chrome_button: Option<(u64, i32, bool)>,
    pub workbench_zoom: std::collections::HashMap<u64, crate::decorations::ZoomState>,
    pub pending_raise: Option<u64>,
    pub no_raise_focus: Option<u64>,
    pub last_applied_focus: Option<u64>,
    pub chrome_drag: Option<crate::decorations::Drag>,
    pub shell_keys: std::collections::HashSet<smithay::backend::input::Keycode>,
    pub desktop_manager: bool,
    pub launch_query: String,
    pub apps: Vec<dome::service::App>,
    pub hover_app: Option<String>,
    pub task_popup: Option<usize>,
    pub task_previews: crate::task_previews::TaskPreviews,
    pub task_context: bool,
    pub task_context_at: Option<[f32; 2]>,
    pub task_context_offset: f32,
    pub task_context_selected: usize,
    pub tray: crate::tray::Tray,
    pub audio: dome::audio::Audio,
    pub audio_open: bool,
    pub audio_selected: usize,
    pub audio_drag: Option<dome::audio::Target>,
    pub audio_drag_value: Option<u8>,
    pub clipboard: crate::clipboard::Clipboard,
    pub history_panel: Option<crate::histories::Kind>,
    pub history_selected: usize,
    pub notifications: crate::notifications::Notifications,
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
    pub _viewporter_state: smithay::wayland::viewporter::ViewporterState,
    pub xdg_shell_state: XdgShellState,
    pub decoration_state: smithay::wayland::shell::xdg::decoration::XdgDecorationState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Dome>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state:
        smithay::wayland::selection::primary_selection::PrimarySelectionState,
    pub data_control_state: smithay::wayland::selection::wlr_data_control::DataControlState,
    pub ext_data_control_state: smithay::wayland::selection::ext_data_control::DataControlState,
    pub popups: PopupManager,
    pub popup_grab: Option<smithay::desktop::PopupGrab<Self>>,
    pub spatial_popups: Vec<(u64, smithay::desktop::PopupKind)>,
    pub next_popup_id: u64,
    pub context_trigger: Option<crate::context_menus::Trigger>,
    pub context_menus: Vec<crate::context_menus::Context>,
    pub context_scroll_hover: Option<(u64, i32)>,
    pub context_scroll_buttons: std::collections::HashSet<u32>,

    pub seat: Seat<Self>,
}

impl Dome {
    pub fn new(event_loop: &mut EventLoop<CalloopData>, display: Display<Self>) -> Self {
        let start_time = std::time::Instant::now();

        let dh = display.handle();
        dh.create_global::<Self, wayland_protocols_plasma::appmenu::server::org_kde_kwin_appmenu_manager::OrgKdeKwinAppmenuManager, _>(1, ());
        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let primary_selection_state =
            smithay::wayland::selection::primary_selection::PrimarySelectionState::new::<Self>(&dh);
        let data_control_state =
            smithay::wayland::selection::wlr_data_control::DataControlState::new::<Self, _>(
                &dh,
                Some(&primary_selection_state),
                |_| true,
            );
        let ext_data_control_state =
            smithay::wayland::selection::ext_data_control::DataControlState::new::<Self, _>(
                &dh,
                Some(&primary_selection_state),
                |_| true,
            );
        let popups = PopupManager::default();
        smithay::wayland::text_input::TextInputManagerState::new::<Self>(&dh);
        smithay::wayland::input_method::InputMethodManagerState::new::<Self, _>(&dh, |c| {
            c.get_data::<ClientState>().is_some_and(|d| d.trusted_ime)
        });
        smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState::new::<Self, _>(&dh, |c| {
            c.get_data::<ClientState>().is_some_and(|d| d.trusted_ime)
        });

        // A seat is a group of keyboards, pointer and touch devices.
        // A seat typically has a pointer and maintains a keyboard focus and a pointer focus.
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");

        // FLTK 1.4's registry callback stores gtk_shell1 on its seat without
        // checking that wl_seat has arrived. Advertise the seat first so these
        // native Wayland clients can bind our GTK menu extension safely.
        crate::gtk_menu::register(&dh);

        // Notify clients that we have a keyboard, for the sake of the example we assume that keyboard is always present.
        // You may want to track keyboard hot-plug in real compositor.
        let mut keyboard_config = dome::keyboard::Keyboard::load();
        if let Err(e) = seat.add_keyboard(
            smithay::input::keyboard::XkbConfig {
                layout: &keyboard_config.layout,
                variant: &keyboard_config.variant,
                options: Some(keyboard_config.options.clone()),
                ..Default::default()
            },
            200,
            25,
        ) {
            eprintln!("Keyboard configuration could not be compiled, using defaults: {e}");
            keyboard_config = Default::default();
            seat.add_keyboard(
                smithay::input::keyboard::XkbConfig {
                    layout: &keyboard_config.layout,
                    ..Default::default()
                },
                200,
                25,
            )
            .unwrap();
        }
        let keyboard = seat.get_keyboard().unwrap();
        let numlock = dome::keyboard::startup_numlock().or_else(crate::keyboard::existing_numlock);
        if let Some(num_lock) = numlock {
            let mut modifiers = keyboard.modifier_state();
            modifiers.num_lock = num_lock;
            keyboard.set_modifier_state(modifiers);
        }
        // Restore the last group after construction through update_keyboard.
        let keyboard_switch = keyboard_config.active as i32;

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
            dmabuf_state: smithay::wayland::dmabuf::DmabufState::new(),
            dmabuf_importer: None,
            native_session_active: true,
            native_ignored_keys: Default::default(),
            cursor_status: smithay::input::pointer::CursorImageStatus::default_named(),
            appearance: dome::appearance::Appearance::load(),
            toolkit_appearance: None,
            appearance_open: false,
            default_apps: crate::default_apps::Defaults::new(),
            default_apps_open: false,
            default_apps_row: 0,
            default_apps_choice: None,
            ime: crate::ime::Ime::new(),
            ime_popups: vec![],
            #[cfg(feature = "standalone")]
            keyboard_devices: Vec::new(),
            keyboard_config,
            keyboard_draft: None,
            keyboard_field: 0,
            keyboard_error: String::new(),
            keyboard_switch,
            keyboard_apply: false,
            xr_settings_open: false,
            xr_settings_row: 0,
            display_modes: Default::default(),
            geometry_saved: dome::Config::default(),
            geometry_error: String::new(),
            session: crate::session::Session::new(),
            session_persistence: None,
            restore_stacking: false,
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
            launch_index: 0,
            launch_navigation: Default::default(),
            binding_capture: None,
            appmenus: crate::appmenu::Menus::new(),
            appmenu_target: (None, None),
            appmenu_path: Vec::new(),
            menu: None,
            about: false,
            menu_hover: -1,
            snap_preview: None,
            chrome_button: None,
            workbench_zoom: Default::default(),
            pending_raise: None,
            no_raise_focus: None,
            last_applied_focus: None,
            chrome_drag: None,
            shell_keys: Default::default(),
            desktop_manager: false,
            launch_query: String::new(),
            apps: dome::service::apps(),
            hover_app: None,
            task_popup: None,
            task_previews: Default::default(),
            task_context: false,
            task_context_at: None,
            task_context_offset: 0.,
            task_context_selected: 0,
            tray: crate::tray::Tray::new(),
            audio: dome::audio::Audio::new(),
            audio_open: false,
            audio_selected: 0,
            audio_drag: None,
            audio_drag_value: None,
            clipboard: crate::clipboard::Clipboard::new(),
            history_panel: None,
            history_selected: 0,
            notifications: crate::notifications::Notifications::new(),
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
            _viewporter_state: smithay::wayland::viewporter::ViewporterState::new::<Self>(&dh),
            xdg_shell_state,
            decoration_state: smithay::wayland::shell::xdg::decoration::XdgDecorationState::new::<
                Self,
            >(&dh),
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            primary_selection_state,
            data_control_state,
            ext_data_control_state,
            popups,
            popup_grab: None,
            spatial_popups: vec![],
            next_popup_id: 0,
            context_trigger: None,
            context_menus: vec![],
            context_scroll_hover: None,
            context_scroll_buttons: Default::default(),
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
        if let Some((_, surface, origin)) = self.popup_under(pos) {
            return Some((surface, origin));
        }
        self.space
            .element_under(pos)
            .and_then(|(window, location)| {
                window
                    .surface_under(
                        pos - location.to_f64(),
                        WindowSurfaceType::TOPLEVEL | WindowSurfaceType::SUBSURFACE,
                    )
                    .map(|(s, p)| (s, (p + location).to_f64()))
            })
    }
}

#[derive(Default)]
pub struct ClientState {
    pub trusted_ime: bool,
    pub origin: dome::Realm,
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
