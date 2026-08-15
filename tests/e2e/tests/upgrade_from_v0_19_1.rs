#![cfg(feature = "v0-19-1-upgrade")]

use serde_json::Value;
#[cfg(target_os = "macos")]
use std::io::Write;
use std::{
    fs::OpenOptions,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};
use uc_e2e_tests::{
    get_session_token, InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon,
    TestProfile, V0_19_1_RELEASE_VERSION,
};

const PASSPHRASE: &str = "v0-19-1-upgrade-e2e-passphrase";

struct UpgradeNode {
    daemon: TestDaemon,
    cli: TestCli,
}

impl UpgradeNode {
    async fn initialized(test_name: &str) -> Self {
        Self::initialized_with_system_clipboard(test_name, false).await
    }

    async fn initialized_with_system_clipboard(test_name: &str, enabled: bool) -> Self {
        let legacy = v0_19_1_binaries();
        let profile = TestProfile::new_v0_19_1_upgrade(test_name);
        assert!(
            profile.name.starts_with("dev-upgrade-v0191-"),
            "upgrade tests must use a fresh dev profile: {}",
            profile.name
        );
        let daemon = TestDaemon::start_clean_configured_with(profile, &legacy, None, |command| {
            if enabled {
                command.env_remove("UC_DAEMON_RUN_MODE");
            }
        })
        .await
        .unwrap_or_else(|error| panic!("start v0.19.1 daemon failed: {error}"));
        let cli = TestCli::with_binaries(&daemon.profile, &legacy);
        let output = cli.run_capture(&[
            "init",
            "--passphrase",
            PASSPHRASE,
            "--device-name",
            test_name,
        ]);
        assert!(
            output.success(),
            "v0.19.1 init failed: stdout={} stderr={} log={}",
            output.stdout,
            output.stderr,
            daemon.diagnostic_log()
        );
        let mut node = Self { daemon, cli };
        if enabled {
            node.daemon
                .restart_preserving_with_system_clipboard()
                .await
                .unwrap_or_else(|error| panic!("restart v0.19.1 desktop daemon failed: {error}"));
        }
        node
    }

    async fn upgrade_to_current(&mut self) {
        self.upgrade_to_current_with_rendezvous(None).await;
    }

    async fn upgrade_to_current_as_gui(&mut self) {
        let current = NodeBinarySet::current();
        self.daemon
            .restart_preserving_as_gui_with(&current)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "current GUI-origin daemon failed to start on v0.19.1 data: {error}\n{}",
                    self.daemon.diagnostic_log()
                )
            });
        self.cli = TestCli::with_binaries(&self.daemon.profile, &current);
    }

    async fn upgrade_to_current_with_rendezvous(&mut self, rendezvous_base_url: Option<&str>) {
        let current = NodeBinarySet::current();
        self.daemon
            .restart_preserving_with(&current, rendezvous_base_url)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "current daemon failed to start on v0.19.1 data: {error}\n{}",
                    self.daemon.diagnostic_log()
                )
            });
        self.cli = TestCli::with_binaries(&self.daemon.profile, &current);
    }
}

fn v0_19_1_binaries() -> NodeBinarySet {
    let directory = std::env::var_os("UC_E2E_V0_19_1_RELEASE_DIR")
        .map(std::path::PathBuf::from)
        .expect("UC_E2E_V0_19_1_RELEASE_DIR must point to the verified v0.19.1 release");
    NodeBinarySet::fixed_release_dir(V0_19_1_RELEASE_VERSION, directory)
        .expect("verified v0.19.1 release binaries")
}

fn spawn_direct_daemon(profile: &TestProfile, binary: &Path, log_name: &str) -> Child {
    let log_path = profile.data_dir().join(log_name);
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|error| panic!("open direct daemon log {}: {error}", log_path.display()));
    let stderr = stdout
        .try_clone()
        .unwrap_or_else(|error| panic!("clone direct daemon log {}: {error}", log_path.display()));
    Command::new(binary)
        .env("UC_PROFILE", &profile.name)
        .env("UNICLIPBOARD_ENV", "development")
        .env("UC_DAEMON_RUN_MODE", "server")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("start direct daemon {}: {error}", binary.display()))
}

fn daemon_conn_pid(profile: &TestProfile) -> Option<u32> {
    let content = std::fs::read_to_string(profile.data_dir().join("daemon.conn")).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    u32::try_from(value.get("pid")?.as_u64()?).ok()
}

async fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("poll direct daemon") {
            return Some(status);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn members(cli: &TestCli) -> Vec<Value> {
    try_members(cli).unwrap_or_else(|error| panic!("{error}"))
}

fn try_members(cli: &TestCli) -> Result<Vec<Value>, String> {
    let output = cli.run_capture(&["--json", "members"]);
    if !output.success() {
        return Err(format!(
            "members failed: stdout={} stderr={}",
            output.stdout, output.stderr
        ));
    }
    serde_json::from_str(output.stdout.trim())
        .map_err(|error| format!("members output is not JSON: {error}\n{}", output.stdout))
}

fn local_device_id(cli: &TestCli) -> String {
    members(cli)
        .into_iter()
        .find(|member| member.get("is_local").and_then(Value::as_bool) == Some(true))
        .and_then(|member| {
            member
                .get("device_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .expect("local device id")
}

fn history(cli: &TestCli) -> Vec<Value> {
    try_history(cli).unwrap_or_else(|error| panic!("{error}"))
}

fn try_history(cli: &TestCli) -> Result<Vec<Value>, String> {
    let output = cli.run_capture(&["--json", "get", "--list", "--limit", "100"]);
    if !output.success() {
        return Err(format!(
            "history list failed: stdout={} stderr={}",
            output.stdout, output.stderr
        ));
    }
    serde_json::from_str(output.stdout.trim()).map_err(|error| {
        format!(
            "history list output is not JSON: {error}\n{}",
            output.stdout
        )
    })
}

#[cfg(target_os = "macos")]
struct ClipboardRestore {
    previous: Vec<u8>,
}

#[cfg(target_os = "macos")]
impl ClipboardRestore {
    fn replace(text: &str) -> Self {
        let previous = Command::new("pbpaste")
            .output()
            .expect("read system clipboard")
            .stdout;
        write_clipboard(text.as_bytes());
        Self { previous }
    }
}

#[cfg(target_os = "macos")]
impl Drop for ClipboardRestore {
    fn drop(&mut self) {
        write_clipboard(&self.previous);
    }
}

#[cfg(target_os = "macos")]
fn write_clipboard(content: &[u8]) {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .expect("start pbcopy");
    child
        .stdin
        .as_mut()
        .expect("pbcopy stdin")
        .write_all(content)
        .expect("write pbcopy input");
    assert!(child.wait().expect("wait for pbcopy").success());
}

#[cfg(target_os = "macos")]
fn write_clipboard_snapshot(format_id: &str, mime: &str, bytes: &[u8]) {
    use uc_platform::clipboard::{
        FormatId, LocalClipboard, MimeType, ObservedClipboardRepresentation, RepresentationId,
        SystemClipboard, SystemClipboardSnapshot,
    };

    let snapshot = SystemClipboardSnapshot {
        ts_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_millis() as i64,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from(format_id),
            Some(MimeType(mime.to_string())),
            bytes.to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    LocalClipboard::new()
        .expect("create local clipboard")
        .write_snapshot(snapshot)
        .expect("write typed system clipboard snapshot");
}

#[cfg(target_os = "macos")]
async fn api_history(daemon: &TestDaemon, client: &reqwest::Client, session: &str) -> Vec<Value> {
    let response = client
        .get(format!("{}/clipboard/entries?limit=100", daemon.base_url()))
        .header("Authorization", format!("Session {session}"))
        .send()
        .await
        .expect("list clipboard entries");
    let status = response.status();
    let body: Value = response.json().await.expect("clipboard entries JSON");
    assert!(
        status.is_success(),
        "list clipboard entries failed: status={status} body={body}"
    );
    body.get("data")
        .unwrap_or(&body)
        .as_array()
        .cloned()
        .expect("clipboard entries data array")
}

#[cfg(target_os = "macos")]
async fn wait_for_api_entry(
    daemon: &TestDaemon,
    client: &reqwest::Client,
    session: &str,
    description: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let entries = api_history(daemon, client, session).await;
        if let Some(entry) = entries.into_iter().find(&predicate) {
            return entry;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "clipboard history did not contain {description} before timeout"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_for_search_entry(cli: &TestCli, args: &[&str], entry_id: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let output = cli.run_capture(args);
        let found = output.success()
            && serde_json::from_str::<Value>(output.stdout.trim()).is_ok_and(|body| {
                body.get("data")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("entry_id").and_then(Value::as_str) == Some(entry_id)
                        })
                    })
            });
        if found {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "search did not return entry {entry_id} before timeout: stdout={} stderr={}",
            output.stdout,
            output.stderr
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_for_history_text(cli: &TestCli, text: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if try_history(cli).is_ok_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.get("preview").and_then(Value::as_str) == Some(text))
        }) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "history did not contain captured text before timeout"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_member_count(cli: &TestCli, expected: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let last_result = match try_members(cli) {
            Ok(members) if members.len() == expected => return,
            Ok(members) => format!("last member count: {}", members.len()),
            Err(error) => error,
        };
        assert!(
            tokio::time::Instant::now() < deadline,
            "member count did not reach {expected} before timeout: {}",
            last_result
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_exact_member_ids(cli: &TestCli, expected: &[String]) {
    let mut expected = expected.to_vec();
    expected.sort();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let last_result = match try_members(cli) {
            Ok(members) => {
                let mut actual: Vec<String> = members
                    .iter()
                    .filter_map(|member| member.get("device_id").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect();
                actual.sort();
                if actual == expected {
                    return;
                }
                format!("last member ids: {actual:?}")
            }
            Err(error) => error,
        };
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} member ids did not converge to {expected:?}: {last_result}",
            cli.profile_name
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_member_absent(cli: &TestCli, device_id: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let member_absent = try_members(cli).is_ok_and(|members| {
            members
                .iter()
                .all(|member| member.get("device_id").and_then(Value::as_str) != Some(device_id))
        });
        if member_absent {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "removed member {device_id} remained visible before timeout"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_remote_online(cli: &TestCli) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (remote_online, last_members) = match try_members(cli) {
            Ok(members) => (
                members.iter().any(|member| {
                    member.get("is_local").and_then(Value::as_bool) == Some(false)
                        && member.get("state").and_then(Value::as_str) == Some("online")
                }),
                format!("{members:?}"),
            ),
            Err(error) => (false, error),
        };
        if remote_online {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} remote member did not become online before timeout; last members: {}",
            cli.profile_name,
            last_members
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_member_online(cli: &TestCli, device_id: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (member_online, last_members) = match try_members(cli) {
            Ok(members) => (
                members.iter().any(|member| {
                    member.get("device_id").and_then(Value::as_str) == Some(device_id)
                        && member.get("state").and_then(Value::as_str) == Some("online")
                }),
                format!("{members:?}"),
            ),
            Err(error) => (false, error),
        };
        if member_online {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} member {device_id} did not become online before timeout; last members: {}",
            cli.profile_name,
            last_members
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_peer_sync_ready(daemon: &TestDaemon, peer_id: &str) {
    let client = reqwest::Client::new();
    let session = get_session_token(daemon, &client).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let response = client
            .get(format!("{}/member/device-trust", daemon.base_url()))
            .header("Authorization", format!("Session {session}"))
            .send()
            .await
            .expect("query device trust while waiting for peer sync readiness");
        let status = response.status();
        let body = response
            .text()
            .await
            .expect("read device trust response while waiting for peer sync readiness");
        let ready = status.is_success()
            && serde_json::from_str::<Value>(&body).is_ok_and(|value| {
                value
                    .get("data")
                    .unwrap_or(&value)
                    .get("devices")
                    .and_then(Value::as_array)
                    .is_some_and(|devices| {
                        devices.iter().any(|device| {
                            device.get("deviceId").and_then(Value::as_str) == Some(peer_id)
                                && device.get("reachability").and_then(Value::as_str)
                                    == Some("online")
                                && device.get("membership").and_then(Value::as_str)
                                    == Some("active")
                                && device.get("groupRelationship").and_then(Value::as_str)
                                    == Some("consistent")
                                && device.get("compatibility").and_then(Value::as_str)
                                    == Some("compatible")
                                && device.get("syncRelationship").and_then(Value::as_str)
                                    == Some("usable")
                        })
                    })
            });
        if ready {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} peer {peer_id} did not become sync-ready before timeout: status={status} body={body}",
            daemon.profile.name
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_offline_peer_without_version_inference(daemon: &TestDaemon, peer_id: &str) {
    let client = reqwest::Client::new();
    let session = get_session_token(daemon, &client).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let response = client
            .get(format!("{}/member/device-trust", daemon.base_url()))
            .header("Authorization", format!("Session {session}"))
            .send()
            .await
            .expect("query device trust while waiting for the offline legacy peer");
        let status = response.status();
        let body = response
            .text()
            .await
            .expect("read device trust response for the offline legacy peer");
        let retained_without_inference = status.is_success()
            && serde_json::from_str::<Value>(&body).is_ok_and(|value| {
                value
                    .get("data")
                    .unwrap_or(&value)
                    .get("devices")
                    .and_then(Value::as_array)
                    .is_some_and(|devices| {
                        devices.iter().any(|device| {
                            device.get("deviceId").and_then(Value::as_str) == Some(peer_id)
                                && device.get("reachability").and_then(Value::as_str)
                                    == Some("offline")
                                && device.get("membership").and_then(Value::as_str)
                                    == Some("unknown")
                                && device.get("groupRelationship").and_then(Value::as_str)
                                    == Some("unknown")
                                && device.get("compatibility").and_then(Value::as_str)
                                    == Some("unknown")
                                && device.get("syncRelationship").and_then(Value::as_str)
                                    == Some("unknown")
                        })
                    })
            });
        if retained_without_inference {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} did not retain offline peer {peer_id} without inferring removal or version: status={status} body={body}",
            daemon.profile.name
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn legacy_unpair(daemon: &TestDaemon, peer_id: &str) {
    let client = reqwest::Client::new();
    let file_token = std::fs::read_to_string(daemon.profile.data_dir().join(".daemon-token"))
        .expect("read legacy daemon token");
    let connect = client
        .post(format!("{}/auth/connect", daemon.base_url()))
        .header("Authorization", format!("Bearer {}", file_token.trim()))
        .json(&serde_json::json!({
            "pid": std::process::id(),
            "clientType": "cli"
        }))
        .send()
        .await
        .expect("connect to legacy daemon");
    let connect_status = connect.status();
    let connect_body: Value = connect.json().await.expect("read legacy auth response");
    assert!(
        connect_status.is_success(),
        "legacy auth failed: status={connect_status} body={connect_body}"
    );
    let session_token = connect_body
        .get("data")
        .unwrap_or(&connect_body)
        .get("sessionToken")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("legacy auth response missing sessionToken: {connect_body}"));

    let response = client
        .post(format!("{}/pairing/unpair", daemon.base_url()))
        .header("Authorization", format!("Session {session_token}"))
        .json(&serde_json::json!({ "peerId": peer_id }))
        .send()
        .await
        .expect("send legacy unpair request");
    let status = response.status();
    let body = response.text().await.expect("read legacy unpair response");
    assert!(
        status.is_success(),
        "legacy unpair failed: status={status} body={body}"
    );
}

async fn wait_for_upgrade_required(cli: &TestCli, expected_device_id: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status = cli.run_capture(&["--json", "status"]);
        let current_status = status.stdout.trim();
        if status.success()
            && serde_json::from_str::<Value>(current_status).is_ok_and(|value| {
                value["device_trust"]["upgrade_required_device_ids"]
                    == serde_json::json!([expected_device_id])
            })
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "device trust did not report {expected_device_id} as upgrade-required before timeout; last status: {}",
            current_status
        );
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn wait_for_upgrade_cleared(cli: &TestCli) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status = cli.run_capture(&["--json", "status"]);
        let current_status = format!(
            "exit={} stdout={} stderr={}",
            status.exit_code,
            status.stdout.trim(),
            status.stderr.trim()
        );
        if status.success()
            && serde_json::from_str::<Value>(status.stdout.trim()).is_ok_and(|value| {
                value["device_trust"]["local_membership"] == serde_json::json!("active")
                    && value["device_trust"]["upgrade_required_device_ids"] == serde_json::json!([])
            })
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "device trust did not become current before timeout; last status: {current_status}"
        );
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn paired_legacy_nodes(test_name: &str) -> (UpgradeNode, UpgradeNode, LocalRendezvous) {
    let legacy = v0_19_1_binaries();
    let rendezvous = LocalRendezvous::start().await;
    let rendezvous_uri = rendezvous.uri();

    let profile_a = TestProfile::new_v0_19_1_upgrade(&format!("{test_name}-a"));
    let daemon_a = TestDaemon::start_clean_with(profile_a, &legacy, Some(&rendezvous_uri))
        .await
        .expect("start legacy node A");
    let cli_a = TestCli::with_binaries(&daemon_a.profile, &legacy);
    let init = cli_a.run_capture(&[
        "init",
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "upgrade-a",
    ]);
    assert!(init.success(), "legacy A init failed: {}", init.stderr);

    let profile_b = TestProfile::new_v0_19_1_upgrade(&format!("{test_name}-b"));
    let daemon_b = TestDaemon::start_clean_with(profile_b, &legacy, Some(&rendezvous_uri))
        .await
        .expect("start legacy node B");
    let cli_b = TestCli::with_binaries(&daemon_b.profile, &legacy);

    let (invite, code) = InviteSession::start(&cli_a).await;
    let join = cli_b.run_capture(&[
        "join",
        "--code",
        &code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "upgrade-b",
    ]);
    invite.finish().await;
    assert!(
        join.success(),
        "legacy B join failed: stdout={} stderr={}",
        join.stdout,
        join.stderr
    );

    let mut a = UpgradeNode {
        daemon: daemon_a,
        cli: cli_a,
    };
    let mut b = UpgradeNode {
        daemon: daemon_b,
        cli: cli_b,
    };
    a.daemon
        .restart_preserving()
        .await
        .expect("restart paired legacy node A");
    b.daemon
        .restart_preserving()
        .await
        .expect("restart paired legacy node B");
    wait_for_member_count(&a.cli, 2).await;
    wait_for_member_count(&b.cli, 2).await;
    wait_for_remote_online(&a.cli).await;
    wait_for_remote_online(&b.cli).await;
    (a, b, rendezvous)
}

async fn three_paired_legacy_nodes(
    test_name: &str,
) -> (UpgradeNode, UpgradeNode, UpgradeNode, LocalRendezvous) {
    let (mut a, mut b, rendezvous) = paired_legacy_nodes(test_name).await;
    let legacy = v0_19_1_binaries();
    let rendezvous_uri = rendezvous.uri();
    let profile_c = TestProfile::new_v0_19_1_upgrade(&format!("{test_name}-c"));
    let daemon_c = TestDaemon::start_clean_with(profile_c, &legacy, Some(&rendezvous_uri))
        .await
        .expect("start legacy node C");
    let cli_c = TestCli::with_binaries(&daemon_c.profile, &legacy);

    let (invite, code) = InviteSession::start(&a.cli).await;
    let join = cli_c.run_capture(&[
        "join",
        "--code",
        &code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "upgrade-c",
    ]);
    invite.finish().await;
    assert!(
        join.success(),
        "legacy C join failed: stdout={} stderr={}",
        join.stdout,
        join.stderr
    );

    // v0.19.1 memberships are local and are not propagated by a sponsor.
    // Pair B and C directly so every legacy node has the complete roster.
    let (invite, code) = InviteSession::start(&b.cli).await;
    let join = cli_c.run_capture(&[
        "join",
        "--code",
        &code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "upgrade-c",
    ]);
    invite.finish().await;
    assert!(
        join.success(),
        "legacy C re-pair with B failed: stdout={} stderr={}",
        join.stdout,
        join.stderr
    );

    let mut c = UpgradeNode {
        daemon: daemon_c,
        cli: cli_c,
    };
    let expected_ids = vec![
        local_device_id(&a.cli),
        local_device_id(&b.cli),
        local_device_id(&c.cli),
    ];
    a.daemon
        .restart_preserving()
        .await
        .expect("restart three-node legacy A");
    b.daemon
        .restart_preserving()
        .await
        .expect("restart three-node legacy B");
    c.daemon
        .restart_preserving()
        .await
        .expect("restart three-node legacy C");
    wait_for_exact_member_ids(&a.cli, &expected_ids).await;
    wait_for_exact_member_ids(&b.cli, &expected_ids).await;
    wait_for_exact_member_ids(&c.cli, &expected_ids).await;
    wait_for_member_online(&a.cli, &expected_ids[1]).await;
    wait_for_member_online(&a.cli, &expected_ids[2]).await;
    wait_for_member_online(&b.cli, &expected_ids[0]).await;
    wait_for_member_online(&b.cli, &expected_ids[2]).await;
    wait_for_member_online(&c.cli, &expected_ids[0]).await;
    wait_for_member_online(&c.cli, &expected_ids[1]).await;
    (a, b, c, rendezvous)
}

#[tokio::test]
#[ignore]
async fn u01_single_node_upgrades_in_place_without_identity_change() {
    let mut node = UpgradeNode::initialized("u01-single-node").await;
    let device_id_before = local_device_id(&node.cli);
    assert_eq!(members(&node.cli).len(), 1);

    node.upgrade_to_current().await;

    let status = node.cli.run_capture(&["--json", "status"]);
    assert!(
        status.success(),
        "current status failed: stdout={} stderr={} log={}",
        status.stdout,
        status.stderr,
        node.daemon.diagnostic_log()
    );
    let status_json: Value = serde_json::from_str(status.stdout.trim()).expect("status JSON");
    assert_eq!(
        status_json["device_trust"]["local_membership"],
        Value::String("active".to_string())
    );
    assert_eq!(local_device_id(&node.cli), device_id_before);
    assert_eq!(members(&node.cli).len(), 1);
}

#[tokio::test]
#[ignore]
#[cfg(target_os = "macos")]
async fn u02_text_history_survives_upgrade() {
    let mut node = UpgradeNode::initialized_with_system_clipboard("u02-history", true).await;
    let text = "v0.19.1 history survives upgrade";
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _clipboard_restore = ClipboardRestore::replace(text);
    wait_for_history_text(&node.cli, text).await;

    node.upgrade_to_current().await;

    let entries = history(&node.cli);
    assert!(
        entries
            .iter()
            .any(|entry| entry.get("preview").and_then(Value::as_str) == Some(text)),
        "current history lost v0.19.1 text: {entries:?}"
    );
}

#[tokio::test]
#[ignore]
#[cfg(target_os = "macos")]
async fn u02_image_file_favorite_search_restore_and_delete_survive_upgrade() {
    let mut node = UpgradeNode::initialized_with_system_clipboard("u02-data", true).await;
    let client = reqwest::Client::new();
    let legacy_session = get_session_token(&node.daemon, &client).await;
    let text = "v0.19.1 searchable restorable compatibility text";
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _clipboard_restore = ClipboardRestore::replace(text);
    let text_entry = wait_for_api_entry(
        &node.daemon,
        &client,
        &legacy_session,
        "legacy text",
        |entry| entry.get("preview").and_then(Value::as_str) == Some(text),
    )
    .await;

    let image_source = tempfile::tempdir().expect("create legacy image source dir");
    let tiff_path = image_source.path().join("v0191-upgrade-image.tiff");
    let png_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src-tauri/test_resources/google.png");
    let conversion = Command::new("/usr/bin/sips")
        .args(["-s", "format", "tiff"])
        .arg(&png_path)
        .arg("--out")
        .arg(&tiff_path)
        .output()
        .expect("convert legacy image to TIFF");
    assert!(
        conversion.status.success(),
        "sips failed: stdout={} stderr={}",
        String::from_utf8_lossy(&conversion.stdout),
        String::from_utf8_lossy(&conversion.stderr)
    );
    let legacy_image_bytes = std::fs::read(&tiff_path).expect("read legacy TIFF source");
    write_clipboard_snapshot("public.tiff", "image/tiff", &legacy_image_bytes);
    let image_entry = wait_for_api_entry(
        &node.daemon,
        &client,
        &legacy_session,
        "legacy image",
        |entry| {
            entry
                .get("contentType")
                .and_then(Value::as_str)
                .is_some_and(|value| value == "image" || value.starts_with("image/"))
        },
    )
    .await;

    let source_dir = tempfile::tempdir().expect("create legacy file source dir");
    let source_path = source_dir.path().join("v0191-upgrade-file.txt");
    let file_bytes = b"v0.19.1 file bytes survive the in-place upgrade\n";
    std::fs::write(&source_path, file_bytes).expect("write legacy source file");
    let file_uri = format!("file://{}\r\n", source_path.display());
    write_clipboard_snapshot("public.file-url", "text/uri-list", file_uri.as_bytes());
    let file_entry = wait_for_api_entry(
        &node.daemon,
        &client,
        &legacy_session,
        "legacy file",
        |entry| {
            entry
                .get("contentType")
                .and_then(Value::as_str)
                .is_some_and(|value| {
                    value == "file" || value == "text/uri-list" || value == "file/uri-list"
                })
        },
    )
    .await;

    let image_id = image_entry["id"].as_str().expect("legacy image id");
    let favorite = client
        .post(format!(
            "{}/clipboard/entries/{image_id}/favorite",
            node.daemon.base_url()
        ))
        .header("Authorization", format!("Session {legacy_session}"))
        .json(&serde_json::json!({ "isFavorited": true }))
        .send()
        .await
        .expect("favorite legacy image");
    assert!(favorite.status().is_success());

    let legacy_entries = api_history(&node.daemon, &client, &legacy_session).await;
    let legacy_count = legacy_entries.len();
    let text_id = text_entry["id"]
        .as_str()
        .expect("legacy text id")
        .to_string();
    let image_id = image_id.to_string();
    let file_id = file_entry["id"]
        .as_str()
        .expect("legacy file id")
        .to_string();
    assert!(legacy_count >= 3);

    node.upgrade_to_current().await;

    let current_session = get_session_token(&node.daemon, &client).await;
    let current_entries = api_history(&node.daemon, &client, &current_session).await;
    assert_eq!(current_entries.len(), legacy_count);
    for expected_id in [&text_id, &image_id, &file_id] {
        assert!(
            current_entries
                .iter()
                .any(|entry| entry.get("id").and_then(Value::as_str) == Some(expected_id)),
            "upgraded history lost entry {expected_id}"
        );
    }
    let upgraded_image = current_entries
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(&image_id))
        .expect("upgraded image entry");
    assert_eq!(upgraded_image["isFavorited"], serde_json::json!(true));

    let export_dir = tempfile::tempdir().expect("create upgraded export dir");
    let image_export = node.cli.run_capture(&[
        "--json",
        "get",
        "--id",
        &image_id,
        "--out",
        export_dir.path().to_str().expect("export dir UTF-8"),
    ]);
    assert!(
        image_export.success(),
        "image export failed: stdout={} stderr={}",
        image_export.stdout,
        image_export.stderr
    );
    let image_out: Value =
        serde_json::from_str(image_export.stdout.trim()).expect("image export JSON");
    let image_path = image_out["path"].as_str().expect("exported image path");
    let source_pixels = image::open(&tiff_path)
        .expect("decode legacy source image")
        .to_rgba8();
    let exported_pixels = image::open(image_path)
        .expect("decode exported upgraded image")
        .to_rgba8();
    assert_eq!(exported_pixels.dimensions(), source_pixels.dimensions());
    assert_eq!(exported_pixels.into_raw(), source_pixels.into_raw());

    let file_export = node.cli.run_capture(&[
        "--json",
        "get",
        "--id",
        &file_id,
        "--out",
        export_dir.path().to_str().expect("export dir UTF-8"),
    ]);
    assert!(
        file_export.success(),
        "file export failed: stdout={} stderr={}",
        file_export.stdout,
        file_export.stderr
    );
    let file_out: Value =
        serde_json::from_str(file_export.stdout.trim()).expect("file export JSON");
    let file_path = file_out["path"].as_str().expect("exported file path");
    assert_eq!(
        std::fs::read(file_path).expect("read exported file"),
        file_bytes
    );

    wait_for_search_entry(&node.cli, &["--json", "search", "searchable"], &text_id).await;
    wait_for_search_entry(
        &node.cli,
        &["--json", "search", "--tag", "favorited"],
        &image_id,
    )
    .await;

    node.daemon
        .restart_preserving_as_gui_with(&NodeBinarySet::current())
        .await
        .expect("restart upgraded daemon for clipboard restore");
    let gui_session = get_session_token(&node.daemon, &client).await;
    let unlock = client
        .post(format!(
            "{}/encryption/unlock-with-passphrase",
            node.daemon.base_url()
        ))
        .header("Authorization", format!("Session {gui_session}"))
        .json(&serde_json::json!({ "passphrase": PASSPHRASE }))
        .send()
        .await
        .expect("unlock GUI-origin upgraded daemon");
    assert!(unlock.status().is_success());

    let restore = client
        .post(format!(
            "{}/clipboard/restore/{text_id}",
            node.daemon.base_url()
        ))
        .header("Authorization", format!("Session {gui_session}"))
        .send()
        .await
        .expect("restore upgraded text entry");
    assert!(restore.status().is_success());
    let restored = Command::new("pbpaste")
        .output()
        .expect("read restored system clipboard");
    assert_eq!(restored.stdout, text.as_bytes());

    let delete = client
        .delete(format!(
            "{}/clipboard/entries/{text_id}",
            node.daemon.base_url()
        ))
        .header("Authorization", format!("Session {gui_session}"))
        .send()
        .await
        .expect("delete upgraded text entry");
    assert_eq!(delete.status(), reqwest::StatusCode::NO_CONTENT);
    assert!(api_history(&node.daemon, &client, &gui_session)
        .await
        .iter()
        .all(|entry| entry.get("id").and_then(Value::as_str) != Some(&text_id)));
}

#[tokio::test]
#[ignore]
async fn u03_modified_settings_survive_upgrade() {
    let mut node = UpgradeNode::initialized("u03-settings").await;
    let client = reqwest::Client::new();
    let legacy_session = get_session_token(&node.daemon, &client).await;
    let patch = serde_json::json!({
        "general": {
            "autoCheckUpdate": false,
            "theme": "dark",
            "language": "zh-CN",
            "telemetryEnabled": false
        },
        "sync": {
            "autoSync": false,
            "syncFrequency": "interval",
            "syncOnRestore": true
        },
        "fileSync": {
            "fileSyncEnabled": false,
            "fileRetentionHours": 72,
            "fileAutoCleanup": false
        },
        "network": {
            "allowRelayFallback": false
        }
    });
    let update = client
        .put(format!("{}/settings", node.daemon.base_url()))
        .header("Authorization", format!("Session {legacy_session}"))
        .json(&patch)
        .send()
        .await
        .expect("update v0.19.1 settings");
    let update_status = update.status();
    let update_body = update.text().await.expect("read settings update response");
    assert!(
        update_status.is_success(),
        "v0.19.1 settings update failed: status={update_status} body={update_body}"
    );

    let legacy_settings = client
        .get(format!("{}/settings", node.daemon.base_url()))
        .header("Authorization", format!("Session {legacy_session}"))
        .send()
        .await
        .expect("get v0.19.1 settings");
    assert!(legacy_settings.status().is_success());
    let legacy_body: Value = legacy_settings.json().await.expect("v0.19.1 settings JSON");
    let legacy = legacy_body.get("data").unwrap_or(&legacy_body);
    assert_eq!(legacy["general"]["theme"], serde_json::json!("dark"));
    assert_eq!(legacy["general"]["language"], serde_json::json!("zh-CN"));
    assert_eq!(legacy["sync"]["autoSync"], serde_json::json!(false));
    assert_eq!(
        legacy["fileSync"]["fileSyncEnabled"],
        serde_json::json!(false)
    );
    assert_eq!(
        legacy["network"]["allowRelayFallback"],
        serde_json::json!(false)
    );

    node.upgrade_to_current().await;

    let current_session = get_session_token(&node.daemon, &client).await;
    let current_settings = client
        .get(format!("{}/settings", node.daemon.base_url()))
        .header("Authorization", format!("Session {current_session}"))
        .send()
        .await
        .expect("get upgraded settings");
    assert!(current_settings.status().is_success());
    let current_body: Value = current_settings
        .json()
        .await
        .expect("current settings JSON");
    let current = current_body.get("data").unwrap_or(&current_body);
    assert_eq!(
        current["general"]["autoCheckUpdate"],
        serde_json::json!(false)
    );
    assert_eq!(current["general"]["theme"], serde_json::json!("dark"));
    assert_eq!(current["general"]["language"], serde_json::json!("zh-CN"));
    assert_eq!(
        current["general"]["telemetryEnabled"],
        serde_json::json!(false)
    );
    assert_eq!(current["sync"]["autoSyncEnabled"], serde_json::json!(false));
    assert_eq!(
        current["sync"]["syncFrequency"],
        serde_json::json!("interval")
    );
    assert_eq!(current["sync"]["syncOnRestore"], serde_json::json!(true));
    assert_eq!(
        current["fileSync"]["fileSyncEnabled"],
        serde_json::json!(false)
    );
    assert_eq!(
        current["fileSync"]["fileRetentionHours"],
        serde_json::json!(72)
    );
    assert_eq!(
        current["fileSync"]["fileAutoCleanup"],
        serde_json::json!(false)
    );
    assert_eq!(
        current["network"]["allowRelayFallback"],
        serde_json::json!(false)
    );
}

#[tokio::test]
#[ignore]
#[cfg(target_os = "macos")]
async fn u05_wrong_passphrase_is_rejected_without_losing_legacy_data() {
    let mut node = UpgradeNode::initialized_with_system_clipboard("u05-passphrase", true).await;
    let device_id_before = local_device_id(&node.cli);
    let legacy_text = "v0.19.1 data survives a wrong upgraded passphrase";
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _clipboard_restore = ClipboardRestore::replace(legacy_text);
    wait_for_history_text(&node.cli, legacy_text).await;

    node.upgrade_to_current().await;

    let client = reqwest::Client::new();
    let session = get_session_token(&node.daemon, &client).await;
    let lock = client
        .post(format!("{}/encryption/lock", node.daemon.base_url()))
        .header("Authorization", format!("Session {session}"))
        .send()
        .await
        .expect("lock upgraded space");
    assert!(lock.status().is_success(), "lock failed: {}", lock.status());

    let wrong_unlock = client
        .post(format!(
            "{}/encryption/unlock-with-passphrase",
            node.daemon.base_url()
        ))
        .header("Authorization", format!("Session {session}"))
        .json(&serde_json::json!({ "passphrase": "definitely-wrong-passphrase" }))
        .send()
        .await
        .expect("attempt wrong passphrase");
    let wrong_status = wrong_unlock.status();
    let wrong_body: Value = wrong_unlock.json().await.expect("wrong-passphrase JSON");
    assert_eq!(wrong_status, reqwest::StatusCode::FORBIDDEN);
    assert_eq!(wrong_body["code"], serde_json::json!("WRONG_PASSPHRASE"));

    let correct_unlock = client
        .post(format!(
            "{}/encryption/unlock-with-passphrase",
            node.daemon.base_url()
        ))
        .header("Authorization", format!("Session {session}"))
        .json(&serde_json::json!({ "passphrase": PASSPHRASE }))
        .send()
        .await
        .expect("unlock with original passphrase");
    let correct_status = correct_unlock.status();
    let correct_body = correct_unlock
        .text()
        .await
        .expect("read correct-passphrase response");
    assert!(
        correct_status.is_success(),
        "original passphrase failed after rejected attempt: status={correct_status} body={correct_body}"
    );

    assert_eq!(local_device_id(&node.cli), device_id_before);
    assert!(
        history(&node.cli)
            .iter()
            .any(|entry| entry.get("preview").and_then(Value::as_str) == Some(legacy_text)),
        "legacy history was lost after wrong-passphrase rejection"
    );
}

#[tokio::test]
#[ignore]
#[cfg(target_os = "macos")]
async fn u04_auto_unlock_setting_survives_gui_restart() {
    let mut node = UpgradeNode::initialized_with_system_clipboard("u04-auto-unlock", true).await;
    let client = reqwest::Client::new();
    let legacy_session = get_session_token(&node.daemon, &client).await;
    let enable = client
        .put(format!("{}/settings", node.daemon.base_url()))
        .header("Authorization", format!("Session {legacy_session}"))
        .json(&serde_json::json!({
            "security": { "autoUnlockEnabled": true }
        }))
        .send()
        .await
        .expect("enable v0.19.1 auto unlock");
    assert!(enable.status().is_success());

    let device_id_before = local_device_id(&node.cli);
    let legacy_text = "v0.19.1 data survives automatic unlock upgrade";
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _clipboard_restore = ClipboardRestore::replace(legacy_text);
    wait_for_history_text(&node.cli, legacy_text).await;

    node.upgrade_to_current_as_gui().await;
    node.daemon
        .restart_preserving_as_gui_with(&NodeBinarySet::current())
        .await
        .expect("restart upgraded daemon as GUI origin");

    let session = get_session_token(&node.daemon, &client).await;
    let settings = client
        .get(format!("{}/settings", node.daemon.base_url()))
        .header("Authorization", format!("Session {session}"))
        .send()
        .await
        .expect("get upgraded auto-unlock setting");
    assert!(settings.status().is_success());
    let settings_body: Value = settings.json().await.expect("upgraded settings JSON");
    let settings_data = settings_body.get("data").unwrap_or(&settings_body);
    assert_eq!(
        settings_data["security"]["autoUnlockEnabled"],
        serde_json::json!(true)
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let automatically_unlocked = loop {
        let state = client
            .get(format!("{}/encryption/state", node.daemon.base_url()))
            .header("Authorization", format!("Session {session}"))
            .send()
            .await
            .expect("get encryption state");
        assert!(state.status().is_success());
        let body: Value = state.json().await.expect("encryption state JSON");
        let data = body.get("data").unwrap_or(&body);
        if data["sessionReady"] == serde_json::json!(true) {
            break true;
        }
        if tokio::time::Instant::now() >= deadline {
            break false;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };

    if !automatically_unlocked {
        let access = client
            .get(format!(
                "{}/encryption/keychain-access",
                node.daemon.base_url()
            ))
            .header("Authorization", format!("Session {session}"))
            .send()
            .await
            .expect("check keychain access");
        assert!(access.status().is_success());
        let access_body: Value = access.json().await.expect("keychain access JSON");
        let access_data = access_body.get("data").unwrap_or(&access_body);
        assert_eq!(
            access_data["granted"],
            serde_json::json!(false),
            "auto unlock stayed locked even though keychain access was granted"
        );

        let manual = client
            .post(format!(
                "{}/encryption/unlock-with-passphrase",
                node.daemon.base_url()
            ))
            .header("Authorization", format!("Session {session}"))
            .json(&serde_json::json!({ "passphrase": PASSPHRASE }))
            .send()
            .await
            .expect("manual fallback unlock");
        assert!(manual.status().is_success());
    }

    assert_eq!(local_device_id(&node.cli), device_id_before);
    assert!(
        history(&node.cli)
            .iter()
            .any(|entry| entry.get("preview").and_then(Value::as_str) == Some(legacy_text)),
        "legacy history was lost after the auto-unlock upgrade"
    );
}

#[tokio::test]
#[ignore]
async fn u16_repeated_current_restarts_do_not_duplicate_identity_or_members() {
    let mut node = UpgradeNode::initialized("u16-restarts").await;
    let device_id_before = local_device_id(&node.cli);
    node.upgrade_to_current().await;

    for _ in 0..3 {
        node.daemon
            .restart_preserving()
            .await
            .unwrap_or_else(|error| panic!("current restart failed: {error}"));
        assert_eq!(local_device_id(&node.cli), device_id_before);
        assert_eq!(members(&node.cli).len(), 1);
    }
}

#[tokio::test]
#[ignore]
async fn u17_current_start_replaces_a_running_legacy_daemon_without_two_writers() {
    let mut node = UpgradeNode::initialized("u17-running-legacy-daemon").await;
    let device_id = local_device_id(&node.cli);
    let member_count = members(&node.cli).len();
    let current = NodeBinarySet::current();
    let mut replacement = spawn_direct_daemon(
        &node.daemon.profile,
        &current.daemon,
        "u17-current-direct-start.log",
    );
    let replacement_pid = replacement.id();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut replaced = false;

    loop {
        let legacy_running = node.daemon.is_running();
        let replacement_running = replacement
            .try_wait()
            .expect("poll current replacement daemon")
            .is_none();
        let replacement_owns_endpoint =
            daemon_conn_pid(&node.daemon.profile) == Some(replacement_pid);
        assert!(
            !(legacy_running && replacement_owns_endpoint),
            "legacy and current daemons both appeared to own the profile"
        );
        if !legacy_running && replacement_running && replacement_owns_endpoint {
            replaced = true;
            break;
        }
        if !replacement_running || tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if !replaced {
        let _ = replacement.kill();
        let _ = replacement.wait();
    }
    assert!(
        replaced,
        "current daemon did not replace the running v0.19.1 daemon: {}",
        std::fs::read_to_string(
            node.daemon
                .profile
                .data_dir()
                .join("u17-current-direct-start.log")
        )
        .unwrap_or_default()
    );

    node.cli = TestCli::with_binaries(&node.daemon.profile, &current);
    assert_eq!(local_device_id(&node.cli), device_id);
    assert_eq!(members(&node.cli).len(), member_count);

    replacement.kill().expect("stop current replacement daemon");
    replacement.wait().expect("reap current replacement daemon");
    node.daemon
        .restart_preserving_with(&current, None)
        .await
        .expect("restart current daemon after direct replacement");
    assert_eq!(local_device_id(&node.cli), device_id);
    assert_eq!(members(&node.cli).len(), member_count);
}

#[tokio::test]
#[ignore]
async fn u18_legacy_daemon_cannot_overwrite_data_written_by_current_version() {
    let (mut a, mut b, rendezvous) = paired_legacy_nodes("u18-downgrade").await;
    let rendezvous_uri = rendezvous.uri();
    let a_id = local_device_id(&a.cli);
    let b_id = local_device_id(&b.cli);
    a.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    b.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_peer_sync_ready(&a.daemon, &b_id).await;
    wait_for_peer_sync_ready(&b.daemon, &a_id).await;

    let current_text = "u18 current data survives rejected legacy restart";
    let send = a
        .cli
        .run_capture(&["--json", "send", current_text, "--peer", &b_id]);
    assert!(
        send.success(),
        "current version failed to send downgrade sentinel: stdout={} stderr={}",
        send.stdout,
        send.stderr
    );
    wait_for_history_text(&b.cli, current_text).await;

    let legacy = v0_19_1_binaries();
    let mut downgrade = spawn_direct_daemon(
        &b.daemon.profile,
        &legacy.daemon,
        "u18-legacy-direct-start.log",
    );
    let status = wait_for_child_exit(&mut downgrade, Duration::from_secs(35))
        .await
        .unwrap_or_else(|| {
            let _ = downgrade.kill();
            let _ = downgrade.wait();
            panic!("v0.19.1 daemon did not stop while current daemon held the profile")
        });
    assert!(
        !status.success(),
        "v0.19.1 daemon unexpectedly accepted the current data profile"
    );
    assert!(
        b.daemon.is_running(),
        "current daemon stopped during downgrade attempt"
    );
    assert_eq!(local_device_id(&b.cli), b_id);
    wait_for_history_text(&b.cli, current_text).await;

    b.daemon
        .restart_preserving_with(&NodeBinarySet::current(), Some(&rendezvous_uri))
        .await
        .expect("restart current daemon after rejected downgrade");
    assert_eq!(local_device_id(&b.cli), b_id);
    wait_for_history_text(&b.cli, current_text).await;
}

#[tokio::test]
#[ignore]
async fn u12_removed_legacy_member_stays_excluded_after_retained_members_upgrade() {
    let (mut a, mut b, rendezvous) = paired_legacy_nodes("u12-removed-member").await;
    let legacy = v0_19_1_binaries();
    let rendezvous_uri = rendezvous.uri();
    let b_id = local_device_id(&b.cli);

    let profile_c = TestProfile::new_v0_19_1_upgrade("u12-removed-member-c");
    let daemon_c = TestDaemon::start_clean_with(profile_c, &legacy, Some(&rendezvous_uri))
        .await
        .expect("start legacy node C");
    let cli_c = TestCli::with_binaries(&daemon_c.profile, &legacy);
    let (invite, code) = InviteSession::start(&a.cli).await;
    let join = cli_c.run_capture(&[
        "join",
        "--code",
        &code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "upgrade-c",
    ]);
    invite.finish().await;
    assert!(
        join.success(),
        "legacy C join failed: stdout={} stderr={}",
        join.stdout,
        join.stderr
    );
    let mut c = UpgradeNode {
        daemon: daemon_c,
        cli: cli_c,
    };
    a.daemon
        .restart_preserving()
        .await
        .expect("restart legacy A");
    b.daemon
        .restart_preserving()
        .await
        .expect("restart legacy B");
    c.daemon
        .restart_preserving()
        .await
        .expect("restart legacy C");
    wait_for_member_count(&a.cli, 3).await;

    legacy_unpair(&a.daemon, &b_id).await;
    wait_for_member_absent(&a.cli, &b_id).await;

    a.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    c.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_member_count(&a.cli, 2).await;
    wait_for_member_count(&c.cli, 2).await;
    wait_for_member_absent(&a.cli, &b_id).await;
    wait_for_member_absent(&c.cli, &b_id).await;
    wait_for_remote_online(&a.cli).await;
    wait_for_remote_online(&c.cli).await;

    let c_id = local_device_id(&c.cli);
    let retained_text = "retained member receives after removed-member upgrade";
    let retained_send = a
        .cli
        .run_capture(&["--json", "send", retained_text, "--peer", &c_id]);
    assert!(
        retained_send.success(),
        "send to retained member failed: stdout={} stderr={}",
        retained_send.stdout,
        retained_send.stderr
    );
    wait_for_history_text(&c.cli, retained_text).await;

    let removed_text = "removed member must not receive after retained-member upgrade";
    let removed_send = a
        .cli
        .run_capture(&["--json", "send", removed_text, "--peer", &b_id]);
    let outcome = serde_json::from_str::<Value>(removed_send.stdout.trim()).unwrap_or_default();
    assert!(
        !removed_send.success() || outcome["totalAccepted"] == serde_json::json!(0),
        "send to removed member was accepted: exit={} stdout={} stderr={}",
        removed_send.exit_code,
        removed_send.stdout,
        removed_send.stderr
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        history(&b.cli)
            .iter()
            .all(|entry| { entry.get("preview").and_then(Value::as_str) != Some(removed_text) }),
        "removed legacy member received post-upgrade content"
    );
}

#[tokio::test]
#[ignore]
async fn u15_interrupted_first_current_start_recovers_without_data_loss() {
    let (mut a, mut b, rendezvous) = paired_legacy_nodes("u15-interrupted-upgrade").await;
    let rendezvous_uri = rendezvous.uri();
    let a_id = local_device_id(&a.cli);
    let b_id = local_device_id(&b.cli);
    let legacy_text = "legacy history survives interrupted first current start";
    let legacy_send = a
        .cli
        .run_capture(&["--json", "send", legacy_text, "--peer", &b_id]);
    assert!(
        legacy_send.success(),
        "legacy send failed: {}",
        legacy_send.stderr
    );
    wait_for_history_text(&b.cli, legacy_text).await;

    a.daemon.kill();
    let current = NodeBinarySet::current();
    let mut interrupted = Command::new(&current.daemon)
        .env("UC_PROFILE", &a.daemon.profile.name)
        .env("UNICLIPBOARD_ENV", "development")
        .env("UC_DAEMON_RUN_MODE", "server")
        .env("UC_E2E_RENDEZVOUS_BASE_URL", &rendezvous_uri)
        .spawn()
        .expect("start first current daemon for interruption");
    tokio::time::sleep(Duration::from_millis(50)).await;
    interrupted.kill().expect("interrupt first current start");
    interrupted.wait().expect("reap interrupted current daemon");

    a.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_upgrade_required(&a.cli, &b_id).await;
    b.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_member_count(&a.cli, 2).await;
    wait_for_member_count(&b.cli, 2).await;
    wait_for_remote_online(&a.cli).await;
    wait_for_remote_online(&b.cli).await;
    assert_eq!(local_device_id(&a.cli), a_id);
    assert_eq!(local_device_id(&b.cli), b_id);
    assert!(history(&b.cli)
        .iter()
        .any(|entry| { entry.get("preview").and_then(Value::as_str) == Some(legacy_text) }));
}

#[tokio::test]
#[ignore]
async fn u14_u20_independent_spaces_and_named_profiles_remain_isolated_after_upgrade() {
    let mut a = UpgradeNode::initialized("u14-u20-profile-a").await;
    let mut b = UpgradeNode::initialized("u14-u20-profile-b").await;
    let a_id = local_device_id(&a.cli);
    let b_id = local_device_id(&b.cli);
    assert_ne!(a_id, b_id);

    a.upgrade_to_current().await;
    b.upgrade_to_current().await;

    let a_members = members(&a.cli);
    let b_members = members(&b.cli);
    assert_eq!(a_members.len(), 1);
    assert_eq!(b_members.len(), 1);
    assert_eq!(local_device_id(&a.cli), a_id);
    assert_eq!(local_device_id(&b.cli), b_id);
    assert!(a_members
        .iter()
        .all(|member| { member.get("device_id").and_then(Value::as_str) != Some(&b_id) }));
    assert!(b_members
        .iter()
        .all(|member| { member.get("device_id").and_then(Value::as_str) != Some(&a_id) }));

    let cross_space_text = "u14 independent spaces must not exchange content";
    let send = a
        .cli
        .run_capture(&["--json", "send", cross_space_text, "--peer", &b_id]);
    assert!(
        !send.success(),
        "cross-space send was accepted: stdout={} stderr={}",
        send.stdout,
        send.stderr
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(history(&b.cli)
        .iter()
        .all(|entry| { entry.get("preview").and_then(Value::as_str) != Some(cross_space_text) }));
}

#[tokio::test]
#[ignore]
async fn u06_u07_u08_staged_two_node_upgrade_recovers_bidirectional_sync() {
    let (mut a, mut b, rendezvous) = paired_legacy_nodes("u06-u08-two-node").await;
    let rendezvous_uri = rendezvous.uri();
    let a_id = local_device_id(&a.cli);
    let b_id = local_device_id(&b.cli);

    let legacy_text = "legacy a to b before upgrade";
    let legacy_send = a
        .cli
        .run_capture(&["--json", "send", legacy_text, "--peer", &b_id]);
    assert!(
        legacy_send.success(),
        "legacy send failed: stdout={} stderr={}",
        legacy_send.stdout,
        legacy_send.stderr
    );
    wait_for_history_text(&b.cli, legacy_text).await;

    a.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_upgrade_required(&a.cli, &b_id).await;

    b.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_member_count(&a.cli, 2).await;
    wait_for_member_count(&b.cli, 2).await;
    wait_for_remote_online(&a.cli).await;
    wait_for_remote_online(&b.cli).await;
    wait_for_upgrade_cleared(&a.cli).await;
    wait_for_upgrade_cleared(&b.cli).await;
    wait_for_peer_sync_ready(&a.daemon, &b_id).await;
    wait_for_peer_sync_ready(&b.daemon, &a_id).await;
    assert_eq!(local_device_id(&a.cli), a_id);
    assert_eq!(local_device_id(&b.cli), b_id);

    let a_to_b = "current a to b after staged upgrade";
    let send_a = a
        .cli
        .run_capture(&["--json", "send", a_to_b, "--peer", &b_id]);
    assert!(send_a.success(), "current A send failed: {}", send_a.stderr);
    wait_for_history_text(&b.cli, a_to_b).await;

    let b_to_a = "current b to a after staged upgrade";
    let send_b = b
        .cli
        .run_capture(&["--json", "send", b_to_a, "--peer", &a_id]);
    assert!(send_b.success(), "current B send failed: {}", send_b.stderr);
    wait_for_history_text(&a.cli, b_to_a).await;
}

#[tokio::test]
#[ignore]
async fn u09_offline_legacy_member_recovers_after_later_upgrade() {
    let (mut a, mut b, rendezvous) = paired_legacy_nodes("u09-offline-upgrade").await;
    let rendezvous_uri = rendezvous.uri();
    let a_id = local_device_id(&a.cli);
    let b_id = local_device_id(&b.cli);
    let expected_ids = vec![a_id.clone(), b_id.clone()];

    b.daemon.kill();
    a.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_exact_member_ids(&a.cli, &expected_ids).await;

    b.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_exact_member_ids(&a.cli, &expected_ids).await;
    wait_for_exact_member_ids(&b.cli, &expected_ids).await;
    wait_for_remote_online(&a.cli).await;
    wait_for_remote_online(&b.cli).await;
    wait_for_upgrade_cleared(&a.cli).await;
    wait_for_upgrade_cleared(&b.cli).await;
    wait_for_peer_sync_ready(&a.daemon, &b_id).await;
    wait_for_peer_sync_ready(&b.daemon, &a_id).await;
    assert_eq!(local_device_id(&a.cli), a_id);
    assert_eq!(local_device_id(&b.cli), b_id);

    let a_to_b = "u09 current a reaches formerly offline b";
    let send_a = a
        .cli
        .run_capture(&["--json", "send", a_to_b, "--peer", &b_id]);
    assert!(send_a.success(), "U09 A send failed: {}", send_a.stderr);
    wait_for_history_text(&b.cli, a_to_b).await;

    let b_to_a = "u09 current b reaches a after later upgrade";
    let send_b = b
        .cli
        .run_capture(&["--json", "send", b_to_a, "--peer", &a_id]);
    assert!(send_b.success(), "U09 B send failed: {}", send_b.stderr);
    wait_for_history_text(&a.cli, b_to_a).await;
}

#[tokio::test]
#[ignore]
async fn u10_u11_three_node_staged_upgrade_converges_and_syncs() {
    let (mut a, mut b, mut c, rendezvous) = three_paired_legacy_nodes("u10-u11-three-node").await;
    let rendezvous_uri = rendezvous.uri();
    let a_id = local_device_id(&a.cli);
    let b_id = local_device_id(&b.cli);
    let c_id = local_device_id(&c.cli);
    let expected_ids = vec![a_id.clone(), b_id.clone(), c_id.clone()];

    c.daemon.kill();
    a.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    b.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_exact_member_ids(&a.cli, &expected_ids).await;
    wait_for_exact_member_ids(&b.cli, &expected_ids).await;
    wait_for_offline_peer_without_version_inference(&a.daemon, &c_id).await;
    wait_for_offline_peer_without_version_inference(&b.daemon, &c_id).await;
    wait_for_peer_sync_ready(&a.daemon, &b_id).await;
    wait_for_peer_sync_ready(&b.daemon, &a_id).await;

    c.upgrade_to_current_with_rendezvous(Some(&rendezvous_uri))
        .await;
    wait_for_exact_member_ids(&a.cli, &expected_ids).await;
    wait_for_exact_member_ids(&b.cli, &expected_ids).await;
    wait_for_exact_member_ids(&c.cli, &expected_ids).await;
    wait_for_upgrade_cleared(&a.cli).await;
    wait_for_upgrade_cleared(&b.cli).await;
    wait_for_upgrade_cleared(&c.cli).await;
    wait_for_peer_sync_ready(&a.daemon, &b_id).await;
    wait_for_peer_sync_ready(&b.daemon, &c_id).await;
    wait_for_peer_sync_ready(&c.daemon, &a_id).await;
    assert_eq!(local_device_id(&a.cli), a_id);
    assert_eq!(local_device_id(&b.cli), b_id);
    assert_eq!(local_device_id(&c.cli), c_id);

    let a_to_b = "u11 current a reaches b";
    let send_a = a
        .cli
        .run_capture(&["--json", "send", a_to_b, "--peer", &b_id]);
    assert!(send_a.success(), "U11 A send failed: {}", send_a.stderr);
    wait_for_history_text(&b.cli, a_to_b).await;

    let b_to_c = "u11 current b reaches c";
    let send_b = b
        .cli
        .run_capture(&["--json", "send", b_to_c, "--peer", &c_id]);
    assert!(send_b.success(), "U11 B send failed: {}", send_b.stderr);
    wait_for_history_text(&c.cli, b_to_c).await;

    let c_to_a = "u11 current c reaches a";
    let send_c = c
        .cli
        .run_capture(&["--json", "send", c_to_a, "--peer", &a_id]);
    assert!(send_c.success(), "U11 C send failed: {}", send_c.stderr);
    wait_for_history_text(&a.cli, c_to_a).await;
}
