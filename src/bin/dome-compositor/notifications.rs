//! Session notification service. UI snapshots never block on D-Bus clients.
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use zbus::{blocking::Connection, object_server::SignalEmitter, zvariant::OwnedValue};
const PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";
#[derive(Clone, Serialize)]
pub struct Notice {
    pub id: u32,
    pub timestamp: u64,
    pub app: String,
    pub summary: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<NoticeImage>,
    #[serde(skip)]
    source: Option<u64>,
    actions: Vec<[String; 2]>,
    critical: bool,
    #[serde(skip)]
    resident: bool,
    #[serde(skip)]
    deadline: Option<Instant>,
}
#[derive(Clone, Serialize)]
pub struct NoticeImage {
    pub width: u32,
    pub height: u32,
    pub rgba: String,
}
#[derive(Default)]
struct State {
    owns_bus: bool,
    forwarded: std::collections::VecDeque<(String,u32)>,
    revision: u64,
    next: u32,
    notices: Vec<Notice>,
    history: Vec<Notice>,
    unread: bool,
    dismissed_surfaces: std::collections::HashSet<u64>,
    surface_clicks: Vec<(u64, [f64; 2])>,
}
#[derive(Clone, Serialize)]
pub struct Toast {
    #[serde(flatten)]
    notice: Notice,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Toast {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && x < (self.x + self.w) as f64
            && y >= self.y as f64
            && y < (self.y + self.h) as f64
    }
}
struct Server(Arc<Mutex<State>>);
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&str> {
        vec!["body", "actions"]
    }
    fn get_server_information(&self) -> (&str, &str, &str, &str) {
        ("DomeD", "DomeD", env!("CARGO_PKG_VERSION"), "1.2")
    }
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> zbus::fdo::Result<u32> {
        if actions.len() % 2 != 0
            || actions.len() > 32
            || summary.len()
                + body.len()
                + app_name.len()
                + actions.iter().map(|a| a.len()).sum::<usize>()
                > 65536
        {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Notification is too large or actions are not pairs".into(),
            ));
        }
        let delivery=hints.get("x-dome-event-id").and_then(|v|<&str>::try_from(v).ok().map(str::to_owned)).filter(|s|!s.is_empty()&&s.len()<=256&&app_name=="WayVR");
        let mut s = self.0.lock().unwrap();
        if let Some(key)=&delivery {if let Some((_,id))=s.forwarded.iter().find(|(k,_)|k==key){return Ok(*id);}}
        let index = s.notices.iter().position(|n| n.id == replaces_id);
        if index.is_none() && s.notices.len() >= 128 {
            return Err(zbus::fdo::Error::LimitsExceeded(
                "Too many active notifications".into(),
            ));
        }
        let id = if index.is_some() {
            replaces_id
        } else {
            loop {
                s.next = s.next.wrapping_add(1).max(1);
                if !s.notices.iter().any(|n| n.id == s.next) {
                    break s.next;
                }
            }
        };
        if let Some(key)=delivery{s.forwarded.push_back((key,id));while s.forwarded.len()>512{s.forwarded.pop_front();}}
        let critical = hints.get("urgency").and_then(|v| u8::try_from(v).ok()) == Some(2);
        let resident = hints
            .get("resident")
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(false);
        let timeout = if critical || expire_timeout == 0 {
            None
        } else {
            Some(Duration::from_millis(if expire_timeout < 0 {
                8000
            } else {
                expire_timeout as u64
            }))
        };
        let notice = Notice {
            id,
            timestamp: crate::clipboard::now(),
            app: app_name,
            summary,
            body,
            image: None,
            source: None,
            actions: actions
                .chunks_exact(2)
                .map(|p| [p[0].clone(), p[1].clone()])
                .collect(),
            critical,
            resident,
            deadline: timeout.map(|d| Instant::now() + d),
        };
        s.history.retain(|n| n.id != id);
        s.history.push(notice.clone());
        if s.history.len() > 200 {
            s.history.remove(0);
        }
        s.unread = true;
        if let Some(i) = index {
            s.notices[i] = notice;
        } else {
            s.notices.push(notice);
        }
        s.revision = s.revision.wrapping_add(1);
        Ok(id)
    }
    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        if !remove(&self.0, id) {
            return Err(zbus::fdo::Error::InvalidArgs("Unknown notification".into()));
        }
        Self::notification_closed(&emitter, id, 3).await?;
        Ok(())
    }
    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}
