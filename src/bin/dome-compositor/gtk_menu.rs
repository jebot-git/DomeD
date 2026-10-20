//! GTK's Wayland menu metadata and GMenu/GAction bridge for the universal menu.
use crate::{
    appmenu::{Address, Node},
    Dome,
};
use smithay::reexports::wayland_server::{
    protocol::wl_surface::WlSurface, Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New,
};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::Mutex,
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedValue, Signature},
};

#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    unused_imports,
    unused_variables,
    unused_unsafe
)]
pub mod protocol {
    use wayland_server;
    use wayland_server::protocol::*;
    pub mod __interfaces {
        use wayland_server::backend as wayland_backend;
        use wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/gtk-shell.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_server_code!("protocols/gtk-shell.xml");
}
use protocol::{
    gtk_shell1::{self, GtkShell1},
    gtk_surface1::{self, GtkSurface1},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub discover_pid: Option<u32>,
    pub app_menu: Option<String>,
    pub menubar: Option<String>,
    pub app_actions: Option<String>,
    pub window_actions: Option<String>,
}
#[derive(Default)]
pub struct SurfaceExport(pub Mutex<Option<Address>>);
pub fn register(display: &DisplayHandle) {
    display.create_global::<Dome, GtkShell1, _>(3, ());
}
impl GlobalDispatch<GtkShell1, ()> for Dome {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<GtkShell1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let shell = init.init(resource, ());
        // GTK translates these bits into gtk-shell-shows-app-menu/menubar.
        shell.capabilities(3);
    }
}
impl Dispatch<GtkShell1, ()> for Dome {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &GtkShell1,
        request: gtk_shell1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let gtk_shell1::Request::GetGtkSurface {
            gtk_surface,
            surface,
        } = request
        {
            smithay::wayland::compositor::with_states(&surface, |s| {
                s.data_map.insert_if_missing(SurfaceExport::default);
            });
            init.init(gtk_surface, surface);
        }
    }
}
impl Dispatch<GtkSurface1, WlSurface> for Dome {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &GtkSurface1,
        request: gtk_surface1::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if matches!(request, gtk_surface1::Request::Present { .. } | gtk_surface1::Request::RequestFocus { .. }) {
            if let Some(id) = state.id_for(surface) {
                // GTK requests activation; DomeD owns focus, workspace selection,
                // restoration and stacking through its ordinary window policy.
                state.action(dome::Action::Focus { id });
            }
            return;
        }
        if let gtk_surface1::Request::SetDbusProperties {
            app_menu_path,
            menubar_path,
            window_object_path,
            application_object_path,
            unique_bus_name,
            ..
        } = request
        {
            let path = |p: Option<String>| {
                p.filter(|s| s != "/" && zbus::zvariant::ObjectPath::try_from(s.as_str()).is_ok())
            };
            let export = Export {
                discover_pid: None,
                app_menu: path(app_menu_path),
                menubar: path(menubar_path),
                app_actions: path(application_object_path),
                window_actions: path(window_object_path),
            };
            let service = unique_bus_name.filter(|s| {
                s.starts_with(':') && zbus::names::UniqueName::try_from(s.as_str()).is_ok()
            });
            let address = service.and_then(|service| {
                let path = export
                    .menubar
                    .as_ref()
                    .or(export.app_menu.as_ref())?
                    .clone();
                Some(Address {
                    service,
                    path,
                    gtk: Some(export),
                })
            });
            smithay::wayland::compositor::with_states(surface, |s| {
                *s.data_map.get::<SurfaceExport>().unwrap().0.lock().unwrap() = address;
            });
        }
    }
    fn destroyed(
        _: &mut Self,
        _: smithay::reexports::wayland_server::backend::ClientId,
        _: &GtkSurface1,
        surface: &WlSurface,
    ) {
        smithay::wayland::compositor::with_states(surface, |s| {
            if let Some(m) = s.data_map.get::<SurfaceExport>() {
                *m.0.lock().unwrap() = None;
            }
        });
    }
}

type Items = Vec<HashMap<String, OwnedValue>>;
type Menus = HashMap<(u32, u32), Items>;
type ActionState = (bool, Signature, Vec<OwnedValue>);
struct Action {
    path: String,
    name: String,
    target: Option<OwnedValue>,
    submenu: bool,
}
#[derive(Default)]
pub struct Reader {
    actions: HashMap<i32, Action>,
    discovered: HashMap<u32, (std::time::Instant, Option<Address>)>,
    opened: HashSet<(String, String, String)>,
    state_service: String,
    action_states: HashMap<String, HashMap<String, ActionState>>,
}

