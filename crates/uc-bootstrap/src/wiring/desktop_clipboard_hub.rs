//! Shared Windows desktop clipboard hub for multi-space Engine hosts.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use uc_platform::clipboard::watcher::{ClipboardWatcher, PlatformEvent};
use uc_platform::clipboard::{
    build_event_loop, shutdown_channel, PlatformClipboardEventLoop, ShutdownTx, SystemClipboard,
    SystemClipboardSnapshot,
};

use crate::layer::platform::{create_desktop_system_clipboard, SystemClipboardWiring};
use crate::wiring::error::{WiringError, WiringResult};

const ECHO_GUARD_TTL: Duration = Duration::from_secs(5);
const NEXT_CHANGE_FALLBACK_WINDOW: Duration = Duration::from_millis(750);
const WATCHER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) type EventLoopFactory =
    Arc<dyn Fn() -> anyhow::Result<Box<dyn PlatformClipboardEventLoop>> + Send + Sync + 'static>;

/// One process-wide Windows system clipboard owner for multi-space runtimes.
///
/// Profile handles share the same physical clipboard and serialized write
/// path. Only the hub can yield the single watcher stream.
#[derive(Clone)]
pub struct DesktopClipboardHub {
    inner: Arc<DesktopClipboardHubInner>,
}

struct DesktopClipboardHubInner {
    system_clipboard: Arc<dyn SystemClipboard>,
    changes_enabled: bool,
    watcher_taken: AtomicBool,
    write_serialization: Mutex<()>,
    echo_suppression: Mutex<EchoSuppression>,
    event_loop_factory: EventLoopFactory,
}

/// A profile-local clipboard view backed by a shared [`DesktopClipboardHub`].
///
/// The pending slot preserves the exact event-time snapshot for a later
/// `Operation::ObserveClipboardChange`; ordinary reads fall back to the shared
/// physical clipboard. Writes always pass through the hub's global serializer
/// and echo suppression.
#[derive(Clone)]
pub struct DesktopClipboardProfileHandle {
    hub: DesktopClipboardHub,
    pending_snapshot: Arc<Mutex<Option<SystemClipboardSnapshot>>>,
}

/// Create the process-wide Windows clipboard hub from the normal desktop
/// system-clipboard layer.
pub fn prepare_desktop_clipboard_hub() -> WiringResult<DesktopClipboardHub> {
    let (_, system_clipboard, wiring) = create_desktop_system_clipboard()?.into_parts();
    Ok(DesktopClipboardHub::from_parts(
        system_clipboard,
        wiring == SystemClipboardWiring::Real,
        Arc::new(build_event_loop),
    ))
}

impl DesktopClipboardHub {
    pub(crate) fn from_parts(
        system_clipboard: Arc<dyn SystemClipboard>,
        changes_enabled: bool,
        event_loop_factory: EventLoopFactory,
    ) -> Self {
        Self {
            inner: Arc::new(DesktopClipboardHubInner {
                system_clipboard,
                changes_enabled,
                watcher_taken: AtomicBool::new(false),
                write_serialization: Mutex::new(()),
                echo_suppression: Mutex::new(EchoSuppression::default()),
                event_loop_factory,
            }),
        }
    }

    /// Create one profile-local handle. Handles never own or expose a watcher.
    pub fn profile_handle(&self) -> DesktopClipboardProfileHandle {
        DesktopClipboardProfileHandle {
            hub: self.clone(),
            pending_snapshot: Arc::new(Mutex::new(None)),
        }
    }

    /// Stage the event-time snapshot for one profile's next clipboard read.
    pub fn stage_snapshot(
        &self,
        profile: &DesktopClipboardProfileHandle,
        snapshot: SystemClipboardSnapshot,
    ) -> WiringResult<()> {
        if !Arc::ptr_eq(&self.inner, &profile.hub.inner) {
            return Err(WiringError::ClipboardInit(
                "clipboard profile handle belongs to a different hub".into(),
            ));
        }
        *lock_unpoisoned(&profile.pending_snapshot) = Some(snapshot);
        Ok(())
    }