fn remove(state: &Mutex<State>, id: u32) -> bool {
    let mut s = state.lock().unwrap();
    let old = s.notices.len();
    if let Some(source) = s.notices.iter().find(|n| n.id == id).and_then(|n| n.source) {
        s.dismissed_surfaces.insert(source);
    }
    s.notices.retain(|n| n.id != id);
    if old == s.notices.len() {
        return false;
    }
    s.revision = s.revision.wrapping_add(1);
    true
}
enum Request {
    Dismiss(u32),
    Action(u32, String),
}
pub struct Notifications {
    state: Arc<Mutex<State>>,
    send: mpsc::Sender<Request>,
}
impl Notifications {
    pub fn forward(&self,notice:dome::wivrn::Notice) {
        let mut hints=HashMap::new();hints.insert("x-dome-event-id".into(),zbus::zvariant::OwnedValue::from(zbus::zvariant::Str::from(notice.id)));
        let _=Server(self.state.clone()).notify("WayVR".into(),0,String::new(),notice.title,notice.body,vec![],hints,notice.timeout);
    }
    pub fn owns_bus(&self)->bool {self.state.lock().unwrap().owns_bus}
    pub fn screenshot(&self,title:&str,body:&str) {
        let _=Server(self.state.clone()).notify("Dome Screenshot".into(),0,"camera-photo".into(),
            title.into(),body.into(),vec![],HashMap::new(),5000);
    }

    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let (send, receive) = mpsc::channel();
        let shared = state.clone();
        std::thread::spawn(move || {
            if let Err(e) = run(shared.clone(), receive) {
                eprintln!("Notification service unavailable (another desktop may own it): {e}");
            }
            shared.lock().unwrap().owns_bus=false;
        });
        Self { state, send }
    }
    /// Keep Steam's rendered notification content in the same stack and history
    /// as D-Bus notices. It never becomes a normal desktop or XR application.
    pub fn surface(&self, source: u64, width: u32, height: u32, rgba: &[u8]) {
        use base64::Engine;
        if width == 0
            || height == 0
            || width > 1024
            || height > 512
            || rgba.len() != width as usize * height as usize * 4
        {
            return;
        }
        let mut s = self.state.lock().unwrap();
        if s.dismissed_surfaces.contains(&source) {
            return;
        }
        let image = NoticeImage {
            width,
            height,
            rgba: base64::engine::general_purpose::STANDARD.encode(rgba),
        };
        let existing = s.notices.iter().find(|n| n.source == Some(source)).cloned();
        let notice = if let Some(mut n) = existing {
            if n.image.as_ref().is_some_and(|old| {
                old.width == width && old.height == height && old.rgba == image.rgba
            }) {
                return;
            }
            n.image = Some(image);
            n
        } else {
            if s.notices.len() >= 128 {
                return;
            }
            loop {
                s.next = s.next.wrapping_add(1).max(1);
                if !s.notices.iter().any(|n| n.id == s.next) {
                    break;
                }
            }
            Notice {
                id: s.next,
                timestamp: crate::clipboard::now(),
                app: "Steam".into(),
                summary: "Steam notification".into(),
                body: String::new(),
                image: Some(image),
                source: Some(source),
                actions: Vec::new(),
                critical: false,
                resident: true,
                deadline: None,
            }
        };
        s.notices.retain(|n| n.id != notice.id);
        s.notices.push(notice.clone());
        // Images are bounded independently of the 200 text-history entries.
        if let Some(n) = s.history.iter_mut().find(|n| n.id == notice.id) {
            *n = notice;
        } else {
            s.history.push(notice);
        }
        while s.history.iter().filter(|n| n.image.is_some()).count() > 16 {
            let i = s.history.iter().position(|n| n.image.is_some()).unwrap();
            s.history.remove(i);
        }
        if s.history.len() > 200 {
            s.history.remove(0);
        }
        s.unread = true;
        s.revision = s.revision.wrapping_add(1);
    }
    pub fn withdraw_surface(&self, source: u64) {
        let mut s = self.state.lock().unwrap();
        let len = s.notices.len();
        s.notices.retain(|n| n.source != Some(source));
        s.dismissed_surfaces.remove(&source);
        if s.notices.len() != len {
            s.revision = s.revision.wrapping_add(1);
        }
    }
    pub fn next_surface_click(&self) -> Option<(u64, [f64; 2])> {
        let mut state = self.state.lock().unwrap();
        if state.surface_clicks.is_empty() {
            None
        } else {
            Some(state.surface_clicks.remove(0))
        }
    }
    pub fn history(&self) -> Vec<Notice> {
        self.state
            .lock()
            .unwrap()
            .history
            .iter()
            .rev()
            .cloned()
            .collect()
    }
    pub fn unread(&self) -> bool {
        self.state.lock().unwrap().unread
    }
    pub fn mark_read(&self) {
        let mut s = self.state.lock().unwrap();
        s.unread = false;
        s.revision += 1;
    }
    pub fn clear_history(&self) {
        let mut s = self.state.lock().unwrap();
        s.history.clear();
        s.unread = false;
        s.revision += 1;
    }
    pub fn remove_history(&self, id: u32) {
        let mut s = self.state.lock().unwrap();
        s.history.retain(|n| n.id != id);
        s.revision += 1;
    }
    pub fn revision(&self) -> u64 {
        self.state.lock().unwrap().revision
    }
    pub fn layout(&self, width: i32, height: i32) -> Vec<Toast> {
        let s = self.state.lock().unwrap();
        let w = 360.min(width - 24).max(1);
        let mut y = 48;
        let mut result = vec![];
        for n in s.notices.iter().rev().take(3) {
            let count = n.actions.iter().filter(|a| a[0] != "default").count() as i32;
            let h = if let Some(image) = &n.image {
                36 + ((w - 16) as f64 * image.height as f64 / image.width as f64)
                    .round()
                    .min(256.) as i32
            } else {
                100 + ((count + 1) / 2) * 32
            };
            if y + h > height - 72 {
                break;
            }
            result.push(Toast {
                notice: n.clone(),
                x: width - w - 12,
                y,
                w,
                h,
            });
            y += h + 10;
        }
        result
    }
    pub fn click(&self, toast: &Toast, x: f64, y: f64) {
        let n = &toast.notice;
        if let Some(source) = n.source {
            if x >= (toast.x + toast.w - 32) as f64 && y < (toast.y + 32) as f64 {
                let _ = self.send.send(Request::Dismiss(n.id));
            } else if y >= (toast.y + 32) as f64 {
                let uv = [
                    ((x - toast.x as f64 - 8.) / (toast.w - 16) as f64).clamp(0., 1.),
                    ((y - toast.y as f64 - 32.) / (toast.h - 36) as f64).clamp(0., 1.),
                ];
                let mut s = self.state.lock().unwrap();
                if s.surface_clicks.len() < 32 {
                    s.surface_clicks.push((source, uv));
                }
            }
            return;
        }
        let request = if x >= (toast.x + toast.w - 32) as f64 && y < (toast.y + 32) as f64 {
            Request::Dismiss(n.id)
        } else if y >= (toast.y + 100) as f64 {
            let row = ((y - toast.y as f64 - 100.) / 32.) as usize;
            let col = usize::from(x >= (toast.x + toast.w / 2) as f64);
            let Some(a) = n
                .actions
                .iter()
                .filter(|a| a[0] != "default")
                .nth(row * 2 + col)
            else {
                return;
            };
            Request::Action(n.id, a[0].clone())
        } else if n.actions.iter().any(|a| a[0] == "default") {
            Request::Action(n.id, "default".into())
        } else {
            Request::Dismiss(n.id)
        };
        let _ = self.send.send(request);
    }
}
fn run(state: Arc<Mutex<State>>, receive: mpsc::Receiver<Request>) -> zbus::Result<()> {
    // Export first so the initial caller cannot race interface registration.
    // Never replace the host desktop's notification daemon.
    let connection = Connection::session()?;
    connection.object_server().at(PATH, Server(state.clone()))?;
    let owner=connection.request_name_with_flags(INTERFACE, zbus::fdo::RequestNameFlags::DoNotQueue.into())?;
    if !matches!(owner,zbus::fdo::RequestNameReply::PrimaryOwner|zbus::fdo::RequestNameReply::AlreadyOwner){return Err(zbus::Error::Failure("Notification service already owned".into()));}
    state.lock().unwrap().owns_bus=true;
    loop {
        while let Ok(request) = receive.try_recv() {
            match request {
                Request::Dismiss(id) => {
                    if remove(&state, id) {
                        connection.emit_signal(
                            None::<&str>,
                            PATH,
                            INTERFACE,
                            "NotificationClosed",
                            &(id, 2u32),
                        )?;
                    }
                }
                Request::Action(id, key) => {
                    let notice = state
                        .lock()
                        .unwrap()
                        .notices
                        .iter()
                        .find(|n| n.id == id)
                        .cloned();
                    if let Some(n) = notice.filter(|n| n.actions.iter().any(|a| a[0] == key)) {
                        connection.emit_signal(
                            None::<&str>,
                            PATH,
                            INTERFACE,
                            "ActionInvoked",
                            &(id, key),
                        )?;
                        if !n.resident && remove(&state, id) {
                            connection.emit_signal(
                                None::<&str>,
                                PATH,
                                INTERFACE,
                                "NotificationClosed",
                                &(id, 2u32),
                            )?;
                        }
                    }
                }
            }
        }
        let expired: Vec<_> = state
            .lock()
            .unwrap()
            .notices
            .iter()
            .filter(|n| n.deadline.is_some_and(|d| Instant::now() >= d))
            .map(|n| n.id)
            .collect();
        for id in expired {
            if remove(&state, id) {
                connection.emit_signal(
                    None::<&str>,
                    PATH,
                    INTERFACE,
                    "NotificationClosed",
                    &(id, 1u32),
                )?;
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forwarded_wayvr_notices_have_one_popup_and_history_entry() {
        let state=Arc::new(Mutex::new(State::default()));
        let server=Server(state.clone());
        let notify=|key:&str| {
            let mut hints=HashMap::new();
            hints.insert("x-dome-event-id".into(),zbus::zvariant::OwnedValue::from(zbus::zvariant::Str::from(key)));
            server.notify("WayVR".into(),0,String::new(),"one".into(),"body".into(),vec![],hints,5000).unwrap()
        };
        let id=notify("one");assert_eq!(notify("one"),id);
        assert_eq!(state.lock().unwrap().notices.len(),1);
        assert_eq!(state.lock().unwrap().history.len(),1);
        assert!(remove(&state,id));assert_eq!(notify("one"),id);
        assert_eq!(state.lock().unwrap().notices.len(),0);
        assert_ne!(notify("two"),id);
    }
    #[test]
    fn surface_notifications_replace_dismiss_and_bound_image_history() {
        let (send, _) = mpsc::channel();
        let notices = Notifications {
            state: Arc::new(Mutex::new(State::default())),
            send,
        };
        notices.surface(5, 2, 1, &[255; 8]);
        notices.surface(5, 2, 1, &[128; 8]);
        assert_eq!(notices.history().len(), 1);
        let id = notices.history()[0].id;
        assert!(remove(&notices.state, id));
        notices.surface(5, 2, 1, &[64; 8]);
        assert!(notices.layout(1024, 768).is_empty());
        notices.withdraw_surface(5);
        notices.surface(5, 2, 1, &[32; 8]);
        assert_eq!(notices.layout(1024, 768).len(), 1);
        for id in 10..30 {
            notices.surface(id, 2, 1, &[0; 8]);
        }
        assert_eq!(notices.history().len(), 16);
    }
    #[test]
    fn replacement_expiry_and_dismissal_keep_a_bounded_notification_history() {
        let state = Arc::new(Mutex::new(State::default()));
        let server = Server(state.clone());
        let notify = |replaces, summary: &str| {
            server
                .notify(
                    "app".into(),
                    replaces,
                    String::new(),
                    summary.into(),
                    "body".into(),
                    vec![],
                    HashMap::new(),
                    100,
                )
                .unwrap()
        };
        let id = notify(0, "first");
        assert_eq!(notify(id, "replacement"), id);
        assert_eq!(state.lock().unwrap().history.len(), 1);
        assert!(remove(&state, id));
        assert_eq!(state.lock().unwrap().history[0].summary, "replacement");
        for _ in 0..210 {
            let id = notify(0, "later");
            remove(&state, id);
        }
        let (send, _) = mpsc::channel();
        let notices = Notifications { state, send };
        assert_eq!(notices.history().len(), 200);
        assert!(notices.unread());
        notices.mark_read();
        assert!(!notices.unread());
        notices.clear_history();
        assert!(notices.history().is_empty());
    }
}
