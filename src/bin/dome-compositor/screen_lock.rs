//! LightXR owns PAM; the compositor only blanks/protects its session until logind unlocks it.
use dome::appearance::SessionAction;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::SyncSender, Arc};
use zbus::blocking::{Connection, Proxy};

struct ScreenSaver {
    locked: Arc<AtomicBool>,
    send: SyncSender<SessionAction>,
}
#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    fn lock(&self) -> zbus::fdo::Result<()> {
        self.locked.store(true, Ordering::SeqCst);
        self.send.try_send(SessionAction::Lock).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    fn get_active(&self) -> bool { self.locked.load(Ordering::SeqCst) }
    fn set_active(&self, active: bool) -> zbus::fdo::Result<()> {
        if active { self.lock() }
        else { Err(zbus::fdo::Error::AccessDenied("Authenticate in LightXR to unlock".into())) }
    }
}

pub fn managed_session() -> bool {
    std::env::var("DOME_LOGIN_SESSION").as_deref() == Ok("1")
        && std::env::var("XDG_SESSION_PATH").is_ok_and(|p| p.starts_with("/org/freedesktop/DisplayManager/Session"))
}

pub fn register(locked: Arc<AtomicBool>, send: SyncSender<SessionAction>) -> Option<Connection> {
    if !managed_session() { return None; }
    let system = Connection::system().ok()?;
    let id = std::env::var("XDG_SESSION_ID").ok()?;
    let manager = Proxy::new(&system,"org.freedesktop.login1","/org/freedesktop/login1","org.freedesktop.login1.Manager").ok()?;
    let path: zbus::zvariant::OwnedObjectPath = manager.call("GetSession", &(id,)).ok()?;
    let session = Proxy::new(&system,"org.freedesktop.login1",path.as_str(),"org.freedesktop.login1.Session").ok()?;
    // Install the authenticated system-bus signal match before publishing Lock.
    let signals = session.receive_all_signals().ok()?;
    locked.store(session.get_property::<bool>("LockedHint").ok()?, Ordering::SeqCst);
    let connection = zbus::blocking::connection::Builder::session().ok()?
        .name("org.freedesktop.ScreenSaver").ok()?
        .serve_at("/ScreenSaver", ScreenSaver { locked:locked.clone(),send:send.clone() }).ok()?
        .build().ok()?;
    std::thread::spawn(move || {
        // The iterator owns the matched proxy/connection and filters the bus sender.
        for message in signals {
            match message.header().member().map(|s|s.as_str()) {
                Some("Lock") => {
                    if !locked.swap(true,Ordering::SeqCst) { let _=send.try_send(SessionAction::Lock); }
                }
                Some("Unlock") => locked.store(false,Ordering::SeqCst),
                _ => (),
            }
        }
        // Loss of the authority connection must fail closed.
        locked.store(true, Ordering::SeqCst);
    });
    Some(connection)
}

pub fn show_greeter(system: &Connection) -> Result<(),String> {
    let path=std::env::var("XDG_SESSION_PATH").map_err(|_|"LightXR session path is unavailable")?;
    let proxy = Proxy::new(system,"org.freedesktop.DisplayManager",path.as_str(),"org.freedesktop.DisplayManager.Session")
        .map_err(|e|e.to_string())?;
    proxy.call::<_,_,()>("Lock",&()).map_err(|e|e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_screensaver_clients_can_lock_but_never_unlock() {
        let (send, recv)=std::sync::mpsc::sync_channel(4);
        let locked=Arc::new(AtomicBool::new(false));
        let saver=ScreenSaver{locked:locked.clone(),send};
        saver.lock().unwrap();
        assert!(saver.get_active());
        assert_eq!(recv.recv().unwrap(),SessionAction::Lock);
        assert!(matches!(saver.set_active(false),Err(zbus::fdo::Error::AccessDenied(_))));
        assert!(locked.load(Ordering::SeqCst));
    }
}
