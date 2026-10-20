//! Session-bus StatusNotifier host. D-Bus calls run off the compositor thread.
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
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
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub items: Vec<Item>,
    pub menu: Option<usize>,
    pub rows: Vec<Row>,
    pub error: String,
}
enum Request {
    Activate(usize, i32, i32, bool),
    Select(usize),
    Close,
}
pub struct Tray {
    snapshot: Arc<Mutex<Snapshot>>,
    send: mpsc::SyncSender<Request>,
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
        std::thread::spawn(move || {
            if let Err(e) = run(state.clone(), receive) {
                state.lock().unwrap().error = e.to_string();
            }
        });
        Self { snapshot, send }
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
    pub fn close(&self) {
        let _ = self.send.try_send(Request::Close);
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
            })
        })
        .collect())
}
fn run(
    state: Arc<Mutex<Snapshot>>,
    receive: mpsc::Receiver<Request>,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::session()?;
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
    loop {
        while let Ok(request) = receive.try_recv() {
            let snapshot = state.lock().unwrap().clone();
            match request {
                Request::Close => {
                    let mut s = state.lock().unwrap();
                    s.menu = None;
                    s.rows.clear();
                    s.revision += 1;
                }
                Request::Activate(index, x, y, context) => {
                    if let Some(item) = snapshot.items.get(index) {
                        if item.menu != "/"
                            && !item.menu.is_empty()
                            && (context || item.item_is_menu)
                        {
                            match menu_rows(&connection, item, 0) {
                                Ok(rows) => {
                                    let mut s = state.lock().unwrap();
                                    s.menu = Some(index);
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
                    if let Some(item) = snapshot.menu.and_then(|i| snapshot.items.get(i)) {
                        if let Some(row) = snapshot
                            .rows
                            .get(index)
                            .filter(|r| r.enabled && !r.separator)
                        {
                            if row.submenu {
                                let result = menu_rows(&connection, item, row.id);
                                let mut s = state.lock().unwrap();
                                match result {
                                    Ok(rows) => s.rows = rows,
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
        let mut alive = vec![];
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
            alive.push(key.clone());
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
        if !owner {
            own_items.lock().unwrap().retain(|key| alive.contains(key));
        }
        let mut s = state.lock().unwrap();
        let before = serde_json::to_string(&s.items)?;
        if serde_json::to_string(&items)? != before {
            s.items = items;
            s.menu = None;
            s.rows.clear();
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
        if let Some(index) = state.menu.as_mut() {
            *index += 2;
        }
        let own = |key: &str, title: &str, attention: bool| Item {
            key: key.into(),
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
        state.items.splice(
            0..0,
            [
                own("dome.clipboard", "Clipboard history", false),
                own(
                    "dome.notifications",
                    "Notification history",
                    self.notifications.unread(),
                ),
            ],
        );
        state.items.truncate(((width - 704).max(0) / 28) as usize);
        if state.menu.is_some_and(|i| i >= state.items.len()) {
            state.menu = None;
            state.rows.clear();
            self.tray.close();
        }
        state
    }
}