pub fn discover_for(surface: &WlSurface, display: &DisplayHandle) -> Option<Address> {
    use smithay::reexports::wayland_server::Resource;
    let pid = surface.client()?.get_credentials(display).ok()?.pid as u32;
    Some(Address {
        service: String::new(),
        path: format!("/dome/menu_pid/{pid}"),
        gtk: Some(Export {
            discover_pid: Some(pid),
            app_menu: None,
            menubar: None,
            app_actions: None,
            window_actions: None,
        }),
    })
}

fn discover(c: &Connection, pid: u32) -> Option<Address> {
    let dbus = proxy(
        c,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .ok()?;
    let names: Vec<String> = dbus.call("ListNames", &()).ok()?;
    let mut found = vec![];
    for service in names.into_iter().filter(|s| s.starts_with(':')).take(256) {
        if dbus
            .call::<_, _, u32>("GetConnectionUnixProcessID", &(service.as_str(),))
            .ok()
            != Some(pid)
        {
            continue;
        }
        let mut pending = vec![("/".to_owned(), 0)];
        let mut visited = HashSet::new();
        while let Some((path, depth)) = pending.pop() {
            if depth > 12 || visited.len() >= 128 || !visited.insert(path.clone()) {
                continue;
            }
            let Ok(p) = proxy(c, &service, &path, "org.freedesktop.DBus.Introspectable") else {
                continue;
            };
            let Ok(xml) = p.call::<_, _, String>("Introspect", &()) else {
                continue;
            };
            let interfaces = xml_names(&xml, "interface");
            if interfaces.contains(&"org.gtk.Menus") && interfaces.contains(&"org.gtk.Actions") {
                found.push(Address {
                    service: service.clone(),
                    path: path.clone(),
                    gtk: Some(Export {
                        discover_pid: None,
                        app_menu: None,
                        menubar: Some(path.clone()),
                        app_actions: None,
                        window_actions: None,
                    }),
                });
            }
            for name in xml_names(&xml, "node").into_iter().take(128) {
                let child = format!("{}/{}", path.trim_end_matches('/'), name);
                if zbus::zvariant::ObjectPath::try_from(child.as_str()).is_ok() {
                    pending.push((child, depth + 1));
                }
            }
        }
    }
    // Never guess which window owns a menu when legacy metadata is ambiguous.
    (found.len() == 1).then(|| found.remove(0))
}
fn xml_names<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    xml.split(&format!("<{tag}"))
        .skip(1)
        .filter_map(|part| {
            let head = part.split_once('>')?.0;
            let value = head.split_once("name=")?.1.trim_start();
            let quote = value.chars().next()?;
            if quote != '\'' && quote != '"' {
                return None;
            }
            value[1..].split_once(quote).map(|(name, _)| name)
        })
        .collect()
}
fn text(props: &HashMap<String, OwnedValue>, key: &str) -> String {
    props
        .get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("")
        .to_owned()
}
fn link(props: &HashMap<String, OwnedValue>, key: &str) -> Option<(u32, u32)> {
    <(u32, u32)>::try_from(props.get(key)?.try_clone().ok()?).ok()
}
fn proxy<'a>(
    c: &'a Connection,
    service: &'a str,
    path: &'a str,
    interface: &'a str,
) -> zbus::Result<Proxy<'a>> {
    Proxy::new(c, service, path, interface)
}
fn load(c: &Connection, service: &str, path: &str) -> Option<Menus> {
    let p = proxy(c, service, path, "org.gtk.Menus").ok()?;
    let mut menus = HashMap::new();
    let mut started = HashSet::new();
    let mut pending = vec![0];
    let mut valid = true;
    while let Some(group) = pending.pop() {
        if started.contains(&group) {
            continue;
        }
        if started.len() >= 32 {
            valid = false;
            break;
        }
        let reply = p.call::<_, _, Vec<(u32, u32, Items)>>("Start", &(vec![group],));
        let Ok(reply) = reply else {
            valid = false;
            break;
        };
        started.insert(group);
        for (g, m, items) in reply {
            if items.len() > 4096 || menus.len() >= 256 {
                valid = false;
                break;
            }
            for item in &items {
                for key in [":section", ":submenu"] {
                    if let Some((g, _)) = link(item, key) {
                        if !started.contains(&g) {
                            pending.push(g);
                        }
                    }
                }
            }
            menus.insert((g, m), items);
        }
        if !valid {
            break;
        }
    }
    let _ = p.call_method("End", &(started.into_iter().collect::<Vec<_>>(),));
    valid.then_some(menus)
}
impl Reader {
    pub fn read(&mut self, c: &Connection, a: &Address) -> Option<Node> {
        if let Some(pid) = a.gtk.as_ref()?.discover_pid {
            if self.discovered.len() > 64 {
                self.discovered.clear();
            }
            let entry = self
                .discovered
                .entry(pid)
                .or_insert_with(|| (std::time::Instant::now(), discover(c, pid)));
            if entry.0.elapsed().as_secs_f32() > 2. {
                *entry = (std::time::Instant::now(), discover(c, pid));
            }
            let address = entry.1.clone()?;
            return self.read(c, &address);
        }
        self.actions.clear();
        let export = a.gtk.as_ref()?;
        if self.state_service != a.service {
            self.action_states.clear();
            self.state_service = a.service.clone();
        }
        // A slow application must not make enabled menu items flicker off.
        // Keep its last description on timeout; activation always revalidates.
        let mut states = std::mem::take(&mut self.action_states);
        states.retain(|path, _| {
            export.app_actions.as_ref() == Some(path)
                || export.window_actions.as_ref() == Some(path)
                || *path == a.path
        });
        for path in [
            &export.app_actions,
            &export.window_actions,
            &Some(a.path.clone()),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(p) = proxy(c, &a.service, path, "org.gtk.Actions") {
                if let Ok(values) = p.call::<_, _, HashMap<String, ActionState>>("DescribeAll", &())
                {
                    states.insert(path.clone(), values);
                }
            }
        }
        let mut root = Node::default();
        if let Some(path) = &export.app_menu {
            if let Some(menus) = load(c, &a.service, path) {
                let children = self.nodes(
                    &menus,
                    (0, 0),
                    export,
                    &states,
                    path,
                    "root",
                    &mut HashSet::new(),
                    0,
                );
                if !children.is_empty() {
                    root.children.push(Node {
                        id: -1,
                        text: "Application".into(),
                        enabled: true,
                        submenu: true,
                        children,
                        ..Node::default()
                    });
                }
            }
        }
        if let Some(path) = &export.menubar {
            if let Some(menus) = load(c, &a.service, path) {
                root.children.extend(self.nodes(
                    &menus,
                    (0, 0),
                    export,
                    &states,
                    path,
                    "root",
                    &mut HashSet::new(),
                    0,
                ));
            }
        }
        self.action_states = states;
        Some(root)
    }
    fn nodes(
        &mut self,
        menus: &Menus,
        key: (u32, u32),
        export: &Export,
        states: &HashMap<String, HashMap<String, ActionState>>,
        path: &str,
        identity: &str,
        seen: &mut HashSet<(u32, u32)>,
        depth: usize,
    ) -> Vec<Node> {
        if depth > 16 || !seen.insert(key) {
            return vec![];
        }
        let mut out = vec![];
        for (index, item) in menus.get(&key).into_iter().flatten().enumerate().take(4096) {
            if let Some(section) = link(item, ":section") {
                let children = self.nodes(
                    menus,
                    section,
                    export,
                    states,
                    path,
                    &format!("{identity}/section{index}"),
                    seen,
                    depth + 1,
                );
                if !out.is_empty() && !children.is_empty() {
                    out.push(Node {
                        separator: true,
                        ..Node::default()
                    });
                }
                out.extend(children);
                continue;
            }
            let name = {
                let action = text(item, "action");
                if action.is_empty() {
                    text(item, "submenu-action")
                } else {
                    action
                }
            };
            let label = text(item, "label")
                .replace("__", "\0")
                .replace('_', "")
                .replace('\0', "_");
            let target = item.get("target").and_then(|v| v.try_clone().ok());
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            (path, identity, index, &label, &name, format!("{target:?}")).hash(&mut hash);
            let mut id = (hash.finish() & 0x7fffffff) as i32;
            while self.actions.contains_key(&id) {
                id = id.wrapping_add(1).max(1);
            }
            let submenu = link(item, ":submenu");
            let mut node = Node {
                id,
                text: label,
                enabled: submenu.is_some(),
                submenu: submenu.is_some(),
                shortcut: text(item, "accel"),
                ..Node::default()
            };
            if let Some(key) = submenu {
                node.children = self.nodes(
                    menus,
                    key,
                    export,
                    states,
                    path,
                    &format!("{identity}/item{index}"),
                    seen,
                    depth + 1,
                );
            }
            if let Some((scope, action)) = name.split_once('.') {
                let action_path = match scope {
                    "app" => &export.app_actions,
                    "win" => &export.window_actions,
                    "unity" => &Some(path.to_owned()),
                    _ => &None,
                };
                if let Some(action_path) = action_path {
                    if let Some((enabled, _, value)) =
                        states.get(action_path).and_then(|s| s.get(action))
                    {
                        node.enabled = *enabled;
                        if let Some(value) = value.first().filter(|_| !node.submenu) {
                            node.checkable = true;
                            node.radio = target.is_some();
                            node.checked = target.as_ref().map_or_else(
                                || bool::try_from(value).unwrap_or(false),
                                |t| t == value,
                            );
                        }
                        self.actions.insert(
                            id,
                            Action {
                                path: action_path.clone(),
                                name: action.into(),
                                target,
                                submenu: node.submenu,
                            },
                        );
                    }
                }
            }
            out.push(node);
        }
        seen.remove(&key);
        out
    }
    pub fn activate(&mut self, c: &Connection, a: &Address, id: i32) {
        if std::env::var_os("DOME_MENU_TRACE").is_some() {
            eprintln!("GTK activate: address={a:?} id={id}");
        }
        if let Some(pid) = a.gtk.as_ref().and_then(|e| e.discover_pid) {
            let address = self.discovered.get(&pid).and_then(|(_, a)| a.clone());
            if let Some(address) = address {
                self.activate(c, &address, id);
            }
            return;
        }
        // Refresh menu identity and enabled state before dispatching an action.
        if self.read(c, a).is_none() {
            return;
        }
        let Some(action) = self.actions.get(&id) else {
            if std::env::var_os("DOME_MENU_TRACE").is_some() {
                eprintln!("GTK action identity expired: {id}");
            }
            return;
        };
        if action.submenu {
            return;
        }
        if let Ok(p) = proxy(c, &a.service, &action.path, "org.gtk.Actions") {
            if let Ok((true, _, _)) =
                p.call::<_, _, ActionState>("Describe", &(action.name.as_str(),))
            {
                let args: Vec<OwnedValue> = action
                    .target
                    .as_ref()
                    .and_then(|v| v.try_clone().ok())
                    .into_iter()
                    .collect();
                let _ = p.call_method(
                    "Activate",
                    &(
                        action.name.as_str(),
                        args,
                        HashMap::<String, OwnedValue>::new(),
                    ),
                );
            }
        }
    }
    pub fn open(&mut self, c: &Connection, a: &Address, id: i32) {
        if std::env::var_os("DOME_MENU_TRACE").is_some() {
            eprintln!("GTK open: address={a:?} id={id}");
        }
        if let Some(pid) = a.gtk.as_ref().and_then(|e| e.discover_pid) {
            if let Some(address) = self.discovered.get(&pid).and_then(|(_, a)| a.clone()) {
                self.open(c, &address, id);
            }
            return;
        }
        let Some(action) = self.actions.get(&id).filter(|a| a.submenu) else {
            return;
        };
        if let Ok(p) = proxy(c, &a.service, &action.path, "org.gtk.Actions") {
            let _ = p.call_method(
                "SetState",
                &(
                    action.name.as_str(),
                    zbus::zvariant::Value::from(true),
                    HashMap::<String, OwnedValue>::new(),
                ),
            );
            self.opened
                .insert((a.service.clone(), action.path.clone(), action.name.clone()));
        }
    }
    pub fn close(&mut self, c: &Connection) {
        for (service, path, name) in self.opened.drain() {
            if let Ok(p) = proxy(c, &service, &path, "org.gtk.Actions") {
                let _ = p.call_method(
                    "SetState",
                    &(
                        name,
                        zbus::zvariant::Value::from(false),
                        HashMap::<String, OwnedValue>::new(),
                    ),
                );
            }
        }
    }
}

pub fn enable_legacy_module() {
    for name in ["GTK_MODULES", "UBUNTU_MENUPROXY"] {
        if std::env::var_os(name).is_none() {
            std::env::set_var(name, "");
        }
    }
    let mut roots = vec![
        std::path::PathBuf::from("/usr/lib"),
        "/usr/lib64".into(),
        "/usr/local/lib".into(),
    ];
    if let Ok(entries) = std::fs::read_dir("/usr/lib") {
        roots.extend(entries.flatten().map(|e| e.path()));
    }
    if roots.iter().any(|root| {
        root.join("gtk-3.0/modules/libappmenu-gtk-module.so")
            .is_file()
    }) {
        let mut modules: Vec<String> = std::env::var("GTK_MODULES")
            .unwrap_or_default()
            .split(':')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if !modules.iter().any(|s| s == "appmenu-gtk-module") {
            modules.push("appmenu-gtk-module".into());
        }
        std::env::set_var("GTK_MODULES", modules.join(":"));
        std::env::set_var("UBUNTU_MENUPROXY", "1");
    }
}
