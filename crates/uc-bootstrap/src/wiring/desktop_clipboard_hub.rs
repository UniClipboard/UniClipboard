//! Shared Windows desktop clipboard hub for multi-space Engine hosts.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use uc_platform::clipboard::watcher::{ClipboardWatcher, PlatformEvent};
use uc_platform::clipboard::{
    build_event_loop, shutdown_channel, ClipboardChangeToken, PlatformClipboardEventLoop,
    ShutdownTx, SystemClipboard, SystemClipboardSnapshot,
};

use crate::layer::platform::{create_desktop_system_clipboard, SystemClipboardWiring};
use crate::wiring::error::{WiringError, WiringResult};

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
    causality_gate: Mutex<()>,
    echo_suppression: Mutex<EchoSuppression>,
    event_loop_factory: EventLoopFactory,
    watcher_shutdown_timeout: Duration,
    next_stage_id: AtomicU64,
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
    pending_snapshot: Arc<Mutex<Option<StagedSnapshot>>>,
}

struct StagedSnapshot {
    id: u64,
    snapshot: SystemClipboardSnapshot,
}

/// Transaction guard for one exact event-time snapshot staged to a profile.
///
/// Dropping or rolling back an uncompleted guard clears only its own stage. A
/// successful clipboard read consumes the stage, after which `complete()`
/// confirms the transaction.
pub struct DesktopClipboardStageGuard {
    hub: DesktopClipboardHub,
    profile: DesktopClipboardProfileHandle,
    stage_id: u64,
    active: bool,
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
        Self::from_parts_with_shutdown_timeout(
            system_clipboard,
            changes_enabled,
            event_loop_factory,
            WATCHER_SHUTDOWN_TIMEOUT,
        )
    }

    fn from_parts_with_shutdown_timeout(
        system_clipboard: Arc<dyn SystemClipboard>,
        changes_enabled: bool,
        event_loop_factory: EventLoopFactory,
        watcher_shutdown_timeout: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(DesktopClipboardHubInner {
                system_clipboard,
                changes_enabled,
                watcher_taken: AtomicBool::new(false),
                causality_gate: Mutex::new(()),
                echo_suppression: Mutex::new(EchoSuppression::default()),
                event_loop_factory,
                watcher_shutdown_timeout,
                next_stage_id: AtomicU64::new(0),
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
    ) -> WiringResult<DesktopClipboardStageGuard> {
        self.ensure_profile_belongs_to_hub(profile)?;
        let mut pending = lock_unpoisoned(&profile.pending_snapshot);
        if pending.is_some() {
            return Err(WiringError::ClipboardInit(
                "clipboard profile already has an unconsumed staged snapshot".into(),
            ));
        }
        let stage_id = self.inner.next_stage_id.fetch_add(1, Ordering::Relaxed);
        *pending = Some(StagedSnapshot {
            id: stage_id,
            snapshot,
        });
        drop(pending);
        Ok(DesktopClipboardStageGuard {
            hub: self.clone(),
            profile: profile.clone(),
            stage_id,
            active: true,
        })
    }

    /// Explicitly clear a profile's pending stage after an aborted actor step.
    pub fn clear_staged_snapshot(
        &self,
        profile: &DesktopClipboardProfileHandle,
    ) -> WiringResult<bool> {
        self.ensure_profile_belongs_to_hub(profile)?;
        Ok(lock_unpoisoned(&profile.pending_snapshot).take().is_some())
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
            lease: Some(DesktopClipboardWatcherLease {
                inner: Arc::clone(&self.inner),
            }),
        }))
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        // This gate orders writes against watcher-event matching. The smaller
        // echo-state mutex is deliberately acquired only after the OS write.
        let _causality = lock_unpoisoned(&self.inner.causality_gate);
        let receipt = self
            .inner
            .system_clipboard
            .write_snapshot_with_receipt(snapshot)?;
        if let Some(token) = receipt.change_token {
            lock_unpoisoned(&self.inner.echo_suppression).arm(token);
        }
        Ok(())
    }

    fn should_suppress_watcher_event(&self, token: Option<ClipboardChangeToken>) -> bool {
        let _causality = lock_unpoisoned(&self.inner.causality_gate);
        lock_unpoisoned(&self.inner.echo_suppression).consume(token)
    }

    fn ensure_profile_belongs_to_hub(
        &self,
        profile: &DesktopClipboardProfileHandle,
    ) -> WiringResult<()> {
        if Arc::ptr_eq(&self.inner, &profile.hub.inner) {
            Ok(())
        } else {
            Err(WiringError::ClipboardInit(
                "clipboard profile handle belongs to a different hub".into(),
            ))
        }
    }

    fn clear_stage_if_matches(&self, profile: &DesktopClipboardProfileHandle, stage_id: u64) {
        let mut pending = lock_unpoisoned(&profile.pending_snapshot);
        if pending.as_ref().is_some_and(|staged| staged.id == stage_id) {
            pending.take();
        }
    }
}

