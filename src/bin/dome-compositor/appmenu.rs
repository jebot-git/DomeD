//! Per-surface application menus. All remote calls stay off the compositor thread.
use crate::Dome;
use serde::Serialize;
use smithay::reexports::wayland_server::{
    protocol::wl_surface::WlSurface, Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch,
    New, Resource,
};
use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use wayland_protocols_plasma::appmenu::server::{
    org_kde_kwin_appmenu::{self, OrgKdeKwinAppmenu},
    org_kde_kwin_appmenu_manager::{self, OrgKdeKwinAppmenuManager},
};
use zbus::{
    blocking::{Connection, Proxy},
    message::Header,
    object_server::SignalEmitter,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub service: String,
    pub path: String,
    pub gtk: Option<crate::gtk_menu::Export>,
}
#[derive(Default)]
pub struct SurfaceMenu(Mutex<Option<(u32, Address)>>);
impl GlobalDispatch<OrgKdeKwinAppmenuManager, ()> for Dome {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<OrgKdeKwinAppmenuManager>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}
impl Dispatch<OrgKdeKwinAppmenuManager, ()> for Dome {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &OrgKdeKwinAppmenuManager,
        request: org_kde_kwin_appmenu_manager::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let org_kde_kwin_appmenu_manager::Request::Create { id, surface } = request {
            smithay::wayland::compositor::with_states(&surface, |s| {
                s.data_map.insert_if_missing(SurfaceMenu::default);
            });
            let menu = init.init(id, surface.clone());
            // Replacement objects supersede older objects for the same surface.
            smithay::wayland::compositor::with_states(&surface, |s| {
                *s.data_map.get::<SurfaceMenu>().unwrap().0.lock().unwrap() = None;
            });
            let _ = menu;
        }
    }
}
impl Dispatch<OrgKdeKwinAppmenu, WlSurface> for Dome {
    fn request(
        _: &mut Self,
        _: &Client,
        resource: &OrgKdeKwinAppmenu,
        request: org_kde_kwin_appmenu::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let org_kde_kwin_appmenu::Request::SetAddress {
            service_name,
            object_path,
        } = request
        {
            let valid = zbus::names::BusName::try_from(service_name.as_str()).is_ok()
                && zbus::zvariant::ObjectPath::try_from(object_path.as_str()).is_ok()
                && object_path != "/";
            smithay::wayland::compositor::with_states(surface, |s| {
                *s.data_map.get::<SurfaceMenu>().unwrap().0.lock().unwrap() = valid.then(|| {
                    (
                        resource.id().protocol_id(),
                        Address {
                            service: service_name,
                            path: object_path,
                            gtk: None,
                        },
                    )
                });
            });
        }
    }
    fn destroyed(
        _: &mut Self,
        _: smithay::reexports::wayland_server::backend::ClientId,
        resource: &OrgKdeKwinAppmenu,
        surface: &WlSurface,
    ) {
        smithay::wayland::compositor::with_states(surface, |s| {
            let mut address = s.data_map.get::<SurfaceMenu>().unwrap().0.lock().unwrap();
            if address
                .as_ref()
                .is_some_and(|(id, _)| *id == resource.id().protocol_id())
            {
                *address = None;
            }
        });
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Node {
    /// Local actions never travel through the application D-Bus dispatcher.
    #[serde(skip)]
    pub command: Option<dome::shortcuts::Command>,
    pub id: i32,
    pub text: String,
    pub enabled: bool,
    pub separator: bool,
    pub checked: bool,
    pub checkable: bool,
    pub radio: bool,
    pub shortcut: String,
    pub submenu: bool,
    pub children: Vec<Node>,
}
impl Node {
    pub fn find(&self, id: i32) -> Option<&Self> {
        if self.id == id {
            Some(self)
        } else {
            self.children.iter().find_map(|n| n.find(id))
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub address: Option<Address>,
    pub root: Node,
    pub revision: u64,
}
enum Request {
    Focus(Option<Address>),
    Open(Address, i32),
    Activate(Address, i32),
    Close,
}
pub struct Menus {
    snapshot: Arc<Mutex<Snapshot>>,
    send: mpsc::Sender<Request>,
    visible: std::sync::atomic::AtomicBool,
}
impl Menus {
    pub fn new() -> Self {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let state = snapshot.clone();
        let (send, recv) = mpsc::channel();
        std::thread::spawn(move || {
            if let Err(e) = run(state, recv) {
                eprintln!("Application menus: {e}");
            }
        });
        Self {
            snapshot,
            send,
            visible: std::sync::atomic::AtomicBool::new(false),
        }
    }
    pub fn state(&self) -> Snapshot {
        self.snapshot.lock().unwrap().clone()
    }
    pub fn focus(&self, address: Option<Address>) {
        let _ = self.send.send(Request::Focus(address));
    }
    pub fn open(&self, address: Address, id: i32) {
        self.visible
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = self.send.send(Request::Open(address, id));
    }
    pub fn activate(&self, address: Address, id: i32) {
        let _ = self.send.send(Request::Activate(address, id));
    }
    pub fn close(&self) {
        if self
            .visible
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            let _ = self.send.send(Request::Close);
        }
    }
}
// Qt's platform theme checks for this name before exporting native menu bars.
#[derive(Default)]
struct Registrar {
    windows: Mutex<HashMap<u32, (String, OwnedObjectPath)>>,
}
#[zbus::interface(name = "com.canonical.AppMenu.Registrar")]
impl Registrar {
    async fn register_window(
        &self,
        window: u32,
        menu: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = header
            .sender()
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("No sender".into()))?
            .to_string();
        self.windows
            .lock()
            .unwrap()
            .insert(window, (sender.clone(), menu.clone()));
        Self::window_registered(&emitter, window, &sender, menu)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    async fn unregister_window(
        &self,
        window: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let removed = {
            let mut windows = self.windows.lock().unwrap();
            if windows
                .get(&window)
                .is_some_and(|(s, _)| Some(s.as_str()) == header.sender().map(|s| s.as_str()))
            {
                windows.remove(&window);
                true
            } else {
                false
            }
        };
        if removed {
            Self::window_unregistered(&emitter, window)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        }
        Ok(())
    }
    fn get_menu_for_window(&self, window: u32) -> (String, OwnedObjectPath) {
        self.windows
            .lock()
            .unwrap()
            .get(&window)
            .cloned()
            .unwrap_or_else(|| (String::new(), OwnedObjectPath::try_from("/").unwrap()))
    }
    fn get_menus(&self) -> Vec<(u32, String, OwnedObjectPath)> {
        self.windows
            .lock()
            .unwrap()
            .iter()
            .map(|(id, (s, p))| (*id, s.clone(), p.clone()))
            .collect()
    }
    #[zbus(signal)]
    async fn window_registered(
        emitter: &SignalEmitter<'_>,
        window: u32,
        service: &str,
        path: OwnedObjectPath,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn window_unregistered(emitter: &SignalEmitter<'_>, window: u32) -> zbus::Result<()>;
}
type WireNode = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);
fn decode((id, props, children): WireNode, depth: usize) -> Option<Node> {
    if depth > 16 {
        return None;
    }
    let text = |key| {
        props
            .get(key)
            .and_then(|v| <&str>::try_from(v).ok())
            .unwrap_or("")
    };
    let boolean = |key, default| {
        props
            .get(key)
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(default)
    };
    if !boolean("visible", true) {
        return None;
    }
    let shortcut = props
        .get("shortcut")
        .and_then(|v| Vec::<Vec<String>>::try_from(v.try_clone().ok()?).ok())
        .and_then(|v| v.into_iter().next())
        .unwrap_or_default()
        .join("+");
    Some(Node {
        command: None,
        id,
        text: text("label")
            .replace("__", "\0")
            .replace('_', "")
            .replace('\0', "_"),
        enabled: boolean("enabled", true),
        separator: text("type") == "separator",
        checked: props
            .get("toggle-state")
            .and_then(|v| i32::try_from(v).ok())
            == Some(1),
        checkable: !text("toggle-type").is_empty(),
        radio: text("toggle-type") == "radio",
        shortcut,
        submenu: text("children-display") == "submenu",
        children: children
            .into_iter()
            .take(4096)
            .filter_map(|v| WireNode::try_from(v).ok())
            .filter_map(|n| decode(n, depth + 1))
            .collect(),
    })
}
fn proxy<'a>(c: &'a Connection, a: &'a Address) -> zbus::Result<Proxy<'a>> {
    Proxy::new(
        c,
        a.service.as_str(),
        a.path.as_str(),
        "com.canonical.dbusmenu",
    )
}
fn run(
    state: Arc<Mutex<Snapshot>>,
    recv: mpsc::Receiver<Request>,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(Duration::from_millis(500))
        .build()?;
    let dbus = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let owned: bool = dbus.call("NameHasOwner", &("com.canonical.AppMenu.Registrar",))?;
    if !owned {
        connection
            .object_server()
            .at("/com/canonical/AppMenu/Registrar", Registrar::default())?;
        connection.request_name("com.canonical.AppMenu.Registrar")?;
    }
    let mut focused = None;
    let mut gtk = crate::gtk_menu::Reader::default();
    loop {
        match recv.recv_timeout(Duration::from_millis(300)) {
            Ok(Request::Focus(address)) => {
                gtk.close(&connection);
                focused = address;
                let mut s = state.lock().unwrap();
                s.address = focused.clone();
                s.root = Node::default();
                s.revision += 1;
            }
            Ok(Request::Close) => gtk.close(&connection),
            Ok(Request::Open(address, id)) if focused.as_ref() == Some(&address) => {
                if address.gtk.is_some() {
                    gtk.open(&connection, &address, id);
                    continue;
                }
                if let Ok(p) = proxy(&connection, &address) {
                    let _ = p.call_method("AboutToShow", &(id,));
                }
            }
            Ok(Request::Activate(address, id)) if focused.as_ref() == Some(&address) => {
                if address.gtk.is_some() {
                    gtk.activate(&connection, &address, id);
                    continue;
                }
                // Re-read before dispatch: an item may have been disabled since it was painted.
                if let Ok(p) = proxy(&connection, &address) {
                    if let Ok((_, node)) = p.call::<_, _, (u32, WireNode)>(
                        "GetLayout",
                        &(id, 0i32, Vec::<String>::new()),
                    ) {
                        if decode(node, 0).is_some_and(|n| n.enabled && !n.separator && !n.submenu)
                        {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u32;
                            let _ = p.call_method(
                                "Event",
                                &(id, "clicked", Value::from(0i32), timestamp),
                            );
                        }
                    }
                }
            }
            Ok(_) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
        if let Some(address) = &focused {
            let root = if address.gtk.is_some() {
                gtk.read(&connection, address).unwrap_or_default()
            } else {
                proxy(&connection, address)
                    .ok()
                    .and_then(|p| {
                        p.call::<_, _, (u32, WireNode)>(
                            "GetLayout",
                            &(0i32, 8i32, Vec::<String>::new()),
                        )
                        .ok()
                    })
                    .and_then(|(_, n)| decode(n, 0))
                    .unwrap_or_default()
            };
            let mut s = state.lock().unwrap();
            if s.root != root {
                if std::env::var_os("DOME_MENU_TRACE").is_some() {
                    eprintln!("menu model: {root:?}");
                }
                s.root = root;
                s.revision += 1;
            }
        }
    }
}
impl Dome {
    pub fn focused_menu_address(&self) -> Option<Address> {
        let scene = self.shell_scene();
        let mut window = scene.focused?;
        // Dialogs inherit their parent's exported menu.
        for _ in 0..16 {
            let surface = self
                .registry
                .iter()
                .find(|(id, _)| *id == window)?
                .1
                .toplevel()?
                .wl_surface();
            let (address, parent) = smithay::wayland::compositor::with_states(surface, |s| {
                let address = s
                    .data_map
                    .get::<SurfaceMenu>()
                    .and_then(|m| m.0.lock().unwrap().as_ref().map(|(_, a)| a.clone()))
                    .or_else(|| {
                        s.data_map
                            .get::<crate::gtk_menu::SurfaceExport>()
                            .and_then(|m| m.0.lock().unwrap().clone())
                    });
                let parent = s
                    .data_map
                    .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                    .and_then(|d| d.lock().unwrap().parent.clone());
                (address, parent)
            });
            if address.is_some() {
                return address;
            }
            if let Some(parent) = parent {
                window = self.id_for(&parent)?;
            } else {
                return crate::gtk_menu::discover_for(surface, &self.display_handle);
            }
        }
        None
    }
    pub fn update_appmenu(&mut self) {
        if self.menu.is_none() {
            self.appmenus.close();
        }
        let address = self.focused_menu_address();
        let focused = self.shell_scene().focused;
        if self.appmenu_target != (focused, address.clone()) {
            if std::env::var_os("DOME_MENU_TRACE").is_some() {
                eprintln!("menu target: focus={focused:?} address={address:?}");
            }
            self.appmenu_target = (focused, address.clone());
            self.menu = None;
            self.appmenu_path.clear();
            self.menu_hover = -1;
            self.appmenus.focus(address);
        }
    }
    pub fn appmenu_snapshot(&self) -> Snapshot {
        let snapshot = self.appmenus.state();
        if snapshot.address == self.focused_menu_address() {
            snapshot
        } else {
            Snapshot::default()
        }
    }
}