    /// Take the process's single physical watcher stream.
    ///
    /// The first caller receives the stream; every later caller receives
    /// `None`. The OS listener starts lazily on the first `next()` call.
    pub fn take_change_stream(&self) -> WiringResult<Option<DesktopClipboardHubChangeStream>> {
        if !self.inner.changes_enabled {
            return Ok(None);
        }
        if self
            .inner
            .watcher_taken
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(None);
        }
        Ok(Some(DesktopClipboardHubChangeStream {
            hub: self.clone(),
            running: None,
        }))
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        let _write = lock_unpoisoned(&self.inner.write_serialization);
        let now = Instant::now();
        let mut suppression = lock_unpoisoned(&self.inner.echo_suppression);
        let guard_id = suppression.arm(&snapshot, now);
        match self.inner.system_clipboard.write_snapshot(snapshot) {
            Ok(()) => Ok(()),
            Err(error) => {
                suppression.disarm(guard_id);
                Err(error)
            }
        }
    }

    fn should_suppress_watcher_snapshot(&self, snapshot: &SystemClipboardSnapshot) -> bool {
        lock_unpoisoned(&self.inner.echo_suppression).consume(snapshot, Instant::now())
    }
}

impl SystemClipboard for DesktopClipboardProfileHandle {
    fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
        if let Some(snapshot) = lock_unpoisoned(&self.pending_snapshot).take() {
            return Ok(snapshot);
        }
        self.hub.inner.system_clipboard.read_snapshot()
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        self.hub.write_snapshot(snapshot)
    }
}

/// Exact snapshots emitted by the hub's unique physical watcher.
pub struct DesktopClipboardHubChangeStream {
    hub: DesktopClipboardHub,
    running: Option<RunningDesktopClipboardHubChanges>,
}

struct RunningDesktopClipboardHubChanges {
    receiver: tokio::sync::mpsc::Receiver<PlatformEvent>,
    shutdown: ShutdownTx,
    join: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl DesktopClipboardHubChangeStream {
    fn start_if_needed(&mut self) -> WiringResult<()> {
        if self.running.is_some() {
            return Ok(());
        }
        let event_loop = (self.hub.inner.event_loop_factory)()
            .map_err(|error| WiringError::ClipboardInit(error.to_string()))?;
        let (sender, receiver) = tokio::sync::mpsc::channel(64);
        let watcher = ClipboardWatcher::new(Arc::clone(&self.hub.inner.system_clipboard), sender);
        let (shutdown, shutdown_receiver) = shutdown_channel();
        let join = tokio::task::spawn_blocking(move || event_loop.run(watcher, shutdown_receiver));
        self.running = Some(RunningDesktopClipboardHubChanges {
            receiver,
            shutdown,
            join,
        });
        Ok(())
    }

    /// Wait for the next real user clipboard change.
    ///
    /// Programmatic-write echoes and empty snapshots are consumed internally;
    /// returned snapshots are the exact bytes captured by the watcher.
    pub async fn next(&mut self) -> WiringResult<Option<SystemClipboardSnapshot>> {
        self.start_if_needed()?;
        loop {
            let event = match self.running.as_mut() {
                Some(running) => running.receiver.recv().await,
                None => return Ok(None),
            };
            match event {
                Some(PlatformEvent::ClipboardChanged { snapshot }) if snapshot.is_empty() => {}
                Some(PlatformEvent::ClipboardChanged { snapshot })
                    if self.hub.should_suppress_watcher_snapshot(&snapshot) => {}
                Some(PlatformEvent::ClipboardChanged { snapshot }) => return Ok(Some(snapshot)),
                None => return Ok(None),
            }
        }
    }

    pub async fn shutdown(&mut self) -> WiringResult<()> {
        let Some(running) = self.running.take() else {
            return Ok(());
        };
        running.shutdown.signal();
        match tokio::time::timeout(WATCHER_SHUTDOWN_TIMEOUT, running.join).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => Err(WiringError::ClipboardInit(error.to_string())),
            Ok(Err(error)) => Err(WiringError::ClipboardInit(error.to_string())),
            Err(_) => Err(WiringError::ClipboardInit(
                "desktop clipboard hub watcher shutdown timed out".into(),
            )),
        }
    }
}

impl Drop for DesktopClipboardHubChangeStream {
    fn drop(&mut self) {
        if let Some(running) = self.running.as_ref() {
            running.shutdown.signal();
        }
    }
}

