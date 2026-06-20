//! `RestoreBroadcastWorker` — debounced, gated outbound broadcast of local
//! history restores (issue #1017 PR4).
//!
//! A local history restore that advances the active-clipboard register offers
//! the activation here via the application-internal restore-broadcast channel.
//! This worker:
//!
//! 1. **Coalesces** rapid restores: it keeps only the latest offered request
//!    and emits a single broadcast after a quiet window (D7, ~300ms). A user
//!    clicking through several history entries quickly produces one announce
//!    of the final selection, not one per click.
//! 2. **Feature-gates** on `sync_on_restore` (default off): when disabled the
//!    register still advanced locally, but nothing is announced to peers.
//! 3. **Per-peer gates** through the shared fan-out: `send_enabled` ∧
//!    `send_content_types` (D2), identical to the inbound re-broadcast path.
//!
//! The gate is re-read from settings at emit time (not at offer time) so a
//! toggle taking effect between the restore and the debounced emit is honoured.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, instrument, warn};

use uc_core::ports::clipboard::ActiveClipboardDispatchPort;
use uc_core::ports::{PeerAddressRepositoryPort, PresencePort, SettingsPort};
use uc_core::MemberRepositoryPort;

use crate::clipboard_write::RestoreBroadcastRequest;

use super::super::send_gate::MemberSendGate;
use super::fanout::fan_out_active_state;

/// Debounce window for coalescing rapid restores into one broadcast (D7).
const RESTORE_BROADCAST_DEBOUNCE: Duration = Duration::from_millis(300);

/// Handle owning the spawned restore-broadcast worker. Drop or `abort()` to
/// stop it; the worker also exits on its own when every
/// [`RestoreBroadcastTrigger`](crate::clipboard_write::RestoreBroadcastTrigger)
/// sender is dropped.
pub struct RestoreBroadcastHandle {
    join: JoinHandle<()>,
}

impl RestoreBroadcastHandle {
    pub fn abort(&self) {
        self.join.abort();
    }
}

impl Drop for RestoreBroadcastHandle {
    fn drop(&mut self) {
        self.join.abort();
    }
}

/// Dependencies for the restore-broadcast worker.
pub(crate) struct RestoreBroadcastWorker {
    rx: UnboundedReceiver<RestoreBroadcastRequest>,
    settings: Arc<dyn SettingsPort>,
    dispatch: Arc<dyn ActiveClipboardDispatchPort>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    presence: Arc<dyn PresencePort>,
    send_gate: MemberSendGate,
}

impl RestoreBroadcastWorker {
    pub(crate) fn new(
        rx: UnboundedReceiver<RestoreBroadcastRequest>,
        settings: Arc<dyn SettingsPort>,
        dispatch: Arc<dyn ActiveClipboardDispatchPort>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        presence: Arc<dyn PresencePort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
    ) -> Self {
        Self {
            rx,
            settings,
            dispatch,
            peer_addr_repo,
            presence,
            send_gate: MemberSendGate::new(member_repo),
        }
    }

    /// Spawn the worker loop.
    pub(crate) fn spawn(self) -> RestoreBroadcastHandle {
        let join = tokio::spawn(self.run());
        RestoreBroadcastHandle { join }
    }

    #[instrument(name = "active_state.restore_broadcast_loop", skip_all)]
    async fn run(mut self) {
        loop {
            // Block until the next offer (or all senders drop → exit).
            let mut latest = match self.rx.recv().await {
                Some(req) => req,
                None => {
                    debug!("restore broadcast worker: all senders dropped; exiting");
                    return;
                }
            };

            // Debounce: keep draining for the quiet window, replacing `latest`
            // with each newer offer. A fresh offer that arrives during the
            // window restarts it, so the timer measures quiet time, not a fixed
            // batch interval.
            loop {
                tokio::select! {
                    biased;
                    maybe = self.rx.recv() => match maybe {
                        Some(req) => {
                            latest = req;
                            // loop again — window restarts
                        }
                        None => {
                            // Senders gone mid-window: emit what we have, then
                            // the outer loop's recv will see the close and exit.
                            break;
                        }
                    },
                    _ = sleep(RESTORE_BROADCAST_DEBOUNCE) => break,
                }
            }

            self.emit(latest).await;
        }
    }

