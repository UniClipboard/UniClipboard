use super::{Document, WindowPreferences};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{mpsc, Mutex},
    thread,
    time::Duration,
};
use tracing::warn;

const DEBOUNCE: Duration = Duration::from_millis(500);
const MAX_BYTES: u64 = 64 * 1024;

enum WriteRequest {
    Update(Document),
    Flush(mpsc::SyncSender<io::Result<()>>),
}

/// One process-local owner and one serialized writer for all desktop windows.
pub struct PreferencesStore {
    document: Mutex<Document>,
    writer: mpsc::Sender<WriteRequest>,
}
impl PreferencesStore {
    pub fn load(path: PathBuf) -> io::Result<Self> {
        Self::load_with_debounce(path, DEBOUNCE)
    }

    fn load_with_debounce(path: PathBuf, debounce: Duration) -> io::Result<Self> {
        let (document, writable) = match read(&path) {
            Ok(document) => (document, true),
            Err(error) => {
                warn!(error_kind = "desktop_preferences_read", error = %error, "Desktop preferences unavailable; preserving file and using session defaults");
                (Document::default(), false)
            }
        };
        let (writer, receiver) = mpsc::channel();
        let span = tracing::Span::current();
        thread::Builder::new().name("desktop-preferences".into()).spawn(move || {
            let _entered = span.enter();
            let mut pending = None;
            loop {
                let request = if pending.is_some() { receiver.recv_timeout(debounce) }
                    else { receiver.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected) };
                match request {
                    Ok(WriteRequest::Update(document)) => { if writable { pending = Some(document); } }
                    Ok(WriteRequest::Flush(reply)) => { let _ = reply.send(save_pending(&path, &mut pending)); }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Err(error) = save_pending(&path, &mut pending) {
                            warn!(error_kind = "desktop_preferences_write", error = %error, "Failed to save desktop preferences; retrying on next update or flush");
                            // Keep the last value for a later flush, without spinning on a
                            // persistent filesystem error every debounce interval.
                            match receiver.recv() {
                                Ok(WriteRequest::Update(document)) => pending = Some(document),
                                Ok(WriteRequest::Flush(reply)) => { let _ = reply.send(save_pending(&path, &mut pending)); }
                                Err(_) => break,
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if let Err(error) = save_pending(&path, &mut pending) {
                            warn!(error_kind = "desktop_preferences_write", error = %error, "Failed to flush desktop preferences at writer shutdown");
                        }
                        break;
                    }
                }
            }
        })?;
        Ok(Self {
            document: Mutex::new(document),
            writer,
        })
    }

    pub fn window(&self, key: &str) -> io::Result<Option<WindowPreferences>> {
        Ok(self
            .document
            .lock()
            .map_err(|_| io::Error::other("desktop preferences lock poisoned"))?
            .windows
            .get(key)
            .cloned())
    }
    pub fn update(&self, key: &str, value: WindowPreferences) -> io::Result<()> {
        if key.is_empty() || key.len() > 128 || !value.valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid window preferences",
            ));
        }
        // Enqueue under the state lock: concurrent windows cannot reorder revisions.
        let mut document = self
            .document
            .lock()
            .map_err(|_| io::Error::other("desktop preferences lock poisoned"))?;
        if document.windows.get(key) == Some(&value) {
            return Ok(());
        }
        if !document.windows.contains_key(key) && document.windows.len() >= 32 {
            return Err(io::Error::other("too many desktop windows"));
        }
        document.windows.insert(key.to_owned(), value);
        self.writer
            .send(WriteRequest::Update(document.clone()))
            .map_err(|_| io::Error::other("desktop preferences writer stopped"))
    }
    /// Flush only accepted preferences, never recapture a clamped live window.
    pub fn flush(&self) -> io::Result<()> {
        let (reply, result) = mpsc::sync_channel(1);
        self.writer
            .send(WriteRequest::Flush(reply))
            .map_err(|_| io::Error::other("desktop preferences writer stopped"))?;
        result
            .recv()
            .map_err(|_| io::Error::other("desktop preferences writer stopped"))?
    }
}

