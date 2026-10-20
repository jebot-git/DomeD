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
    actions: Vec<[String; 2]>,
    critical: bool,
    #[serde(skip)]
    resident: bool,
    #[serde(skip)]
    deadline: Option<Instant>,
}
#[derive(Default)]
struct State {
    revision: u64,
    next: u32,
    notices: Vec<Notice>,
    history: Vec<Notice>,
    unread: bool,
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
        let mut s = self.0.lock().unwrap();
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
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let (send, receive) = mpsc::channel();
        let shared = state.clone();
        std::thread::spawn(move || {
            if let Err(e) = run(shared, receive) {
                eprintln!("Notification service unavailable (another desktop may own it): {e}");
            }
        });
        Self { state, send }
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
            let h = 100 + ((count + 1) / 2) * 32;
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
    connection
        .request_name_with_flags(INTERFACE, zbus::fdo::RequestNameFlags::DoNotQueue.into())?;
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
