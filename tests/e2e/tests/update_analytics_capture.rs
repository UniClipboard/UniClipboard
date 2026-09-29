//! E2E: update-lifecycle analytics sent by the GUI reach a real daemon.
//!
//! The GUI's Rust updater and scheduler hand `CaptureUiEventRequest` values to
//! `DaemonAnalyticsClient`, which posts them to `POST /analytics/capture`. The
//! daemon mirrors the body-safe update fields into its durable JSON log under
//! the `update_diag` target, which is what these tests read back.
//!
//! Run with: cargo test --manifest-path tests/e2e/Cargo.toml --test update_analytics_capture -- --ignored

use std::io::Write as _;
use std::time::Duration;

use serde_json::Value;
use uc_daemon_client::{DaemonAnalyticsClient, DaemonConnectionState};
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_daemon_contract::api::dto::analytics::{
    CaptureUiEventRequest, UiDialogOpenSource, UiDismissSource, UiInstallKind,
    UiNotificationDeliveryStatus, UiUpdateAction, UiUpdateActionOutcome, UiUpdateCheckOutcome,
    UiUpdateCheckSource, UiUpdateFailureKind, UiUpdatePhase,
};
use uc_e2e_tests::{read_daemon_file_token, NodeBinarySet, TestDaemon, TestProfile};

async fn start_daemon(name: &str) -> TestDaemon {
    TestDaemon::start_clean_configured_with(
        TestProfile::new(name),
        &NodeBinarySet::current(),
        None,
        |command| {
            command.env("RUST_LOG", "warn,update_diag=info");
        },
    )
    .await
    .expect("daemon start")
}

fn gui_analytics_client(daemon: &TestDaemon) -> DaemonAnalyticsClient {
    let state = DaemonConnectionState::default();
    state.set(DaemonConnectionInfo {
        base_url: daemon.base_url(),
        ws_url: format!("ws://127.0.0.1:{}/ws", daemon.port()),
        token: read_daemon_file_token(daemon),
        pid: std::process::id(),
    });
    DaemonAnalyticsClient::new(state).expect("analytics client")
}

/// `update_diag` records from the daemon's JSON logs, as flat JSON objects.
fn update_diag_records(daemon: &TestDaemon) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(daemon.profile.log_dir()) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("uniclipboard-daemon.json")
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .flat_map(|content| {
            content
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .collect::<Vec<_>>()
        })
        .filter(|record| record.get("target").and_then(Value::as_str) == Some("update_diag"))
        .collect()
}

fn matches(record: &Value, message: &str, fields: &[(&str, &str)]) -> bool {
    record.get("message").and_then(Value::as_str) == Some(message)
        && fields
            .iter()
            .all(|(key, value)| record.get(*key).and_then(Value::as_str) == Some(*value))
}

/// Append the matched record to `$UC_E2E_EVIDENCE_DIR/update_analytics_capture.jsonl`
/// when that directory is set. Only `target`, `message` and the asserted enum
/// fields are kept, so the evidence carries no address, token or path.
fn record_evidence(test: &str, record: &Value, fields: &[(&str, &str)]) {
    let Some(dir) = std::env::var_os("UC_E2E_EVIDENCE_DIR") else {
        return;
    };
    let mut kept = serde_json::Map::new();
    kept.insert("test".to_string(), Value::from(test));
    for key in ["target", "message"]
        .into_iter()
        .chain(fields.iter().map(|(key, _)| *key))
    {
        if let Some(value) = record.get(key) {
            kept.insert(key.to_string(), value.clone());
        }
    }
    std::fs::create_dir_all(&dir).expect("evidence directory");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::path::Path::new(&dir).join("update_analytics_capture.jsonl"))
        .expect("evidence file");
    writeln!(file, "{}", Value::Object(kept)).expect("write evidence");
}