    /// Feature-gate then fan out one coalesced activation.
    async fn emit(&self, request: RestoreBroadcastRequest) {
        // Re-read the toggle at emit time so a setting change between the
        // restore and this debounced emit is respected.
        let sync_on_restore = match self.settings.load().await {
            Ok(settings) => settings.sync.sync_on_restore,
            Err(err) => {
                // Fail closed: if we can't confirm the user opted in, don't
                // announce. A restore that should have broadcast is recovered
                // by the next restore or a peer-online resync.
                warn!(error = %err, "restore broadcast skipped: settings load failed");
                return;
            }
        };
        if !sync_on_restore {
            debug!(
                snapshot_hash = %request.state.snapshot_hash,
                "restore broadcast skipped: sync_on_restore disabled"
            );
            return;
        }

        fan_out_active_state(
            &self.dispatch,
            &self.peer_addr_repo,
            &self.presence,
            &self.send_gate,
            &request.state,
            &request.categories,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::Utc;
    use tokio::sync::mpsc::unbounded_channel;

    use uc_core::clipboard::{
        ActiveClipboardState, ClipboardContentCategory, ClipboardContentCategorySet,
    };
    use uc_core::ids::{DeviceId, EntryId};
    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::ports::clipboard::ActiveClipboardDispatchError;
    use uc_core::ports::{
        PeerAddressError, PeerAddressRecord, PresenceError, PresenceEvent, PresencePort,
        ReachabilityState,
    };
    use uc_core::settings::model::Settings;
    use uc_core::MemberSyncPreferences;

    /// Presence fake reporting every device with a fixed reachability. The
    /// broadcast tests want their roster peer reachable, so `Online` is the
    /// default; `Offline` exercises the fan-out skip.
    struct StaticPresence(ReachabilityState);
    #[async_trait]
    impl PresencePort for StaticPresence {
        async fn ensure_reachable(
            &self,
            _device: &DeviceId,
        ) -> Result<ReachabilityState, PresenceError> {
            Ok(self.0)
        }
        async fn current_state(&self, _device: &DeviceId) -> ReachabilityState {
            self.0
        }
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PresenceEvent> {
            tokio::sync::broadcast::channel(1).1
        }
    }

    // ---- spies / fakes ------------------------------------------------------

    struct FixedSettings {
        sync_on_restore: bool,
    }
    #[async_trait]
    impl SettingsPort for FixedSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            let mut s = Settings::default();
            s.sync.sync_on_restore = self.sync_on_restore;
            Ok(s)
        }
        async fn save(&self, _settings: &Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Records the content hashes dispatched, in order.
    #[derive(Default)]
    struct DispatchSpy {
        sent: Mutex<Vec<(String, String)>>, // (target, snapshot_hash)
    }
    #[async_trait]
    impl ActiveClipboardDispatchPort for DispatchSpy {
        async fn dispatch(
            &self,
            target: &DeviceId,
            state: &ActiveClipboardState,
        ) -> Result<(), ActiveClipboardDispatchError> {
            self.sent
                .lock()
                .unwrap()
                .push((target.as_str().to_string(), state.snapshot_hash.clone()));
            Ok(())
        }
    }

    /// One peer in the roster.
    struct OnePeerAddrRepo {
        device: DeviceId,
    }
    #[async_trait]
    impl PeerAddressRepositoryPort for OnePeerAddrRepo {
        async fn get(
            &self,
            _device: &DeviceId,
        ) -> Result<Option<PeerAddressRecord>, PeerAddressError> {
            Ok(None)
        }
        async fn upsert(&self, _record: &PeerAddressRecord) -> Result<(), PeerAddressError> {
            Ok(())
        }
        async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError> {
            Ok(vec![PeerAddressRecord {
                device_id: self.device.clone(),
                addr_blob: vec![],
                observed_at: Utc::now(),
            }])
        }
        async fn remove(&self, _device: &DeviceId) -> Result<(), PeerAddressError> {
            Ok(())
        }
    }

    struct AllowAllMembers;
    #[async_trait]
    impl MemberRepositoryPort for AllowAllMembers {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(Some(SpaceMember {
                device_id: device_id.clone(),
                device_name: "peer".to_string(),
                identity_fingerprint: uc_core::security::IdentityFingerprint::from_raw_string(
                    "0123456789abcdef",
                )
                .expect("valid test fingerprint"),
                joined_at: Utc::now(),
                sync_preferences: MemberSyncPreferences::default(),
            }))
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(vec![])
        }
        async fn save(&self, _member: &SpaceMember) -> Result<(), MembershipError> {
            Ok(())
        }
        async fn remove(&self, _device_id: &DeviceId) -> Result<bool, MembershipError> {
            Ok(false)
        }
    }

    fn request(snapshot_hash: &str) -> RestoreBroadcastRequest {
        let mut categories = ClipboardContentCategorySet::empty();
        categories.insert(ClipboardContentCategory::Text);
        RestoreBroadcastRequest {
            state: ActiveClipboardState::new(
                snapshot_hash,
                EntryId::new(),
                1_000,
                DeviceId::new("self"),
            ),
            categories,
        }
    }

    fn build_worker(
        sync_on_restore: bool,
        presence: ReachabilityState,
    ) -> (
        RestoreBroadcastWorker,
        tokio::sync::mpsc::UnboundedSender<RestoreBroadcastRequest>,
        Arc<DispatchSpy>,
    ) {
        let (tx, rx) = unbounded_channel();
        let dispatch = Arc::new(DispatchSpy::default());
        let worker = RestoreBroadcastWorker::new(
            rx,
            Arc::new(FixedSettings { sync_on_restore }),
            Arc::clone(&dispatch) as Arc<dyn ActiveClipboardDispatchPort>,
            Arc::new(OnePeerAddrRepo {
                device: DeviceId::new("peer-1"),
            }),
            Arc::new(StaticPresence(presence)),
            Arc::new(AllowAllMembers),
        );
        (worker, tx, dispatch)
    }

    #[tokio::test]
    async fn sync_on_restore_disabled_does_not_broadcast() {
        let (worker, tx, dispatch) = build_worker(false, ReachabilityState::Online);
        let handle = worker.spawn();

        tx.send(request("blake3v1:aa")).unwrap();
        // Give the worker past the debounce window + a slice of slack.
        tokio::time::sleep(RESTORE_BROADCAST_DEBOUNCE + Duration::from_millis(80)).await;

        assert!(
            dispatch.sent.lock().unwrap().is_empty(),
            "no broadcast must happen while sync_on_restore is off"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn enabled_broadcasts_to_allowed_peer() {
        let (worker, tx, dispatch) = build_worker(true, ReachabilityState::Online);
        let handle = worker.spawn();

        tx.send(request("blake3v1:aa")).unwrap();
        tokio::time::sleep(RESTORE_BROADCAST_DEBOUNCE + Duration::from_millis(80)).await;

        let sent = dispatch.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "exactly one peer is in the roster");
        assert_eq!(sent[0], ("peer-1".to_string(), "blake3v1:aa".to_string()));
        drop(sent);
        handle.abort();
    }

    #[tokio::test]
    async fn rapid_restores_coalesce_to_latest() {
        let (worker, tx, dispatch) = build_worker(true, ReachabilityState::Online);
        let handle = worker.spawn();

        // Three offers inside the debounce window — only the last should win.
        tx.send(request("blake3v1:aa")).unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        tx.send(request("blake3v1:bb")).unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        tx.send(request("blake3v1:cc")).unwrap();

        tokio::time::sleep(RESTORE_BROADCAST_DEBOUNCE + Duration::from_millis(120)).await;

        let sent = dispatch.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "rapid restores coalesce into one broadcast");
        assert_eq!(
            sent[0].1, "blake3v1:cc",
            "the coalesced broadcast carries the latest restore"
        );
        drop(sent);
        handle.abort();
    }

    #[tokio::test]
    async fn known_offline_peer_is_skipped() {
        // The only roster peer is already known offline → the fan-out skips it
        // without dialing, so nothing is dispatched even with sync_on_restore on.
        let (worker, tx, dispatch) = build_worker(true, ReachabilityState::Offline);
        let handle = worker.spawn();

        tx.send(request("blake3v1:aa")).unwrap();
        tokio::time::sleep(RESTORE_BROADCAST_DEBOUNCE + Duration::from_millis(80)).await;

        assert!(
            dispatch.sent.lock().unwrap().is_empty(),
            "a peer known offline must be skipped, not dialed"
        );
        handle.abort();
    }
}
