//! Recovery uses disposable development profiles, never the user's system keychain.
#![cfg(target_os = "macos")]

use reqwest::StatusCode;
use serde_json::Value;
use uc_e2e_tests::{get_session_token, setup_initialized_node, TestCli, TestDaemon};

const PASSPHRASE: &str = "isolated-content-lock-recovery-test";

async fn recovery(daemon: &TestDaemon) -> Value {
    let client = reqwest::Client::new();
    let session = get_session_token(daemon, &client).await;
    let response: Value = client
        .get(format!("{}/encryption/recovery", daemon.base_url()))
        .header("Authorization", format!("Session {session}"))
        .send()
        .await
        .expect("recovery query")
        .error_for_status()
        .expect("recovery status")
        .json()
        .await
        .expect("recovery JSON");
    response["data"].clone()
}

fn seed(cli: &TestCli, text: &str) -> String {
    let output = cli.run_ok(&["dev", "seed-clipboard", "--text", text]);
    output
        .lines()
        .find_map(|line| line.strip_prefix("SEED_ENTRY_ID="))
        .expect("seed entry id")
        .to_owned()
}

fn assert_readable(cli: &TestCli, id: &str, text: &str) {
    let output = cli.run_ok(&["--json", "get", "--id", id]);
    let value: Value = serde_json::from_str(output.trim()).expect("entry JSON");
    assert_eq!(value["text"], text);
}

async fn unlock(daemon: &TestDaemon, passphrase: &str) -> StatusCode {
    let client = reqwest::Client::new();
    let session = get_session_token(daemon, &client).await;
    client
        .post(format!(
            "{}/encryption/unlock-with-passphrase",
            daemon.base_url()
        ))
        .header("Authorization", format!("Session {session}"))
        .json(&serde_json::json!({ "passphrase": passphrase }))
        .send()
        .await
        .expect("unlock request")
        .status()
}

async fn session_ready(daemon: &TestDaemon) -> bool {
    let client = reqwest::Client::new();
    let session = get_session_token(daemon, &client).await;
    let response: Value = client
        .get(format!("{}/encryption/state", daemon.base_url()))
        .header("Authorization", format!("Session {session}"))
        .send()
        .await
        .expect("state request")
        .json()
        .await
        .expect("state JSON");
    response["data"]["sessionReady"]
        .as_bool()
        .expect("sessionReady")
}

#[tokio::test]
#[ignore = "requires freshly built uniclip and uniclipd"]
async fn wrong_passphrase_is_rejected_with_a_ready_background_session() {
    let (daemon, _) = setup_initialized_node("content-auth", "Content auth test", PASSPHRASE).await;
    assert!(session_ready(&daemon).await);
    assert_eq!(
        unlock(&daemon, "wrong-passphrase").await,
        StatusCode::FORBIDDEN
    );
    assert!(
        session_ready(&daemon).await,
        "failed GUI authentication must not lock the background"
    );
    assert_eq!(unlock(&daemon, PASSPHRASE).await, StatusCode::OK);
}

