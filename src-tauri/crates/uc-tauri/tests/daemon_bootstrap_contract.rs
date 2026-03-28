use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use uc_daemon::api::auth::DaemonConnectionInfo;
use uc_daemon::api::types::HealthResponse;
use uc_daemon::DAEMON_API_REVISION;
use uc_daemon_client::DaemonConnectionState;
use uc_tauri::bootstrap::run::{
    bootstrap_daemon_connection_with_hooks, DaemonBootstrapError, ProbeOutcome,
};
use uc_tauri::bootstrap::runtime::DaemonBootstrapOwnershipState;
use uc_daemon_client::daemon_lifecycle::GuiOwnedDaemonState;

fn compatible_health() -> HealthResponse {
    HealthResponse {
        status: "ok".to_string(),
        package_version: env!("CARGO_PKG_VERSION").to_string(),
        api_revision: DAEMON_API_REVISION.to_string(),
    }
}

fn fixed_connection_info() -> DaemonConnectionInfo {
    DaemonConnectionInfo {
        base_url: "http://127.0.0.1:42715".to_string(),
        ws_url: "ws://127.0.0.1:42715/ws".to_string(),
        token: "token-46-3".to_string(),
    }
}

// Note: CommandChild cannot be constructed outside a Tauri runtime, so spawn
// closures in these tests return Ok(None) (no GUI-owned daemon registered).
// Tests that previously asserted snapshot_pid after spawn now skip those assertions.