impl DesktopClipboardStageGuard {
    /// Confirm that the staged snapshot was consumed by the intended read.
    pub fn complete(mut self) -> WiringResult<()> {
        let still_pending = lock_unpoisoned(&self.profile.pending_snapshot)
            .as_ref()
            .is_some_and(|staged| staged.id == self.stage_id);
        if still_pending {
            return Err(WiringError::ClipboardInit(
                "staged clipboard snapshot was not consumed".into(),
            ));
        }
        self.active = false;
        Ok(())
    }

    /// Abort this stage and clear it if it has not already been consumed.
    pub fn rollback(mut self) {
        self.hub
            .clear_stage_if_matches(&self.profile, self.stage_id);
        self.active = false;
    }
}

impl Drop for DesktopClipboardStageGuard {
    fn drop(&mut self) {
        if self.active {
            self.hub
                .clear_stage_if_matches(&self.profile, self.stage_id);
        }
    }
}

impl SystemClipboard for DesktopClipboardProfileHandle {
    fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
        if let Some(staged) = lock_unpoisoned(&self.pending_snapshot).take() {
            return Ok(staged.snapshot);
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
    lease: Option<DesktopClipboardWatcherLease>,
}

struct DesktopClipboardWatcherLease {
    inner: Arc<DesktopClipboardHubInner>,
}

impl Drop for DesktopClipboardWatcherLease {
    fn drop(&mut self) {
        self.inner.watcher_taken.store(false, Ordering::SeqCst);
    }
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
        if self.lease.is_none() {
            return Err(WiringError::ClipboardInit(
                "desktop clipboard watcher lease is no longer owned".into(),
            ));
        }
        let event_loop = match (self.hub.inner.event_loop_factory)() {
            Ok(event_loop) => event_loop,
            Err(error) => {
                self.lease.take();
                return Err(WiringError::ClipboardInit(error.to_string()));
            }
        };
        let (sender, receiver) = tokio::sync::mpsc::channel(64);
        let watcher =
            ClipboardWatcher::new_passthrough(Arc::clone(&self.hub.inner.system_clipboard), sender);
        let (shutdown, shutdown_receiver) = shutdown_channel();
        let lease = self
            .lease
            .take()
            .expect("watcher lease checked before event-loop startup");
        let join = tokio::task::spawn_blocking(move || {
            let _lease = lease;
            event_loop.run(watcher, shutdown_receiver)
        });
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
                Some(PlatformEvent::ClipboardChanged { snapshot, .. }) if snapshot.is_empty() => {}
                Some(PlatformEvent::ClipboardChanged { change_token, .. })
                    if self.hub.should_suppress_watcher_event(change_token) => {}
                Some(PlatformEvent::ClipboardChanged { snapshot, .. }) => {
                    return Ok(Some(snapshot))
                }
                None => return Ok(None),
            }
        }
    }

    pub async fn shutdown(&mut self) -> WiringResult<()> {
        let Some(running) = self.running.as_mut() else {
            self.lease.take();
            return Ok(());
        };
        running.shutdown.signal();
        let (result, timed_out) =
            match tokio::time::timeout(self.hub.inner.watcher_shutdown_timeout, &mut running.join)
                .await
            {
                Ok(Ok(Ok(()))) => (Ok(()), false),
                Ok(Ok(Err(error))) => (Err(WiringError::ClipboardInit(error.to_string())), false),
                Ok(Err(error)) => (Err(WiringError::ClipboardInit(error.to_string())), false),
                Err(_) => (
                    Err(WiringError::ClipboardInit(
                        "desktop clipboard hub watcher shutdown timed out".into(),
                    )),
                    true,
                ),
            };
        if !timed_out {
            self.running.take();
        }
        result
    }
}

