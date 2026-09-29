//! Probe kept as a record: how long does a killed peer stay `connected` in the
//! peer list? It observes and prints only; it runs when UC_E2E_RECORD_ONLY is set.
use std::time::{Duration, Instant};

use uc_e2e_tests::{get_session_token, pair_two_nodes};

#[tokio::test]
#[ignore]
async fn probe_connected_flag_after_peer_kill() {
    if std::env::var_os("UC_E2E_RECORD_ONLY").is_none() {
        eprintln!("skipped: record-only probe, set UC_E2E_RECORD_ONLY=1 to run");
        return;
    }
    let (alice_daemon, _alice_cli, mut bob_daemon, _bob_cli) =
        pair_two_nodes("probe-peer", "probe-pass").await;
    let client = reqwest::Client::new();
    let token = get_session_token(&alice_daemon, &client).await;
    let url = format!("{}/peers", alice_daemon.base_url());

    let read = |label: String| {
        let client = client.clone();
        let url = url.clone();
        let token = token.clone();
        async move {
            let body = client
                .get(&url)
                .header("Authorization", format!("Session {token}"))
                .send()
                .await
                .expect("peers request")
                .text()
                .await
                .expect("peers body");
            println!("PROBE {label}: {body}");
        }
    };

    read("before-kill".to_string()).await;
    bob_daemon.kill();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(70) {
        read(format!("t+{}s", started.elapsed().as_secs())).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
