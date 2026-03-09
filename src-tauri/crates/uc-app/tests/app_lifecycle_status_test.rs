use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use uc_app::usecases::clipboard::ClipboardIntegrationMode;
use uc_app::usecases::{
    AppLifecycleCoordinator, AppLifecycleCoordinatorDeps, LifecycleEvent, LifecycleEventEmitter,
    LifecycleState, LifecycleStatusPort, SessionReadyEmitter, StartNetworkAfterUnlock,
};
use uc_core::ports::network_control::NetworkControlPort;
use uc_platform::ports::{WatcherControlError, WatcherControlPort};
use uc_platform::usecases::StartClipboardWatcher;

// ---------------------------------------------------------------------------
// Mock: WatcherControlPort (configurable failure)
// ---------------------------------------------------------------------------

struct MockWatcherControl {
    calls: Arc<AtomicUsize>,
    should_fail: bool,
}

#[async_trait]
impl WatcherControlPort for MockWatcherControl {
    async fn start_watcher(&self) -> Result<(), WatcherControlError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.should_fail {
            return Err(WatcherControlError::StartFailed(
                "watcher mock failure".to_string(),
            ));
        }
        Ok(())
    }

    async fn stop_watcher(&self) -> Result<(), WatcherControlError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock: NetworkControlPort (configurable failure)
// ---------------------------------------------------------------------------

struct MockNetworkControl {
    calls: Arc<AtomicUsize>,
    error_message: Option<String>,
}