#[derive(Default)]
struct EchoSuppression {
    next_id: u64,
    guards: VecDeque<EchoGuard>,
}

struct EchoGuard {
    id: u64,
    content_key: Option<String>,
    content_class: Option<String>,
    fallback_until: Instant,
    expires_at: Instant,
}

impl EchoSuppression {
    fn arm(&mut self, snapshot: &SystemClipboardSnapshot, now: Instant) -> u64 {
        self.prune(now);
        self.next_id = self.next_id.wrapping_add(1);
        let content_key = snapshot.meaningful_origin_key();
        let content_class = content_key
            .as_deref()
            .and_then(content_class)
            .map(str::to_owned);
        self.guards.push_back(EchoGuard {
            id: self.next_id,
            content_key,
            content_class,
            fallback_until: now + NEXT_CHANGE_FALLBACK_WINDOW,
            expires_at: now + ECHO_GUARD_TTL,
        });
        self.next_id
    }

    fn disarm(&mut self, id: u64) {
        if let Some(position) = self.guards.iter().position(|guard| guard.id == id) {
            self.guards.remove(position);
        }
    }

    fn consume(&mut self, snapshot: &SystemClipboardSnapshot, now: Instant) -> bool {
        self.prune(now);
        let observed_key = snapshot.meaningful_origin_key();
        if let Some(position) = self
            .guards
            .iter()
            .position(|guard| guard.content_key.is_some() && guard.content_key == observed_key)
        {
            self.guards.remove(position);
            return true;
        }

        let observed_class = observed_key.as_deref().and_then(content_class);
        if let Some(position) = self.guards.iter().position(|guard| {
            now <= guard.fallback_until
                && (guard.content_key.is_none()
                    || (allows_next_change_fallback(guard.content_class.as_deref())
                        && (observed_key.is_none()
                            || guard.content_class.as_deref() == observed_class)))
        }) {
            self.guards.remove(position);
            return true;
        }
        false
    }

    fn prune(&mut self, now: Instant) {
        self.guards.retain(|guard| now <= guard.expires_at);
    }
}

fn content_class(key: &str) -> Option<&str> {
    key.split_once(':').map(|(class, _)| class)
}

