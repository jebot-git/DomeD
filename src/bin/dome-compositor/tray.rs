//! Session-bus StatusNotifier host. D-Bus calls run off the compositor thread.
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex, atomic::{AtomicBool, Ordering}},
    time::Duration,
};
use zbus::{
    blocking::{Connection, Proxy},
    message::Header,
    object_server::SignalEmitter,
    zvariant::{OwnedValue, Value},
};
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub key: String,
    pub id: String,
    pub title: String,
    pub icon: String,
    pub icon_theme_path: String,
    pub pixmap: String,
    pub width: i32,
    pub height: i32,
    pub attention: bool,
    #[serde(skip)]
    pub service: String,
    #[serde(skip)]
    pub path: String,
    #[serde(skip)]
    pub menu: String,
    #[serde(skip)]
    pub item_is_menu: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub id: i32,
    pub text: String,
    pub enabled: bool,
    pub separator: bool,
    pub checked: bool,
    pub checkable: bool,
    pub submenu: bool,
    pub icon: String,
    pub radio: bool,
}
fn placements() -> HashMap<String, bool> {
    let config = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("dome/tray-applications.json");
    std::fs::metadata(&config).ok().filter(|m| m.len() <= 65536)
        .and_then(|_| std::fs::read(config).ok())
        .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
fn managed_item(settings: &HashMap<String, bool>, id: &str) -> bool {
    settings.get(id).copied().unwrap_or(id == "dome.translator" || id == "wivrn.pro")
}
fn loading_rows() -> Vec<Row> {
    vec![Row {
        id: 0, text: "Loading input methods…".into(), enabled: false,
        separator: false, checked: false, checkable: false, submenu: false,
        icon: String::new(), radio: false,
    }]
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub items: Vec<Item>,
    pub menu: Option<usize>,
    pub rows: Vec<Row>,
    pub selected: Option<usize>,
    pub error: String,
    pub keyboard_menu: bool,
    pub screenshot_managed: bool,
}
impl Snapshot {
    fn active_item(&self) -> Option<Item> {
        self.menu?;
        if self.keyboard_menu { Some(keyboard_item()) }
        else { self.items.get(self.menu?).cloned() }
    }
}
fn keyboard_item() -> Item {
    Item {
        key: "dome.keyboard".into(), id: "dome.keyboard".into(),
        title: "Input methods".into(), icon: String::new(),
        icon_theme_path: String::new(), pixmap: String::new(), width: 0, height: 0,
        attention: false, service: "org.domed.Tray".into(),
        path: "/InputMethods".into(), menu: "/InputMethods/Menu".into(), item_is_menu: true,
    }
}
enum Request {
    Keyboard,
    Activate(usize, i32, i32, bool),
    Select(usize),
    Close,
    Back,
}
pub struct Tray {
    snapshot: Arc<Mutex<Snapshot>>,
    send: mpsc::SyncSender<Request>,
    screenshot: Arc<AtomicBool>,
}
// Queue capture on the compositor thread; D-Bus never touches renderer state.
struct SuiteControl {
    screenshot: Arc<AtomicBool>,
}
#[zbus::interface(name = "org.domed.Compositor")]
impl SuiteControl {
    fn screenshot(&self) { self.screenshot.store(true, Ordering::Release); }
}
struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
}
#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let key = if service.starts_with('/') {
            format!(
                "{}{}",
                header
                    .sender()
                    .ok_or_else(|| zbus::fdo::Error::InvalidArgs("No sender".into()))?,
                service
            )
        } else {
            format!("{service}/StatusNotifierItem")
        };
        let added = {
            let mut items = self.items.lock().unwrap();
            if items.contains(&key) {
                false
            } else {
                items.push(key.clone());
                true
            }
        };
        if added {
            Self::status_notifier_item_registered(&emitter, &key)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        }
        Ok(())
    }
    async fn register_status_notifier_host(&self, _service: &str) {}
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        item: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        item: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
impl Tray {
    pub fn new() -> Self {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let (send, receive) = mpsc::sync_channel(32);
        let state = snapshot.clone();
        let screenshot = Arc::new(AtomicBool::new(false));
        let capture = screenshot.clone();
        std::thread::spawn(move || {
            if let Err(e) = run(state.clone(), receive, capture) {
                state.lock().unwrap().error = e.to_string();
            }
        });
        Self { snapshot, send, screenshot }
    }
    pub fn take_screenshot_request(&self) -> bool {
        self.screenshot.swap(false, Ordering::AcqRel)
    }
    pub fn state(&self) -> Snapshot {
        self.snapshot.lock().unwrap().clone()
    }
    pub fn activate(&self, index: usize, x: i32, y: i32, context: bool) {
        let _ = self.send.try_send(Request::Activate(index, x, y, context));
    }
    pub fn select(&self, index: usize) {
        let _ = self.send.try_send(Request::Select(index));
    }
    pub fn keyboard(&self) {
        let mut state = self.snapshot.lock().unwrap();
        if state.keyboard_menu && state.menu.is_some() {
            drop(state);
            self.close();
            return;
        }
        state.menu = Some(0);
        state.keyboard_menu = true;
        state.selected = None;
        state.rows = loading_rows();
        state.revision += 1;
        drop(state);
        let _ = self.send.try_send(Request::Keyboard);
    }
    pub fn close(&self) {
        let _ = self.send.try_send(Request::Close);
    }
    pub fn key(&self, key: u32) -> bool {
        let mut state = self.snapshot.lock().unwrap();
        if state.menu.is_none() { return false; }
        let enabled: Vec<_> = state.rows.iter().enumerate()
            .filter(|(_, row)| row.enabled && !row.separator).map(|(i, _)| i).collect();
        let current = enabled.iter().position(|i| Some(*i) == state.selected).unwrap_or(0);
        if let Some(next) = crate::menu_navigation::selection(current, enabled.len(), key) {
            state.selected = enabled.get(next).copied();
            state.revision += 1;
        } else {
            let request = match key {
                0xff1b => Some(Request::Close),
                0xff51 => Some(Request::Back),
                0xff0d | 0xff8d | 0x20 | 0xff53 => state.selected.and_then(|i| {
                    (key != 0xff53 || state.rows[i].submenu).then_some(Request::Select(i))
                }),
                _ => None,
            };
            if let Some(request) = request { let _ = self.send.try_send(request); }
        }
        true
    }
}
type Node = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);
fn menu_rows(c: &Connection, item: &Item, parent: i32) -> zbus::Result<Vec<Row>> {
    let p = Proxy::new(
        c,
        item.service.as_str(),
        item.menu.as_str(),
        "com.canonical.dbusmenu",
    )?;
    let _ = p.call_method("AboutToShow", &(parent,));
    let (_, root): (u32, Node) = p
        .call_method("GetLayout", &(parent, 1i32, Vec::<String>::new()))?
        .body()
        .deserialize()?;
    Ok(root
        .2
        .into_iter()
        .filter_map(|value| {
            let (id, props, _) = Node::try_from(value).ok()?;
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
            Some(Row {
                id,
                text: text("label").replace('_', ""),
                enabled: boolean("enabled", true),
                separator: text("type") == "separator",
                checkable: !text("toggle-type").is_empty(),
                checked: props
                    .get("toggle-state")
                    .and_then(|v| i32::try_from(v).ok())
                    == Some(1),
                submenu: text("children-display") == "submenu",
                icon: text("icon-name").to_owned(),
                radio: text("toggle-type") == "radio",
            })
        })
        .collect())
}
fn native_frontend(item: &Item) -> Option<&'static str> {
    let identity = format!("{} {} {}", item.id, item.title, item.service).to_lowercase();
    if identity.contains("kdeconnect") || identity.contains("kde connect") { Some("devices") }
    else if identity.contains("fcitx") { Some("input") }
    else if identity.contains("kwallet") || identity.contains("wallet manager") { Some("wallets") }
    else { None }
}
fn open_native(connection: &Connection, page: &str) -> bool {
    if let Ok(proxy) = Proxy::new(connection, "org.domed.Tray", "/org/domed/Tray", "org.domed.Tray") {
        if proxy.call_method("Show", &(page,)).is_ok() { return true; }
    }
    dome::desktop_entries::find_executable("dome-tray")
        .is_some_and(|program| std::process::Command::new(program)
            .arg(if page.is_empty() { "--daemon".into() } else { format!("--{page}") }).spawn().is_ok())
}
fn run(
    state: Arc<Mutex<Snapshot>>,
    receive: mpsc::Receiver<Request>,
    screenshot: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::session()?;
    // A second diagnostic compositor on this bus must not take session control.
    if connection.request_name("org.domed.Compositor").is_ok() {
        connection.object_server().at("/org/domed/Compositor", SuiteControl { screenshot })?;
    }
    let dbus = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let owner: bool = dbus
        .call_method("NameHasOwner", &("org.kde.StatusNotifierWatcher",))?
        .body()
        .deserialize()?;
    let own_items = Arc::new(Mutex::new(Vec::<String>::new()));
    if !owner {
        connection.object_server().at(
            "/StatusNotifierWatcher",
            Watcher {
                items: own_items.clone(),
            },
        )?;
        connection.request_name("org.kde.StatusNotifierWatcher")?;
    }
    let watcher = Proxy::new(
        &connection,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    )?;
    let _ = watcher.call_method(
        "RegisterStatusNotifierHost",
        &(connection.unique_name().unwrap().as_str(),),
    );
    let mut parents = vec![0];
    loop {
        while let Ok(request) = receive.try_recv() {
            let snapshot = state.lock().unwrap().clone();
            match request {
                Request::Keyboard => {
                    let mut rows = Err(zbus::Error::Failure("Native Fcitx5 frontend is unavailable".into()));
                    if open_native(&connection, "") {
                        for _ in 0..20 {
                            rows = menu_rows(&connection, &keyboard_item(), 0);
                            if rows.is_ok() { break; }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                    }
                    let mut s = state.lock().unwrap();
                    match rows {
                        Ok(rows) => {
                            s.menu = Some(0); s.keyboard_menu = true;
                            s.selected = rows.iter().position(|r| r.enabled && !r.separator);
                            s.rows = rows; parents = vec![0]; s.error.clear();
                        }
                        Err(error) => {
                            s.error = error.to_string();
                            s.menu = None; s.rows.clear();
                        }
                    }
                    s.revision += 1;
                }
                Request::Close => {
                    let mut s = state.lock().unwrap();
                    s.menu = None;
                    s.rows.clear();
                    s.selected = None;
                    parents = vec![0];
                    s.revision += 1;
                }
                Request::Back => {
                    if parents.len() > 1 {
                        parents.pop();
                        if let Some(item) = snapshot.active_item() {
                            if let Ok(rows) = menu_rows(&connection, &item, *parents.last().unwrap()) {
                                let mut s = state.lock().unwrap();
                                s.selected = rows.iter().position(|r| r.enabled && !r.separator);
                                s.rows = rows;
                                s.revision += 1;
                            }
                        }
                    } else {
                        let mut s = state.lock().unwrap();
                        s.menu = None;
                        s.rows.clear();
                        s.selected = None;
                        s.revision += 1;
                    }
                }
                Request::Activate(index, x, y, context) => {
                    if let Some(item) = snapshot.items.get(index) {
                        if !context {
                            if let Some(page) = native_frontend(item) {
                                if !open_native(&connection, page) {
                                    state.lock().unwrap().error = "Install dome-tray for native service controls".into();
                                }
                                continue;
                            }
                        }
                        if item.menu != "/"
                            && !item.menu.is_empty()
                            && (context || item.item_is_menu)
                        {
                            match menu_rows(&connection, item, 0) {
                                Ok(rows) => {
                                    let mut s = state.lock().unwrap();
                                    s.menu = Some(index);
                                    s.keyboard_menu = false;
                                    s.selected = rows.iter().position(|r| r.enabled && !r.separator);
                                    parents = vec![0];
                                    s.rows = rows;
                                    s.revision += 1;
                                }
                                Err(_) => {
                                    let proxy = Proxy::new(
                                        &connection,
                                        item.service.as_str(),
                                        item.path.as_str(),
                                        "org.kde.StatusNotifierItem",
                                    )?;
                                    let _ = proxy.call_method("ContextMenu", &(x, y));
                                }
                            }
                        } else {
                            let proxy = Proxy::new(
                                &connection,
                                item.service.as_str(),
                                item.path.as_str(),
                                "org.kde.StatusNotifierItem",
                            )?;
                            let _ = proxy.call_method(
                                if context { "ContextMenu" } else { "Activate" },
                                &(x, y),
                            );
                        }
                    }
                }
                Request::Select(index) => {
                    if let Some(item) = snapshot.active_item() {
                        if let Some(row) = snapshot
                            .rows
                            .get(index)
                            .filter(|r| r.enabled && !r.separator)
                        {
                            let settings = row.text.to_lowercase();
                            if !row.submenu && (settings.contains("configur") || settings.contains("settings") || settings.contains("wallet manager"))
                                && native_frontend(&item).is_some_and(|page| open_native(&connection, page)) {
                                let mut s = state.lock().unwrap();
                                s.menu = None; s.rows.clear(); s.revision += 1;
                                continue;
                            }
                            if row.submenu {
                                if snapshot.keyboard_menu {
                                    let mut s = state.lock().unwrap();
                                    s.rows = loading_rows(); s.selected = None;
                                    s.revision += 1;
                                }
                                let result = menu_rows(&connection, &item, row.id);
                                let mut s = state.lock().unwrap();
                                match result {
                                    Ok(rows) => {
                                        parents.push(row.id);
                                        s.selected = rows.iter().position(|r| r.enabled && !r.separator);
                                        s.rows = rows;
                                    }
                                    Err(e) => {
                                        s.error = e.to_string();
                                        s.menu = None;
                                        s.rows.clear();
                                    }
                                }
                                s.revision += 1;
                            } else {
                                let p = Proxy::new(
                                    &connection,
                                    item.service.as_str(),
                                    item.menu.as_str(),
                                    "com.canonical.dbusmenu",
                                )?;
                                let _ = p.call_method(
                                    "Event",
                                    &(row.id, "clicked", Value::from(0i32), 0u32),
                                );
                                let mut s = state.lock().unwrap();
                                s.menu = None;
                                s.rows.clear();
                                s.revision += 1;
                            }
                        }
                    }
                }
            }
        }
        let keys: Vec<String> = Proxy::new(
            &connection,
            "org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            "org.kde.StatusNotifierWatcher",
        )?
        .get_property("RegisteredStatusNotifierItems")
        .unwrap_or_default();
        let mut items = vec![];
        for key in keys {
            let Some((service, path)) = key.split_once('/') else {
                continue;
            };
            let path = format!("/{path}");
            let Ok(proxy) = Proxy::new(
                &connection,
                service,
                path.as_str(),
                "org.kde.StatusNotifierItem",
            ) else {
                continue;
            };
            let Ok(status) = proxy.get_property::<String>("Status") else {
                continue;
            };
            if status == "Passive" {
                continue;
            }
            let title = proxy.get_property("Title").unwrap_or_default();
            let attention = status == "NeedsAttention";
            let mut icon: String = proxy
                .get_property(if attention {
                    "AttentionIconName"
                } else {
                    "IconName"
                })
                .unwrap_or_default();
            if attention && icon.is_empty() {
                icon = proxy.get_property("IconName").unwrap_or_default();
            }
            let icon_theme_path = proxy.get_property("IconThemePath").unwrap_or_default();
            let mut images: Vec<(i32, i32, Vec<u8>)> = proxy
                .get_property(if attention {
                    "AttentionIconPixmap"
                } else {
                    "IconPixmap"
                })
                .unwrap_or_default();
            if attention && images.is_empty() {
                images = proxy.get_property("IconPixmap").unwrap_or_default();
            }
            let (width, height, pixmap) = images
                .into_iter()
                .filter(|(w, h, p)| {
                    *w > 0
                        && *h > 0
                        && *w <= 512
                        && *h <= 512
                        && p.len() == (*w as usize) * (*h as usize) * 4
                })
                .min_by_key(|(w, _, _)| (*w - 22).abs())
                .map(|(w, h, p)| {
                    use base64::Engine;
                    (w, h, base64::engine::general_purpose::STANDARD.encode(p))
                })
                .unwrap_or_default();
            let menu = proxy
                .get_property::<zbus::zvariant::OwnedObjectPath>("Menu")
                .map(|p| p.to_string())
                .unwrap_or_default();
            let item_is_menu = proxy.get_property("ItemIsMenu").unwrap_or(false);
            items.push(Item {
                key: key.clone(),
                id: proxy.get_property("Id").unwrap_or_default(),
                title,
                icon,
                icon_theme_path,
                pixmap,
                width,
                height,
                attention,
                service: service.into(),
                path: path.clone(),
                menu,
                item_is_menu,
            });
        }
        // The keyboard selector is the single language indicator, regardless
        // of when Fcitx registers or which service frontend is preferred.
        let managed = dbus.call::<_, _, bool>("NameHasOwner", &("org.domed.Tray",)).unwrap_or(false);
        let settings = placements();
        let screenshot_managed = managed && managed_item(&settings, "dome.screenshot");
        items.retain(|item| {
            let identity = format!("{} {} {}", item.id, item.title, item.service).to_lowercase();
            !identity.contains("fcitx") && (item.id == "dome.tray" || !managed || !managed_item(&settings, &item.id))
        });
        if !owner {
            // An item may register before exporting its properties, and live
            // applications can temporarily fail a property read. Only forget
            // registrations whose bus name has actually disappeared.
            let registered = own_items.lock().unwrap().clone();
            let disconnected: Vec<_> = registered
                .into_iter()
                .filter(|key| {
                    let Some((service, _)) = key.split_once('/') else {
                        return false;
                    };
                    matches!(
                        dbus.call_method("NameHasOwner", &(service,))
                            .and_then(|reply| reply.body().deserialize::<bool>()),
                        Ok(false)
                    )
                })
                .collect();
            // Remove only confirmed departures; preserve registrations added
            // while this polling pass was reading properties.
            own_items
                .lock()
                .unwrap()
                .retain(|key| !disconnected.contains(key));
        }
        let mut s = state.lock().unwrap();
        let before = serde_json::to_string(&s.items)?;
        if serde_json::to_string(&items)? != before || s.screenshot_managed != screenshot_managed {
            s.items = items;
            s.screenshot_managed = screenshot_managed;
            if !s.keyboard_menu {
                s.menu = None;
                s.rows.clear();
            }
            s.revision += 1;
        }
        drop(s);
        // Bounded polling keeps property updates and process exits out of the render thread.
        std::thread::sleep(Duration::from_millis(150));
    }
}

impl crate::Dome {
    pub fn visible_tray(&self, width: i32) -> Snapshot {
        let mut state = self.tray.state();
        if let Some(index) = state.menu.as_mut().filter(|_| !state.keyboard_menu) {
            *index += if state.screenshot_managed { 3 } else { 4 };
        }
        let own = |key: &str, title: &str, attention: bool| Item {
            key: key.into(),
            id: key.into(),
            title: title.into(),
            icon: String::new(),
            icon_theme_path: String::new(),
            pixmap: String::new(),
            width: 0,
            height: 0,
            attention,
            service: String::new(),
            path: String::new(),
            menu: String::new(),
            item_is_menu: false,
        };
        let audio = self.audio.state();
        let mut speaker = own("dome.audio", "Audio", false);
        speaker.icon = if audio
            .output
            .is_some_and(|level| level.muted || level.volume <= 0.)
        {
            "muted"
        } else if audio.output.is_some() {
            "volume"
        } else {
            "unavailable"
        }
        .into();
        let mut builtins = vec![speaker, own("dome.clipboard", "Clipboard history", false),
            own("dome.notifications", "Notification history", self.notifications.unread())];
        if !state.screenshot_managed {
            builtins.push(own("dome.screenshot", "Screenshot (PrintScreen)", false));
        }
        state.items.splice(0..0, builtins);
        state.items.truncate(((width - 484 - self.clock_reserve()).max(0) / 28) as usize);
        if !state.keyboard_menu && state.menu.is_some_and(|i| i >= state.items.len()) {
            state.menu = None;
            state.rows.clear();
            self.tray.close();
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wivrn_defaults_to_manager_and_respects_placement_override() {
        let mut settings = HashMap::new();
        assert!(managed_item(&settings, "wivrn.pro"));
        assert!(!managed_item(&settings, "unrelated.application"));
        settings.insert("wivrn.pro".into(), false);
        assert!(!managed_item(&settings, "wivrn.pro"));
        settings.insert("wivrn.pro".into(), true);
        assert!(managed_item(&settings, "wivrn.pro"));
    }
}