async fn wait_for_update_diag(
    daemon: &TestDaemon,
    test: &str,
    message: &str,
    fields: &[(&str, &str)],
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let records = update_diag_records(daemon);
        if let Some(record) = records
            .iter()
            .find(|record| matches(record, message, fields))
        {
            record_evidence(test, record, fields);
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!(
                "no update_diag record {message:?} with {fields:?}; saw {records:#?}\n{}",
                daemon.diagnostic_log()
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore]
async fn every_update_event_kind_is_accepted_and_logged_by_the_daemon() {
    let daemon = start_daemon("update-analytics-kinds").await;
    let client = gui_analytics_client(&daemon);

    let cases: Vec<(CaptureUiEventRequest, &str, Vec<(&str, &str)>)> = vec![
        (
            CaptureUiEventRequest::CheckPerformed {
                source: UiUpdateCheckSource::Manual,
                outcome: UiUpdateCheckOutcome::Failed,
                failure_kind: Some(UiUpdateFailureKind::HttpError),
                install_kind: UiInstallKind::Macos,
            },
            "update check performed",
            vec![
                ("source", "Manual"),
                ("outcome", "Failed"),
                ("failure_kind", "Some(HttpError)"),
            ],
        ),
        (
            CaptureUiEventRequest::CheckPerformed {
                source: UiUpdateCheckSource::Scheduled,
                outcome: UiUpdateCheckOutcome::UpToDate,
                failure_kind: None,
                install_kind: UiInstallKind::AppImage,
            },
            "update check performed",
            vec![
                ("source", "Scheduled"),
                ("outcome", "UpToDate"),
                ("failure_kind", "None"),
            ],
        ),
        (
            CaptureUiEventRequest::ActionInvoked {
                action: UiUpdateAction::DownloadBg,
                outcome: UiUpdateActionOutcome::Started,
                error_kind: None,
            },
            "update action invoked",
            vec![
                ("action", "DownloadBg"),
                ("outcome", "Started"),
                ("error_kind", "-"),
            ],
        ),
        (
            CaptureUiEventRequest::ActionInvoked {
                action: UiUpdateAction::DownloadBg,
                outcome: UiUpdateActionOutcome::Failed,
                error_kind: Some("download_failed".to_string()),
            },
            "update action invoked",
            vec![
                ("action", "DownloadBg"),
                ("outcome", "Failed"),
                ("error_kind", "download_failed"),
            ],
        ),
        (
            CaptureUiEventRequest::NotificationShown {
                version: "1.0.1".to_string(),
                delivery_status: UiNotificationDeliveryStatus::SendFailed,
                install_kind: UiInstallKind::Deb,
            },
            "update notification shown",
            vec![("delivery_status", "SendFailed")],
        ),
        (
            CaptureUiEventRequest::DialogOpened {
                source: UiDialogOpenSource::Notification,
                phase: UiUpdatePhase::Available,
                install_kind: UiInstallKind::Windows,
            },
            "update dialog opened",
            vec![("source", "Notification"), ("phase", "Available")],
        ),
        (
            CaptureUiEventRequest::Dismissed {
                phase: UiUpdatePhase::Ready,
                source: UiDismissSource::DialogLater,
            },
            "update dialog dismissed",
            vec![("phase", "Ready"), ("source", "DialogLater")],
        ),
    ];

    for (event, message, fields) in cases {
        client
            .capture(event.clone())
            .await
            .unwrap_or_else(|error| panic!("daemon rejected {event:?}: {error:#}"));
        wait_for_update_diag(&daemon, "every_update_event_kind", message, &fields)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

#[tokio::test]
#[ignore]
async fn background_capture_reaches_the_daemon_without_blocking_the_caller() {
    let daemon = start_daemon("update-analytics-background").await;
    let client = gui_analytics_client(&daemon);

    client.capture_in_background(CaptureUiEventRequest::CheckPerformed {
        source: UiUpdateCheckSource::WindowShow,
        outcome: UiUpdateCheckOutcome::Available,
        failure_kind: None,
        install_kind: UiInstallKind::WindowsPortable,
    });

    wait_for_update_diag(
        &daemon,
        "background_capture",
        "update check performed",
        &[
            ("source", "WindowShow"),
            ("outcome", "Available"),
            ("failure_kind", "None"),
        ],
    )
    .await
    .unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
#[ignore]
async fn unreachable_daemon_drops_update_events_without_failing_the_caller() {
    let event = CaptureUiEventRequest::ActionInvoked {
        action: UiUpdateAction::Install,
        outcome: UiUpdateActionOutcome::Cancelled,
        error_kind: None,
    };

    // Not connected yet: the direct call reports the failure, the background
    // call returns at once and never panics.
    let disconnected =
        DaemonAnalyticsClient::new(DaemonConnectionState::default()).expect("analytics client");
    assert!(disconnected.capture(event.clone()).await.is_err());
    disconnected.capture_in_background(event.clone());

    // Connected to a daemon that has since stopped.
    let mut daemon = start_daemon("update-analytics-stopped").await;
    let stale = gui_analytics_client(&daemon);
    daemon.kill();
    assert!(stale.capture(event.clone()).await.is_err());
    stale.capture_in_background(event.clone());

    // Outside a tokio runtime (a plain thread), the event is dropped.
    std::thread::spawn(move || stale.capture_in_background(event))
        .join()
        .expect("background capture outside a runtime must not panic");

    // Give spawned sends time to fail; the test runtime must still be healthy.
    tokio::time::sleep(Duration::from_millis(500)).await;
}