impl Drop for DesktopClipboardHubChangeStream {
    fn drop(&mut self) {
        if let Some(running) = self.running.as_ref() {
            // The detached blocking task owns the lease and releases it only
            // after the physical event loop actually exits.
            running.shutdown.signal();
        }
    }
}

#[derive(Default)]
struct EchoSuppression {
    pending_tokens: VecDeque<ClipboardChangeToken>,
    last_suppressed_token: Option<ClipboardChangeToken>,
}

impl EchoSuppression {
    fn arm(&mut self, token: ClipboardChangeToken) {
        if !self.pending_tokens.contains(&token) {
            self.pending_tokens.push_back(token);
        }
    }

    fn consume(&mut self, token: Option<ClipboardChangeToken>) -> bool {
        let Some(token) = token else {
            self.last_suppressed_token = None;
            return false;
        };
        // One physical write may queue more than one Windows notification,
        // all carrying the same sequence number. Consecutive repeats therefore
        // remain suppressed until a different sequence reaches this FIFO.
        if self.last_suppressed_token == Some(token) {
            return true;
        }
        if let Some(position) = self
            .pending_tokens
            .iter()
            .position(|candidate| *candidate == token)
        {
            self.pending_tokens.drain(..=position);
            self.last_suppressed_token = Some(token);
            return true;
        }

        self.last_suppressed_token = None;
        self.pending_tokens
            .retain(|candidate| windows_sequence_is_after(*candidate, token));
        false
    }
}