#[async_trait]
impl NetworkControlPort for MockNetworkControl {
    async fn start_network(&self) -> anyhow::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(message) = &self.error_message {
            return Err(anyhow::anyhow!(message.clone()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock: SessionReadyEmitter
// ---------------------------------------------------------------------------

struct MockSessionReadyEmitter {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl SessionReadyEmitter for MockSessionReadyEmitter {
    async fn emit_ready(&self) -> anyhow::Result<()> {
        let mut guard = self.events.lock().await;
        guard.push("ready".to_string());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock: LifecycleStatusPort
// ---------------------------------------------------------------------------

struct MockLifecycleStatus {
    states: Arc<Mutex<Vec<LifecycleState>>>,
}

#[async_trait]
impl LifecycleStatusPort for MockLifecycleStatus {
    async fn set_state(&self, state: LifecycleState) -> anyhow::Result<()> {
        let mut guard = self.states.lock().await;
        guard.push(state);
        Ok(())
    }

    async fn get_state(&self) -> LifecycleState {
        let guard = self.states.lock().await;
        guard.last().cloned().unwrap_or(LifecycleState::Idle)
    }
}

// ---------------------------------------------------------------------------
// Mock: LifecycleEventEmitter
// ---------------------------------------------------------------------------

struct MockLifecycleEventEmitter {
    events: Arc<Mutex<Vec<LifecycleEvent>>>,
}

#[async_trait]
impl LifecycleEventEmitter for MockLifecycleEventEmitter {
    async fn emit_lifecycle_event(&self, event: LifecycleEvent) -> anyhow::Result<()> {
        let mut guard = self.events.lock().await;
        guard.push(event);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

struct TestMocks {
    watcher_calls: Arc<AtomicUsize>,
    network_calls: Arc<AtomicUsize>,
    session_events: Arc<Mutex<Vec<String>>>,
    status_states: Arc<Mutex<Vec<LifecycleState>>>,
    lifecycle_events: Arc<Mutex<Vec<LifecycleEvent>>>,
}

fn build_coordinator(
    watcher_fails: bool,
    network_fails: bool,
) -> (TestMocks, AppLifecycleCoordinator) {
    let network_error = if network_fails {
        Some("network mock failure")
    } else {
        None
    };
    build_coordinator_with_network_error(watcher_fails, network_error)
}

fn build_coordinator_with_network_error(
    watcher_fails: bool,
    network_error: Option<&str>,
) -> (TestMocks, AppLifecycleCoordinator) {
    let watcher_calls = Arc::new(AtomicUsize::new(0));
    let network_calls = Arc::new(AtomicUsize::new(0));
    let session_events = Arc::new(Mutex::new(Vec::new()));
    let status_states = Arc::new(Mutex::new(Vec::new()));
    let lifecycle_events = Arc::new(Mutex::new(Vec::new()));

    let watcher_control = Arc::new(MockWatcherControl {
        calls: watcher_calls.clone(),
        should_fail: watcher_fails,
    });
    let watcher = Arc::new(StartClipboardWatcher::new(
        watcher_control,
        ClipboardIntegrationMode::Full,
    ));

    let network_control = Arc::new(MockNetworkControl {
        calls: network_calls.clone(),
        error_message: network_error.map(ToString::to_string),
    });
    let network = Arc::new(StartNetworkAfterUnlock::new(network_control));

    let emitter = Arc::new(MockSessionReadyEmitter {
        events: session_events.clone(),
    }) as Arc<dyn SessionReadyEmitter>;

    let status = Arc::new(MockLifecycleStatus {
        states: status_states.clone(),
    }) as Arc<dyn LifecycleStatusPort>;

    let lifecycle_emitter = Arc::new(MockLifecycleEventEmitter {
        events: lifecycle_events.clone(),
    }) as Arc<dyn LifecycleEventEmitter>;

    let coordinator = AppLifecycleCoordinator::from_deps(AppLifecycleCoordinatorDeps {
        watcher,
        network,
        announcer: None,
        emitter,
        status,
        lifecycle_emitter,
    });

    (
        TestMocks {
            watcher_calls,
            network_calls,
            session_events,
            status_states,
            lifecycle_events,
        },
        coordinator,
    )
}

#[tokio::test]
async fn ensure_ready_succeeds_when_network_already_started() {
    let (mocks, coordinator) =
        build_coordinator_with_network_error(false, Some("network already started"));

    let result = coordinator.ensure_ready().await;

    assert!(
        result.is_ok(),
        "already started network should be treated as non-fatal"
    );
    assert_eq!(mocks.watcher_calls.load(Ordering::SeqCst), 1);
    assert_eq!(mocks.network_calls.load(Ordering::SeqCst), 1);

    let states = mocks.status_states.lock().await;
    assert_eq!(states.len(), 2);
    assert_eq!(states[0], LifecycleState::Pending);
    assert_eq!(states[1], LifecycleState::Ready);

    let lifecycle_events = mocks.lifecycle_events.lock().await;
    assert_eq!(lifecycle_events.as_slice(), [LifecycleEvent::Ready]);

    let session_events = mocks.session_events.lock().await;
    assert_eq!(session_events.as_slice(), ["ready"]);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn coordinator_records_status_and_failure_event_on_watcher_fail() {
    let (mocks, coordinator) = build_coordinator(true, false);

    let result = coordinator.ensure_ready().await;

    // Must return an error
    assert!(result.is_err(), "should fail when watcher fails");

    // Watcher was called, network was NOT called
    assert_eq!(mocks.watcher_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        mocks.network_calls.load(Ordering::SeqCst),
        0,
        "network should not be called when watcher fails"
    );

    // Session ready should NOT have been emitted
    assert!(
        mocks.session_events.lock().await.is_empty(),
        "session ready should not be emitted on failure"
    );

    // State transitions: Pending → WatcherFailed
    let states = mocks.status_states.lock().await;
    assert_eq!(states.len(), 2);
    assert_eq!(states[0], LifecycleState::Pending);
    assert_eq!(states[1], LifecycleState::WatcherFailed);

    // Lifecycle event: WatcherFailed
    let events = mocks.lifecycle_events.lock().await;
    assert_eq!(events.len(), 1);
    match &events[0] {
        LifecycleEvent::WatcherFailed(msg) => {
            assert!(
                msg.contains("watcher mock failure"),
                "expected error message to contain 'watcher mock failure', got: {}",
                msg
            );
        }
        other => panic!("expected WatcherFailed event, got: {:?}", other),
    }
}

#[tokio::test]
async fn coordinator_records_status_and_failure_event_on_network_fail() {
    let (mocks, coordinator) = build_coordinator(false, true);

    let result = coordinator.ensure_ready().await;

    // Must return an error
    assert!(result.is_err(), "should fail when network fails");

    // Watcher was called (and succeeded), network was called (and failed)
    assert_eq!(mocks.watcher_calls.load(Ordering::SeqCst), 1);
    assert_eq!(mocks.network_calls.load(Ordering::SeqCst), 1);

    // Session ready should NOT have been emitted
    assert!(
        mocks.session_events.lock().await.is_empty(),
        "session ready should not be emitted on failure"
    );

    // State transitions: Pending → NetworkFailed
    let states = mocks.status_states.lock().await;
    assert_eq!(states.len(), 2);
    assert_eq!(states[0], LifecycleState::Pending);
    assert_eq!(states[1], LifecycleState::NetworkFailed);

    // Lifecycle event: NetworkFailed
    let events = mocks.lifecycle_events.lock().await;
    assert_eq!(events.len(), 1);
    match &events[0] {
        LifecycleEvent::NetworkFailed(msg) => {
            assert!(
                msg.contains("network mock failure"),
                "expected error message to contain 'network mock failure', got: {}",
                msg
            );
        }
        other => panic!("expected NetworkFailed event, got: {:?}", other),
    }
}