fn read(path: &Path) -> io::Result<Document> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Document::default()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::other("desktop preferences too large"));
    }
    let document: Document = serde_json::from_slice(&bytes)?;
    if !document.valid() {
        return Err(io::Error::other(
            "invalid or unsupported desktop preferences",
        ));
    }
    Ok(document)
}
fn save_pending(path: &Path, pending: &mut Option<Document>) -> io::Result<()> {
    if let Some(document) = pending.as_ref() {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("missing preferences directory"))?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temporary, document)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        *pending = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{NormalGeometry, Size};
    use super::*;
    fn value(width: f64) -> WindowPreferences {
        WindowPreferences {
            version: 1,
            normal: NormalGeometry {
                inner_size: Size {
                    width,
                    height: 700.0,
                },
                placement: None,
            },
            maximized: true,
        }
    }
    #[test]
    fn restart_restores_latest_window_and_keeps_settings_independent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop-preferences.json");
        let settings = dir.path().join("settings.json");
        fs::write(&settings, "untouched").unwrap();
        let store =
            PreferencesStore::load_with_debounce(path.clone(), Duration::from_secs(60)).unwrap();
        store.update("main", value(1100.0)).unwrap();
        store.update("other", value(900.0)).unwrap();
        store.update("main", value(1200.0)).unwrap();
        assert!(!path.exists());
        store.flush().unwrap();
        let reopened = PreferencesStore::load(path).unwrap();
        assert_eq!(reopened.window("main").unwrap(), Some(value(1200.0)));
        assert_eq!(reopened.window("other").unwrap(), Some(value(900.0)));
        assert_eq!(fs::read_to_string(settings).unwrap(), "untouched");
    }
    #[test]
    fn malformed_future_and_oversized_files_are_never_overwritten() {
        for bytes in [
            b"{".to_vec(),
            br#"{"schemaVersion":2,"windows":{}}"#.to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("desktop-preferences.json");
            fs::write(&path, &bytes).unwrap();
            let store = PreferencesStore::load(path.clone()).unwrap();
            store.update("main", value(1200.0)).unwrap();
            store.flush().unwrap();
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
    #[test]
    fn failed_write_preserves_original_and_can_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("prefs");
        let path = parent.join("desktop-preferences.json");
        let store = PreferencesStore::load(path.clone()).unwrap();
        fs::write(&parent, "blocked").unwrap();
        store.update("main", value(1200.0)).unwrap();
        assert!(store.flush().is_err());
        assert_eq!(fs::read_to_string(&parent).unwrap(), "blocked");
        fs::remove_file(&parent).unwrap();
        store.flush().unwrap();
        assert_eq!(read(&path).unwrap().windows["main"], value(1200.0));
    }
    #[test]
    fn debounced_update_reaches_disk_without_close() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let store =
            PreferencesStore::load_with_debounce(path.clone(), Duration::from_millis(10)).unwrap();
        store.update("main", value(1100.0)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !path.exists() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(read(&path).unwrap().windows["main"], value(1100.0));
    }
    #[test]
    fn concurrent_windows_do_not_lose_each_others_updates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let store = std::sync::Arc::new(PreferencesStore::load(path.clone()).unwrap());
        let workers: Vec<_> = ["main", "other"]
            .into_iter()
            .map(|key| {
                let store = store.clone();
                thread::spawn(move || {
                    for width in 900..920 {
                        store.update(key, value(width as f64)).unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        store.flush().unwrap();
        let saved = read(&path).unwrap();
        assert_eq!(saved.windows["main"], value(919.0));
        assert_eq!(saved.windows["other"], value(919.0));
    }

    #[test]
    fn future_window_record_is_preserved_and_invalid_updates_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut document = Document::default();
        let mut future = value(1200.0);
        future.version = 2;
        document.windows.insert("main".into(), future);
        let bytes = serde_json::to_vec(&document).unwrap();
        fs::write(&path, &bytes).unwrap();
        let store = PreferencesStore::load(path.clone()).unwrap();
        assert!(store.update("main", value(f64::NAN)).is_err());
        store.update("main", value(1300.0)).unwrap();
        store.flush().unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