#[tokio::test]
async fn daemon_bootstrap_replaces_incompatible_daemon_once() {
    let state = DaemonConnectionState::default();
    let ownership = DaemonBootstrapOwnershipState::default();
    let gui_owned_daemon_state = GuiOwnedDaemonState::default();
    let spawn_calls = Arc::new(AtomicUsize::new(0));
    let terminate_calls = Arc::new(AtomicUsize::new(0));
    let probe_step = Arc::new(AtomicUsize::new(0));
    let expected_connection = fixed_connection_info();

    let result = bootstrap_daemon_connection_with_hooks(
        &state,
        &ownership,
        &gui_owned_daemon_state,
        {
            let spawn_calls = Arc::clone(&spawn_calls);
            move || {
                spawn_calls.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        },
        {
            let probe_step = Arc::clone(&probe_step);
            move || {
                let step = probe_step.fetch_add(1, Ordering::SeqCst);
                async move {
                    Ok(match step {
                        0 => ProbeOutcome::Incompatible {
                            details: "stale daemon build".to_string(),
                            observed_package_version: Some("0.0.0-stale".to_string()),
                            observed_api_revision: Some("legacy-v0".to_string()),
                        },
                        1 => ProbeOutcome::Absent,
                        _ => ProbeOutcome::Compatible(compatible_health()),
                    })
                }
            }
        },
        || Ok(expected_connection.clone()),
        {
            let terminate_calls = Arc::clone(&terminate_calls);
            move || {
                terminate_calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
        Duration::from_millis(20),
        Duration::from_millis(40),
        Duration::from_millis(1),
    )
    .await
    .expect("replacement should converge to compatible-ready");

    assert_eq!(result, expected_connection);
    assert_eq!(state.get(), Some(expected_connection));
    assert_eq!(spawn_calls.load(Ordering::SeqCst), 1);
    assert_eq!(terminate_calls.load(Ordering::SeqCst), 1);

    let snapshot = ownership.snapshot();
    assert_eq!(snapshot.replacement_attempt, 1);
    // spawn returned Ok(None) so no GUI-owned child is registered
    assert_eq!(snapshot.spawned_child_pid, None);
    assert_eq!(
        snapshot.last_incompatible_reason.as_deref(),
        Some("stale daemon build")
    );
    assert_eq!(gui_owned_daemon_state.snapshot_pid(), None);
}

#[tokio::test]
async fn daemon_bootstrap_fails_after_bounded_replacement_attempt() {
    let state = DaemonConnectionState::default();
    let ownership = DaemonBootstrapOwnershipState::default();
    let gui_owned_daemon_state = GuiOwnedDaemonState::default();
    let spawn_calls = Arc::new(AtomicUsize::new(0));
    let terminate_calls = Arc::new(AtomicUsize::new(0));

    let result = bootstrap_daemon_connection_with_hooks(
        &state,
        &ownership,
        &gui_owned_daemon_state,
        {
            let spawn_calls = Arc::clone(&spawn_calls);
            move || {
                spawn_calls.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        },
        move || async {
            Ok(ProbeOutcome::Incompatible {
                details: "legacy daemon refused compatibility".to_string(),
                observed_package_version: Some("0.3.9".to_string()),
                observed_api_revision: Some("legacy-v0".to_string()),
            })
        },
        || Ok(fixed_connection_info()),
        {
            let terminate_calls = Arc::clone(&terminate_calls);
            move || {
                terminate_calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
        Duration::from_millis(5),
        Duration::from_millis(20),
        Duration::from_millis(1),
    )
    .await;

    assert!(matches!(
        result,
        Err(DaemonBootstrapError::IncompatibleDaemon { details })
            if details.contains("did not exit within 5ms")
    ));
    assert_eq!(spawn_calls.load(Ordering::SeqCst), 0);
    assert_eq!(terminate_calls.load(Ordering::SeqCst), 1);
    assert!(state.get().is_none());

    let snapshot = ownership.snapshot();
    assert_eq!(snapshot.replacement_attempt, 1);
    assert_eq!(snapshot.spawned_child_pid, None);
    assert_eq!(
        snapshot.last_incompatible_reason.as_deref(),
        Some("legacy daemon refused compatibility")
    );
    assert_eq!(gui_owned_daemon_state.snapshot_pid(), None);
}

#[tokio::test]
async fn daemon_bootstrap_does_not_replace_when_probe_is_absent() {
    let state = DaemonConnectionState::default();
    let ownership = DaemonBootstrapOwnershipState::default();
    let gui_owned_daemon_state = GuiOwnedDaemonState::default();
    let spawn_calls = Arc::new(AtomicUsize::new(0));
    let terminate_calls = Arc::new(AtomicUsize::new(0));
    let probe_step = Arc::new(AtomicUsize::new(0));
    let expected_connection = fixed_connection_info();

    let result = bootstrap_daemon_connection_with_hooks(
        &state,
        &ownership,
        &gui_owned_daemon_state,
        {
            let spawn_calls = Arc::clone(&spawn_calls);
            move || {
                spawn_calls.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        },
        {
            let probe_step = Arc::clone(&probe_step);
            move || {
                let step = probe_step.fetch_add(1, Ordering::SeqCst);
                async move {
                    Ok(match step {
                        0 => ProbeOutcome::Absent,
                        _ => ProbeOutcome::Compatible(compatible_health()),
                    })
                }
            }
        },
        || Ok(expected_connection.clone()),
        {
            let terminate_calls = Arc::clone(&terminate_calls);
            move || {
                terminate_calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
        Duration::from_millis(5),
        Duration::from_millis(20),
        Duration::from_millis(1),
    )
    .await
    .expect("absent endpoint should spawn directly without replacement");

    assert_eq!(result, expected_connection);
    assert_eq!(spawn_calls.load(Ordering::SeqCst), 1);
    assert_eq!(terminate_calls.load(Ordering::SeqCst), 0);

    let snapshot = ownership.snapshot();
    assert_eq!(snapshot.replacement_attempt, 0);
    // spawn returned Ok(None) so no GUI-owned child is registered
    assert_eq!(snapshot.spawned_child_pid, None);
    assert!(snapshot.last_incompatible_reason.is_none());
    assert_eq!(gui_owned_daemon_state.snapshot_pid(), None);
}
