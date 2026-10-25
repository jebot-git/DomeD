//! Bounded session clipboard history. Transfers never block the compositor.
use smithay::{
    input::Seat,
    wayland::selection::{
        data_device::{request_data_device_client_selection, set_data_device_selection},
        SelectionSource,
    },
};
use std::{
    io::{Read, Write},
    os::{fd::OwnedFd, unix::net::UnixStream},
    sync::{mpsc, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOTAL: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub timestamp: u64,
    pub formats: Vec<(String, Vec<u8>)>,
}
impl Entry {
    pub fn description(&self) -> String {
        if self
            .formats
            .iter()
            .any(|(m, _)| m == "text/uri-list" || m == "x-special/gnome-copied-files")
        {
            "Files".into()
        } else if self.formats.iter().any(|(m, _)| m == "model/gltf-binary") {
            "XR model".into()
        } else if self.formats.iter().any(|(m, _)| m.starts_with("image/")) {
            "Image".into()
        } else if self
            .formats
            .iter()
            .any(|(m, _)| m.starts_with("text/") || m == "UTF8_STRING")
        {
            "Text".into()
        } else {
            "Clipboard data".into()
        }
    }
    pub fn text(&self) -> String {
        let preferred = [
            "text/plain;charset=utf-8",
            "text/plain",
            "UTF8_STRING",
            "text/uri-list",
            "x-special/gnome-copied-files",
        ];
        preferred
            .iter()
            .find_map(|mime| self.formats.iter().find(|(m, _)| m == mime))
            .map(|(_, data)| String::from_utf8_lossy(data).chars().take(240).collect())
            .unwrap_or_else(|| {
                format!(
                    "{} · {} bytes",
                    self.description(),
                    self.formats
                        .first()
                        .map(|(_, data)| data.len())
                        .unwrap_or(0)
                )
            })
    }
    fn size(&self) -> usize {
        self.formats.iter().map(|(_, data)| data.len()).sum()
    }
}
pub struct Clipboard {
    pub entries: Vec<Arc<Entry>>,
    pub revision: u64,
    pending: Option<Vec<String>>,
    next: u64,
    cleared: u64,
    send: mpsc::Sender<Entry>,
    receive: mpsc::Receiver<Entry>,
    inflight: Arc<std::sync::atomic::AtomicUsize>,
}
impl Clipboard {
    pub fn new() -> Self {
        let (send, receive) = mpsc::channel();
        Self {
            entries: vec![],
            revision: 0,
            pending: None,
            next: 0,
            cleared: 0,
            send,
            receive,
            inflight: Arc::new(0.into()),
        }
    }
    pub fn changed(&mut self, source: Option<SelectionSource>) {
        self.next += 1;
        self.pending = source.map(|s| s.mime_types());
    }
    fn record(&mut self, entry: Entry) {
        if entry.formats.is_empty() || entry.id <= self.cleared {
            return;
        }
        self.entries.retain(|e| e.formats != entry.formats);
        self.entries.push(Arc::new(entry));
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.id));
        while self.entries.len() > 50
            || self.entries.iter().map(|e| e.size()).sum::<usize>() > MAX_TOTAL
        {
            self.entries.pop();
        }
        self.revision += 1;
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cleared = self.next;
        self.revision += 1;
    }
    pub fn remove(&mut self, id: u64) {
        self.entries.retain(|e| e.id != id);
        self.revision += 1;
    }
    pub fn poll(&mut self, seat: &Seat<crate::Dome>) {
        while let Ok(entry) = self.receive.try_recv() {
            self.record(entry);
        }
        let Some(mimes) = self.pending.take() else {
            return;
        };
        use std::sync::atomic::Ordering;
        if self.inflight.load(Ordering::Relaxed) >= 4 {
            return;
        }
        let mut readers = vec![];
        for mime in mimes.into_iter().take(16) {
            if readers.iter().any(|(m, _)| m == &mime) {
                continue;
            }
            let Ok((read, write)) = UnixStream::pair() else {
                continue;
            };
            let _ = read.set_read_timeout(Some(Duration::from_millis(500)));
            if request_data_device_client_selection(seat, mime.clone(), write.into()).is_ok() {
                readers.push((mime, read));
            }
        }
        if readers.is_empty() {
            return;
        }
        let send = self.send.clone();
        let id = self.next;
        let inflight = self.inflight.clone();
        inflight.fetch_add(1, Ordering::Relaxed);
        std::thread::spawn(move || {
            let mut entry = Entry {
                id,
                timestamp: now(),
                formats: vec![],
            };
            let mut size = 0;
            for (mime, read) in readers {
                let mut data = vec![];
                if read
                    .take((MAX_BYTES - size + 1) as u64)
                    .read_to_end(&mut data)
                    .is_ok()
                    && size + data.len() <= MAX_BYTES
                {
                    size += data.len();
                    entry.formats.push((mime, data));
                }
            }
            let _ = send.send(entry);
            inflight.fetch_sub(1, Ordering::Relaxed);
        });
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn send_data(fd: OwnedFd, data: Vec<u8>) {
    std::thread::spawn(move || {
        use smithay::reexports::rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
        let Ok(flags) = fcntl_getfl(&fd) else {
            return;
        };
        if fcntl_setfl(&fd, flags | OFlags::NONBLOCK).is_err() {
            return;
        }
        let mut file = std::fs::File::from(fd);
        let mut remaining = data.as_slice();
        let start = std::time::Instant::now();
        while !remaining.is_empty() && start.elapsed() < Duration::from_secs(2) {
            match file.write(remaining) {
                Ok(0) => break,
                Ok(n) => remaining = &remaining[n..],
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
}
impl crate::Dome {
    pub fn publish_capture(&mut self, capture: dome::screenshot::Capture) {
        let mime = capture.mime_type().to_owned();
        let bytes = match capture {
            dome::screenshot::Capture::Png { bytes, .. } | dome::screenshot::Capture::Gltf(bytes) => bytes,
        };
        self.clipboard.next += 1;
        let entry = Arc::new(Entry { id: self.clipboard.next, timestamp: now(),
            formats: vec![(mime.clone(), bytes)] });
        self.clipboard.record((*entry).clone());
        set_data_device_selection::<Self>(&self.display_handle, &self.seat,
            vec![mime], entry);
    }

    pub fn restore_clipboard(&mut self, id: u64) {
        if let Some(entry) = self.clipboard.entries.iter().find(|e| e.id == id).cloned() {
            set_data_device_selection::<Self>(
                &self.display_handle,
                &self.seat,
                entry.formats.iter().map(|(mime, _)| mime.clone()).collect(),
                entry,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_deduplicated_history_survives_source_changes_and_clear_discards_inflight() {
        let mut c = Clipboard::new();
        for id in 1..=60 {
            c.record(Entry {
                id,
                timestamp: 0,
                formats: vec![("text/plain".into(), id.to_string().into_bytes())],
            });
        }
        assert_eq!(c.entries.len(), 50);
        assert_eq!(c.entries[0].id, 60);
        c.record(Entry {
            id: 61,
            timestamp: 0,
            formats: vec![("text/plain".into(), b"60".to_vec())],
        });
        assert_eq!(c.entries.len(), 50);
        assert_eq!(c.entries[0].id, 61);
        c.next = 62;
        c.clear();
        c.record(Entry {
            id: 62,
            timestamp: 0,
            formats: vec![("text/plain".into(), b"late".to_vec())],
        });
        assert!(c.entries.is_empty());
    }
    #[test]
    fn binary_entries_keep_mime_and_exact_bytes() {
        let mut c = Clipboard::new();
        let formats = vec![("image/png".into(), vec![0, 255, 128, 0])];
        c.record(Entry {
            id: 1,
            timestamp: 0,
            formats: formats.clone(),
        });
        assert_eq!(c.entries[0].formats, formats);
        assert!(c.entries[0].text().starts_with("Image"));
    }
}