fn allows_next_change_fallback(content_class: Option<&str>) -> bool {
    matches!(content_class, Some("image" | "files"))
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;

    use uc_platform::clipboard::watcher::ClipboardWatcher;
    use uc_platform::clipboard::{
        FormatId, MimeType, ObservedClipboardRepresentation, PlatformClipboardEventLoop,
        RepresentationId, ShutdownRx, SystemClipboard, SystemClipboardSnapshot,
    };

    use super::*;

    fn text_snapshot(ts_ms: i64, text: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".into())),
                text.as_bytes().to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        }
    }

    fn image_snapshot(ts_ms: i64, bytes: &[u8]) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("image"),
                Some(MimeType("image/png".into())),
                bytes.to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        }
    }

    fn snapshot_text(snapshot: &SystemClipboardSnapshot) -> &[u8] {
        snapshot.representations[0].expect_inline_bytes()
    }

    struct ScriptedEventLoop {
        runs: Arc<AtomicUsize>,
        events: Vec<SystemClipboardSnapshot>,
    }

    impl PlatformClipboardEventLoop for ScriptedEventLoop {
        fn run(
            self: Box<Self>,
            mut handler: ClipboardWatcher,
            shutdown_rx: ShutdownRx,
        ) -> anyhow::Result<()> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            for snapshot in self.events {
                handler.notify_with_snapshot(snapshot);
            }
            shutdown_rx.wait();
            Ok(())
        }
    }

    #[derive(Default)]
    struct StatefulClipboard {
        current: Mutex<Option<SystemClipboardSnapshot>>,
        writes: Mutex<Vec<SystemClipboardSnapshot>>,
        fail_next_write: AtomicBool,
    }

    impl StatefulClipboard {
        fn with_snapshot(snapshot: SystemClipboardSnapshot) -> Self {
            Self {
                current: Mutex::new(Some(snapshot)),
                ..Self::default()
            }
        }
    }

    impl SystemClipboard for StatefulClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            self.current
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .ok_or_else(|| anyhow::anyhow!("clipboard is empty"))
        }

        fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            if self.fail_next_write.swap(false, Ordering::SeqCst) {
                anyhow::bail!("injected write failure");
            }
            *self
                .current
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(snapshot.clone());
            self.writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(snapshot);
            Ok(())
        }
    }

    fn scripted_hub(
        clipboard: Arc<dyn SystemClipboard>,
        runs: Arc<AtomicUsize>,
        events: Vec<SystemClipboardSnapshot>,
    ) -> DesktopClipboardHub {
        DesktopClipboardHub::from_parts(
            clipboard,
            true,
            Arc::new(move || {
                Ok(Box::new(ScriptedEventLoop {
                    runs: Arc::clone(&runs),
                    events: events.clone(),
                }))
            }),
        )
    }

    #[tokio::test]
    async fn two_profiles_can_start_only_one_physical_watcher() {
        let runs = Arc::new(AtomicUsize::new(0));
        let local = text_snapshot(1, "local");
        let clipboard: Arc<dyn SystemClipboard> =
            Arc::new(StatefulClipboard::with_snapshot(local.clone()));
        let hub = scripted_hub(clipboard, Arc::clone(&runs), vec![local]);
        let _profile_a = hub.profile_handle();
        let _profile_b = hub.profile_handle();

        let mut changes = hub.take_change_stream().unwrap().unwrap();
        assert!(hub.take_change_stream().unwrap().is_none());
        assert_eq!(runs.load(Ordering::SeqCst), 0);

        let observed = changes.next().await.unwrap().unwrap();
        assert_eq!(snapshot_text(&observed), b"local");
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        changes.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn programmatic_write_echo_is_suppressed_without_eating_next_user_copy() {
        let runs = Arc::new(AtomicUsize::new(0));
        let echo = text_snapshot(10, "remote from B");
        let user_copy = text_snapshot(11, "next real copy");
        let clipboard = Arc::new(StatefulClipboard::with_snapshot(text_snapshot(0, "old")));
        let hub = scripted_hub(
            clipboard.clone(),
            runs,
            vec![echo.clone(), user_copy.clone()],
        );
        let _profile_a = hub.profile_handle();
        let profile_b = hub.profile_handle();

        profile_b.write_snapshot(echo).unwrap();
        let mut changes = hub.take_change_stream().unwrap().unwrap();

        let observed = changes.next().await.unwrap().unwrap();
        assert_eq!(snapshot_text(&observed), b"next real copy");
        assert_eq!(clipboard.writes.lock().unwrap().len(), 1);
        changes.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn missing_text_echo_does_not_eat_a_different_user_copy() {
        let remote_write = text_snapshot(12, "remote text whose echo was deduped");
        let user_copy = text_snapshot(13, "different real user copy");
        let clipboard = Arc::new(StatefulClipboard::with_snapshot(text_snapshot(0, "old")));
        let hub = scripted_hub(clipboard, Arc::new(AtomicUsize::new(0)), vec![user_copy]);
        let profile = hub.profile_handle();

        profile.write_snapshot(remote_write).unwrap();
        let mut changes = hub.take_change_stream().unwrap().unwrap();

        let observed = tokio::time::timeout(Duration::from_secs(1), changes.next())
            .await
            .expect("a different user text copy must not be swallowed")
            .unwrap()
            .unwrap();
        assert_eq!(snapshot_text(&observed), b"different real user copy");
        changes.shutdown().await.unwrap();
    }

    #[test]
    fn reencoded_image_uses_one_next_change_fallback_only() {
        let now = Instant::now();
        let mut suppression = EchoSuppression::default();
        suppression.arm(&image_snapshot(1, b"programmatic png bytes"), now);

        assert!(suppression.consume(
            &image_snapshot(2, b"same image reencoded by Windows"),
            now + Duration::from_millis(100)
        ));
        assert!(!suppression.consume(
            &image_snapshot(3, b"later real image copy"),
            now + Duration::from_millis(200)
        ));
    }

    #[test]
    fn expired_echo_guard_never_suppresses_a_later_matching_copy() {
        let now = Instant::now();
        let snapshot = text_snapshot(1, "same content later");
        let mut suppression = EchoSuppression::default();
        suppression.arm(&snapshot, now);

        assert!(!suppression.consume(&snapshot, now + ECHO_GUARD_TTL + Duration::from_millis(1)));
    }

    #[tokio::test]
    async fn failed_write_removes_echo_guard() {
        let failed = text_snapshot(20, "failed write content");
        let clipboard = Arc::new(StatefulClipboard::with_snapshot(text_snapshot(0, "old")));
        clipboard.fail_next_write.store(true, Ordering::SeqCst);
        let hub = scripted_hub(
            clipboard.clone(),
            Arc::new(AtomicUsize::new(0)),
            vec![failed.clone()],
        );
        let profile = hub.profile_handle();

        assert!(profile.write_snapshot(failed).is_err());
        let mut changes = hub.take_change_stream().unwrap().unwrap();

        let observed = changes.next().await.unwrap().unwrap();
        assert_eq!(snapshot_text(&observed), b"failed write content");
        changes.shutdown().await.unwrap();
    }

    struct BlockingClipboard {
        current: Mutex<SystemClipboardSnapshot>,
        writes: Mutex<Vec<SystemClipboardSnapshot>>,
        active_writes: AtomicUsize,
        max_active_writes: AtomicUsize,
        first_entered: Mutex<Option<mpsc::Sender<()>>>,
        release_first: Mutex<mpsc::Receiver<()>>,
    }

    impl SystemClipboard for BlockingClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(self
                .current
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone())
        }

        fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            let active = self.active_writes.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active_writes.fetch_max(active, Ordering::SeqCst);
            if let Some(entered) = self
                .first_entered
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                entered.send(()).unwrap();
                self.release_first
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .recv()
                    .unwrap();
            }
            *self
                .current
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot.clone();
            self.writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(snapshot);
            self.active_writes.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn concurrent_profile_writes_are_serialized_and_last_completed_wins() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let clipboard = Arc::new(BlockingClipboard {
            current: Mutex::new(text_snapshot(0, "old")),
            writes: Mutex::new(Vec::new()),
            active_writes: AtomicUsize::new(0),
            max_active_writes: AtomicUsize::new(0),
            first_entered: Mutex::new(Some(entered_tx)),
            release_first: Mutex::new(release_rx),
        });
        let hub = DesktopClipboardHub::from_parts(
            clipboard.clone(),
            false,
            Arc::new(|| anyhow::bail!("watcher must not start")),
        );
        let profile_a = hub.profile_handle();
        let profile_b = hub.profile_handle();
        let first = text_snapshot(30, "A");
        let second = text_snapshot(31, "B");

        let first_thread = thread::spawn(move || profile_a.write_snapshot(first).unwrap());
        entered_rx.recv().unwrap();
        let second_thread = thread::spawn(move || profile_b.write_snapshot(second).unwrap());
        release_tx.send(()).unwrap();
        first_thread.join().unwrap();
        second_thread.join().unwrap();

        assert_eq!(clipboard.max_active_writes.load(Ordering::SeqCst), 1);
        let writes = clipboard.writes.lock().unwrap();
        assert_eq!(snapshot_text(&writes[0]), b"A");
        assert_eq!(snapshot_text(&writes[1]), b"B");
        assert_eq!(snapshot_text(&clipboard.read_snapshot().unwrap()), b"B");
    }

    #[test]
    fn staged_snapshot_is_read_exactly_by_the_selected_profile() {
        let fresh = text_snapshot(41, "fresh OS value");
        let staged = text_snapshot(40, "event-time exact value");
        let clipboard: Arc<dyn SystemClipboard> = Arc::new(StatefulClipboard::with_snapshot(fresh));
        let hub = DesktopClipboardHub::from_parts(
            clipboard,
            false,
            Arc::new(|| anyhow::bail!("watcher must not start")),
        );
        let profile_a = hub.profile_handle();
        let profile_b = hub.profile_handle();

        hub.stage_snapshot(&profile_a, staged).unwrap();

        assert_eq!(
            snapshot_text(&profile_a.read_snapshot().unwrap()),
            b"event-time exact value"
        );
        assert_eq!(
            snapshot_text(&profile_b.read_snapshot().unwrap()),
            b"fresh OS value"
        );
    }
}