fn windows_sequence_is_after(
    candidate: ClipboardChangeToken,
    observed: ClipboardChangeToken,
) -> bool {
    let candidate = candidate.get() as u32;
    let observed = observed.get() as u32;
    let distance = candidate.wrapping_sub(observed);
    distance != 0 && distance < (1 << 31)
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;

    use uc_platform::clipboard::watcher::ClipboardWatcher;
    use uc_platform::clipboard::{
        ClipboardChangeToken, FormatId, MimeType, ObservedClipboardRepresentation,
        PlatformClipboardEventLoop, RepresentationId, ShutdownRx, SystemClipboard,
        SystemClipboardSnapshot,
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

    fn file_snapshot(ts_ms: i64, path: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("files"),
                Some(MimeType("text/uri-list".into())),
                path.as_bytes().to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        }
    }

    fn snapshot_text(snapshot: &SystemClipboardSnapshot) -> &[u8] {
        snapshot.representations[0].expect_inline_bytes()
    }

    fn token(value: u64) -> Option<ClipboardChangeToken> {
        Some(ClipboardChangeToken::new(value))
    }

    struct ScriptedEventLoop {
        runs: Arc<AtomicUsize>,
        events: Vec<(SystemClipboardSnapshot, Option<ClipboardChangeToken>)>,
    }

    impl PlatformClipboardEventLoop for ScriptedEventLoop {
        fn run(
            self: Box<Self>,
            mut handler: ClipboardWatcher,
            shutdown_rx: ShutdownRx,
        ) -> anyhow::Result<()> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            for (snapshot, change_token) in self.events {
                handler.notify_with_snapshot_and_token(snapshot, change_token);
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
        sequence: AtomicU64,
    }

    impl StatefulClipboard {
        fn with_snapshot(snapshot: SystemClipboardSnapshot) -> Self {
            Self {
                current: Mutex::new(Some(snapshot)),
                sequence: AtomicU64::new(100),
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
            self.sequence.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn change_token(&self) -> Option<ClipboardChangeToken> {
            token(self.sequence.load(Ordering::SeqCst))
        }
    }

    fn scripted_hub(
        clipboard: Arc<dyn SystemClipboard>,
        runs: Arc<AtomicUsize>,
        events: Vec<(SystemClipboardSnapshot, Option<ClipboardChangeToken>)>,
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
        let hub = scripted_hub(clipboard, Arc::clone(&runs), vec![(local, token(200))]);
        let _profile_a = hub.profile_handle();
        let _profile_b = hub.profile_handle();

        let mut changes = hub.take_change_stream().unwrap().unwrap();
        assert!(hub.take_change_stream().unwrap().is_none());
        assert_eq!(runs.load(Ordering::SeqCst), 0);

        let observed = changes.next().await.unwrap().unwrap();
        assert_eq!(snapshot_text(&observed), b"local");
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        changes.shutdown().await.unwrap();
        assert!(hub.take_change_stream().unwrap().is_some());
    }

    #[test]
    fn dropping_unstarted_watcher_lease_allows_reacquire() {
        let clipboard: Arc<dyn SystemClipboard> =
            Arc::new(StatefulClipboard::with_snapshot(text_snapshot(1, "local")));
        let hub = DesktopClipboardHub::from_parts(
            clipboard,
            true,
            Arc::new(|| anyhow::bail!("watcher must not start")),
        );

        let changes = hub.take_change_stream().unwrap().unwrap();
        drop(changes);

        assert!(hub.take_change_stream().unwrap().is_some());
    }

    #[tokio::test]
    async fn watcher_factory_failure_releases_lease_for_retry() {
        let clipboard: Arc<dyn SystemClipboard> =
            Arc::new(StatefulClipboard::with_snapshot(text_snapshot(1, "local")));
        let hub = DesktopClipboardHub::from_parts(
            clipboard,
            true,
            Arc::new(|| anyhow::bail!("injected factory failure")),
        );
        let mut changes = hub.take_change_stream().unwrap().unwrap();

        assert!(changes.next().await.is_err());
        assert!(hub.take_change_stream().unwrap().is_some());
    }

    struct DelayedShutdownEventLoop {
        release: mpsc::Receiver<()>,
    }

    impl PlatformClipboardEventLoop for DelayedShutdownEventLoop {
        fn run(
            self: Box<Self>,
            _handler: ClipboardWatcher,
            shutdown_rx: ShutdownRx,
        ) -> anyhow::Result<()> {
            shutdown_rx.wait();
            self.release.recv().unwrap();
            Ok(())
        }
    }

    #[tokio::test]
    async fn shutdown_timeout_retains_join_and_lease_for_retry() {
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(Some(release_rx)));
        let clipboard: Arc<dyn SystemClipboard> =
            Arc::new(StatefulClipboard::with_snapshot(text_snapshot(1, "local")));
        let hub = DesktopClipboardHub::from_parts_with_shutdown_timeout(
            clipboard,
            true,
            Arc::new(move || {
                let release = release_rx
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("event loop already built"))?;
                Ok(Box::new(DelayedShutdownEventLoop { release }))
            }),
            Duration::from_millis(20),
        );
        let mut changes = hub.take_change_stream().unwrap().unwrap();
        changes.start_if_needed().unwrap();

        assert!(changes.shutdown().await.is_err());
        assert!(hub.take_change_stream().unwrap().is_none());

        release_tx.send(()).unwrap();
        changes.shutdown().await.unwrap();
        assert!(hub.take_change_stream().unwrap().is_some());
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
            vec![
                (echo.clone(), token(101)),
                (echo.clone(), token(101)),
                (user_copy.clone(), token(102)),
            ],
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
        let hub = scripted_hub(
            clipboard,
            Arc::new(AtomicUsize::new(0)),
            vec![(user_copy, token(102))],
        );
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

    #[tokio::test]
    async fn only_matching_write_token_is_suppressed_for_repeated_identical_content() {
        let same = text_snapshot(12, "same bytes every time");
        let clipboard = Arc::new(StatefulClipboard::with_snapshot(text_snapshot(0, "old")));
        let hub = scripted_hub(
            clipboard,
            Arc::new(AtomicUsize::new(0)),
            vec![
                (same.clone(), token(99)),
                (same.clone(), token(101)),
                (same.clone(), token(102)),
            ],
        );
        let profile = hub.profile_handle();
        profile.write_snapshot(same).unwrap();
        let mut changes = hub.take_change_stream().unwrap().unwrap();

        let queued_before_write = changes.next().await.unwrap().unwrap();
        assert_eq!(
            snapshot_text(&queued_before_write),
            b"same bytes every time"
        );
        let copied_again = changes.next().await.unwrap().unwrap();
        assert_eq!(snapshot_text(&copied_again), b"same bytes every time");
        changes.shutdown().await.unwrap();
    }

    #[test]
    fn all_pending_write_tokens_remain_causally_suppressible() {
        let mut suppression = EchoSuppression::default();
        for value in 1..=256 {
            suppression.arm(ClipboardChangeToken::new(value));
        }

        for value in 1..=256 {
            assert!(suppression.consume(token(value)));
        }
        assert!(!suppression.consume(token(257)));
    }

    #[tokio::test]
    async fn missing_image_echo_never_suppresses_different_real_image() {
        let remote_write = image_snapshot(12, b"remote image");
        let user_copy = image_snapshot(13, b"different user image");
        let clipboard = Arc::new(StatefulClipboard::with_snapshot(text_snapshot(0, "old")));
        let hub = scripted_hub(
            clipboard,
            Arc::new(AtomicUsize::new(0)),
            vec![(user_copy, token(102))],
        );
        let profile = hub.profile_handle();
        profile.write_snapshot(remote_write).unwrap();
        let mut changes = hub.take_change_stream().unwrap().unwrap();

        let observed = tokio::time::timeout(Duration::from_secs(1), changes.next())
            .await
            .expect("different real image must not be swallowed")
            .unwrap()
            .unwrap();
        assert_eq!(snapshot_text(&observed), b"different user image");
        changes.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn missing_file_echo_never_suppresses_different_real_file_copy() {
        let remote_write = file_snapshot(14, "file:///C:/remote.txt");
        let user_copy = file_snapshot(15, "file:///C:/user.txt");
        let clipboard = Arc::new(StatefulClipboard::with_snapshot(text_snapshot(0, "old")));
        let hub = scripted_hub(
            clipboard,
            Arc::new(AtomicUsize::new(0)),
            vec![(user_copy, token(102))],
        );
        let profile = hub.profile_handle();
        profile.write_snapshot(remote_write).unwrap();
        let mut changes = hub.take_change_stream().unwrap().unwrap();

        let observed = tokio::time::timeout(Duration::from_secs(1), changes.next())
            .await
            .expect("different real file copy must not be swallowed")
            .unwrap()
            .unwrap();
        assert_eq!(snapshot_text(&observed), b"file:///C:/user.txt");
        changes.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn failed_write_removes_echo_guard() {
        let failed = text_snapshot(20, "failed write content");
        let clipboard = Arc::new(StatefulClipboard::with_snapshot(text_snapshot(0, "old")));
        clipboard.fail_next_write.store(true, Ordering::SeqCst);
        let hub = scripted_hub(
            clipboard.clone(),
            Arc::new(AtomicUsize::new(0)),
            vec![(failed.clone(), token(100))],
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

        let stage = hub.stage_snapshot(&profile_a, staged).unwrap();

        assert_eq!(
            snapshot_text(&profile_a.read_snapshot().unwrap()),
            b"event-time exact value"
        );
        assert_eq!(
            snapshot_text(&profile_b.read_snapshot().unwrap()),
            b"fresh OS value"
        );
        stage.complete().unwrap();
    }

    #[test]
    fn second_stage_fails_closed_instead_of_overwriting_unconsumed_snapshot() {
        let clipboard: Arc<dyn SystemClipboard> = Arc::new(StatefulClipboard::with_snapshot(
            text_snapshot(50, "physical"),
        ));
        let hub = DesktopClipboardHub::from_parts(
            clipboard,
            false,
            Arc::new(|| anyhow::bail!("watcher must not start")),
        );
        let profile = hub.profile_handle();

        let first = hub
            .stage_snapshot(&profile, text_snapshot(51, "first staged"))
            .unwrap();
        assert!(hub
            .stage_snapshot(&profile, text_snapshot(52, "must be rejected"))
            .is_err());

        assert_eq!(
            snapshot_text(&profile.read_snapshot().unwrap()),
            b"first staged"
        );
        first.complete().unwrap();
    }

    #[test]
    fn failed_stage_transaction_can_rollback_and_retry() {
        let clipboard: Arc<dyn SystemClipboard> = Arc::new(StatefulClipboard::with_snapshot(
            text_snapshot(60, "physical"),
        ));
        let hub = DesktopClipboardHub::from_parts(
            clipboard,
            false,
            Arc::new(|| anyhow::bail!("watcher must not start")),
        );
        let profile = hub.profile_handle();

        let failed = hub
            .stage_snapshot(&profile, text_snapshot(61, "failed observe"))
            .unwrap();
        failed.rollback();

        let retry = hub
            .stage_snapshot(&profile, text_snapshot(62, "retry exact"))
            .unwrap();
        assert_eq!(
            snapshot_text(&profile.read_snapshot().unwrap()),
            b"retry exact"
        );
        retry.complete().unwrap();
    }
}