#[tokio::test]
#[ignore = "requires freshly built uniclip and uniclipd"]
async fn original_passphrase_recovers_after_keyring_removal() {
    let (mut daemon, cli) =
        setup_initialized_node("missing-keyring", "Recovery test", PASSPHRASE).await;
    daemon.stop_gracefully().await.expect("stop before seeding");
    let old_text = "old encrypted history survives key loss";
    let old_id = seed(&cli, old_text);
    daemon
        .restart_preserving()
        .await
        .expect("control restart with intact keys");
    daemon
        .stop_gracefully()
        .await
        .expect("stop isolated daemon");
    let keyring = daemon.profile.data_dir().join("keyring");
    assert!(
        keyring.is_dir(),
        "test must use file-backed development keyring"
    );
    // Retain a recoverable copy rather than deleting any secret material.
    std::fs::rename(
        &keyring,
        daemon.profile.data_dir().join("keyring-test-backup"),
    )
    .expect("move isolated keyring");
    if let Err(error) = daemon.restart_preserving().await {
        panic!(
            "restart without keyring: {error}\n{}",
            daemon.diagnostic_log()
        );
    }
    assert!(!session_ready(&daemon).await);
    let state = recovery(&daemon).await;
    assert_eq!(state["state"], "awaiting_passphrase");
    assert_eq!(state["canSubmitPassphrase"], true);
    assert_eq!(state["restartRequired"], false);
    assert_eq!(state["backgroundReady"], false);
    let protected_file = daemon.profile.data_dir().join("vault/profile-secrets-v1");
    let protected_before = std::fs::read(&protected_file).expect("encrypted recovery file");
    assert_eq!(
        unlock(&daemon, "wrong-passphrase").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        std::fs::read(&protected_file).unwrap(),
        protected_before,
        "wrong passphrase must not change encrypted recovery data"
    );
    assert_eq!(recovery(&daemon).await["backgroundReady"], false);
    assert_eq!(unlock(&daemon, PASSPHRASE).await, StatusCode::OK);
    assert!(session_ready(&daemon).await);
    assert_eq!(recovery(&daemon).await["state"], "recovered");
    assert_eq!(recovery(&daemon).await["restartRequired"], false);
    assert_readable(&cli, &old_id, old_text);
    daemon
        .stop_gracefully()
        .await
        .expect("stop before new write");
    let new_text = "new encrypted history after key recovery";
    let new_id = seed(&cli, new_text);
    daemon
        .restart_preserving()
        .await
        .expect("restart after recovery");
    assert!(
        session_ready(&daemon).await,
        "restored keyring must survive restart"
    );
    assert_readable(&cli, &old_id, old_text);
    assert_readable(&cli, &new_id, new_text);
}

#[tokio::test]
#[ignore = "requires freshly built uniclip and uniclipd"]
async fn original_passphrase_recovers_after_unlock_key_removal() {
    let (mut daemon, _) =
        setup_initialized_node("missing-unlock-key", "Recovery test", PASSPHRASE).await;
    daemon
        .restart_preserving()
        .await
        .expect("control restart with intact keys");
    daemon
        .stop_gracefully()
        .await
        .expect("stop isolated daemon");
    let keyring = daemon.profile.data_dir().join("keyring");
    // FileSecureStorage hex-encodes entry names. Select only the passphrase-derived KEK.
    let prefix: String = b"kek:v1:"
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let entries: Vec<_> = std::fs::read_dir(&keyring)
        .expect("test keyring")
        .map(|entry| entry.expect("keyring entry"))
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "must identify exactly one isolated unlock key"
    );
    std::fs::rename(
        entries[0].path(),
        daemon.profile.data_dir().join("unlock-key-test-backup"),
    )
    .expect("move isolated unlock key");
    daemon
        .restart_preserving()
        .await
        .expect("restart without unlock key");
    assert!(!session_ready(&daemon).await);
    assert_eq!(
        unlock(&daemon, "wrong-passphrase").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(unlock(&daemon, PASSPHRASE).await, StatusCode::OK);
    assert!(session_ready(&daemon).await);
    daemon
        .restart_preserving()
        .await
        .expect("restart after recovery");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while !session_ready(&daemon).await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "restored key must survive restart"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore = "requires freshly built uniclip and uniclipd"]
async fn original_passphrase_recovers_after_saved_key_is_replaced() {
    let (mut daemon, _) =
        setup_initialized_node("wrong-saved-key", "Recovery test", PASSPHRASE).await;
    daemon
        .stop_gracefully()
        .await
        .expect("stop isolated daemon");
    let prefix: String = b"kek:v1:"
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let entries: Vec<_> = std::fs::read_dir(daemon.profile.data_dir().join("keyring"))
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .collect();
    assert_eq!(entries.len(), 1);
    std::fs::copy(
        entries[0].path(),
        daemon.profile.data_dir().join("key-before-corruption"),
    )
    .unwrap();
    // Only the disposable development profile is changed, never system credentials.
    std::fs::write(entries[0].path(), [0x42u8; 32]).unwrap();
    daemon
        .restart_preserving()
        .await
        .expect("restricted startup with incorrect stored key");
    assert_eq!(recovery(&daemon).await["backgroundReady"], false);
    assert_eq!(
        unlock(&daemon, "wrong-passphrase").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(std::fs::read(entries[0].path()).unwrap(), vec![0x42; 32]);
    assert_eq!(unlock(&daemon, PASSPHRASE).await, StatusCode::OK);
    daemon
        .restart_preserving()
        .await
        .expect("restart after recovery");
    assert_eq!(recovery(&daemon).await["backgroundReady"], true);
}
