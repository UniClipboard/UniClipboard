//! 桌面侧 daemon 健康探测与拉起协调的 helpers（GUI-framework agnostic）。
//!
//! GUI shell 启动期通过 [`bootstrap_daemon_in_process`] 探测 → 连或拉
//! daemon；本模块还导出供其它 shell 自行编排时复用的工具：
//!
//! - 健康探测（HTTP /health → `ProbeOutcome` 分类）
//! - 连接信息加载（`load_daemon_connection_info`）
//! - 不兼容 daemon 终止（`terminate_incompatible_daemon_from_pid_file`）
//!
//! 只用 `reqwest` + `uc-daemon-*` 系列 crate——不依赖任何 GUI 框架。

use std::time::Duration;

use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::types::HealthResponse;
use uc_daemon_contract::DAEMON_API_REVISION;
use uc_daemon_local::contract::{
    terminate_local_daemon_pid, DaemonBootstrapError, ProbeOutcome, TerminateDaemonError,
};
use uc_daemon_local::health_wait::{wait_for_daemon_health, wait_for_endpoint_absent};
use uc_daemon_local::process_metadata::{
    read_pid_metadata, DaemonPidMetadata, DaemonProcessMode, DaemonSpawnOrigin,
};
use uc_daemon_local::socket::try_resolve_daemon_http_addr;
use uc_daemon_local::spawn::spawn_detached_daemon;

use crate::daemon::DaemonOwnership;

pub const HEALTH_PATH: &str = "/health";
pub const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(8);
pub const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
pub const INCOMPATIBLE_DAEMON_EXIT_TIMEOUT: Duration = Duration::from_millis(1500);

/// Probes the daemon HTTP health endpoint for the active profile and classifies its health.
///
/// `expected_package_version` 由调用方传入——典型情况是 shell crate 自己的
/// `env!("CARGO_PKG_VERSION")`，因为 `uc-desktop` 的 cargo 版本号未必和 GUI
/// shell 想校验的一致。
pub async fn probe_daemon_health(
    client: &reqwest::Client,
    expected_package_version: &str,
) -> Result<ProbeOutcome, DaemonBootstrapError> {
    let addr = try_resolve_daemon_http_addr().map_err(|error| {
        DaemonBootstrapError::Probe(
            error.context("failed to resolve profile-aware daemon HTTP address"),
        )
    })?;
    probe_daemon_health_at(client, addr, expected_package_version).await
}

pub async fn probe_daemon_health_at(
    client: &reqwest::Client,
    addr: std::net::SocketAddr,
    expected_package_version: &str,
) -> Result<ProbeOutcome, DaemonBootstrapError> {
    // 用 SocketAddr Display 直接渲染——IPv6 会保留方括号（[::1]:8080），
    // 拼成的 URL 在 reqwest 解析时仍然合法；分别取 ip()/port() 拼接会
    // 漏掉方括号，IPv6 daemon 会被错认成 unreachable。
    let url = format!("http://{addr}{HEALTH_PATH}");

    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(ProbeOutcome::Absent),
        Err(error) => {
            return Err(DaemonBootstrapError::Probe(
                anyhow::Error::new(error).context("daemon health probe request failed"),
            ))
        }
    };

    if !response.status().is_success() {
        return Ok(ProbeOutcome::Incompatible {
            details: format!("daemon health probe returned HTTP {}", response.status()),
            observed_package_version: None,
            observed_api_revision: None,
        });
    }

    let body = response.text().await.map_err(|error| {
        DaemonBootstrapError::Probe(
            anyhow::Error::new(error).context("failed to read daemon health response body"),
        )
    })?;
    // ADR-008 P2: `/health` returns the canonical `{ data, ts }` envelope, not a
    // bare `HealthResponse`. Decode the envelope and take `.data` — decoding the
    // bare struct fails with `missing field \`status\`` and wrongly classifies a
    // healthy daemon as Incompatible, which stalls cold-start bootstrap forever.
    let health = match serde_json::from_str::<ApiEnvelope<HealthResponse>>(&body) {
        Ok(envelope) => envelope.data,
        Err(error) => {
            return Ok(ProbeOutcome::Incompatible {
                details: format!("failed to decode daemon health response: {error}"),
                observed_package_version: None,
                observed_api_revision: None,
            });
        }
    };

    Ok(classify_health_response(health, expected_package_version))
}

/// 把 daemon 上报的健康响应分类成 [`ProbeOutcome`]。
pub fn classify_health_response(
    health: HealthResponse,
    expected_package_version: &str,
) -> ProbeOutcome {
    let observed_package_version = Some(health.package_version.clone());
    let observed_api_revision = Some(health.api_revision.clone());

    if health.status != "ok" {
        return ProbeOutcome::Incompatible {
            details: format!("daemon reported unhealthy status {}", health.status),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.package_version.trim().is_empty() {
        return ProbeOutcome::Incompatible {
            details: "daemon health response missing packageVersion".to_string(),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.api_revision.trim().is_empty() {
        return ProbeOutcome::Incompatible {
            details: "daemon health response missing apiRevision".to_string(),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.package_version != expected_package_version {
        return ProbeOutcome::Incompatible {
            details: format!(
                "daemon packageVersion {} does not match shell packageVersion {}",
                health.package_version, expected_package_version
            ),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.api_revision != DAEMON_API_REVISION {
        return ProbeOutcome::Incompatible {
            details: format!(
                "daemon apiRevision {} does not match required {}",
                health.api_revision, DAEMON_API_REVISION
            ),
            observed_package_version,
            observed_api_revision,
        };
    }

    ProbeOutcome::Compatible(health)
}

pub fn load_daemon_connection_info() -> Result<DaemonConnectionInfo, DaemonBootstrapError> {
    uc_daemon_client::resolve_connection_info_from_env()
        .map_err(DaemonBootstrapError::ConnectionInfo)
}

/// GUI 进程启动时统一的"探测 → 连或拉"入口（双模 daemon 模型）。
///
/// ADR-008 P3-3 (B2'-3): GUI 是外部 `uniclipd` 的纯客户端。不再有 in-process
/// daemon —— 没有 daemon 时拉起的是一个 **detached 外部进程**
/// ([`spawn_detached_daemon`])。
///
/// 行为：
/// 1. 探测本机 daemon HTTP 端点；
/// 2. **Compatible** —— 已有外部 daemon（如 `cli start` 拉起的独立进程）
///    在跑且版本匹配，把 `ownership` 标记为 [`DaemonOwnership::set_external`]
///    并返回连接信息；
/// 3. **Absent** —— 没有 daemon，detached spawn `uniclipd` 外部进程，等到
///    daemon 健康再返回连接信息；
/// 4. **Incompatible** —— 旧版 daemon（决策 B1：legacy "杀并替换"）：
///    SIGTERM 旧 daemon → 等端点消失 → detached spawn → 等健康。
///
/// 所有拉起路径都把 `ownership` 标记为 `External`：GUI-spawned daemon 现在是
/// 独立进程,GUI 退出不再 owns 它的生命周期(ADR-008 D3 orphan-on-quit,
/// 作为 interim 接受,留待 P4 ownership 重设计)。
pub async fn bootstrap_daemon_in_process(
    ownership: &DaemonOwnership,
    expected_package_version: &str,
    incompatible_exit_timeout: Duration,
    health_check_timeout: Duration,
    health_poll_interval: Duration,
) -> Result<DaemonConnectionInfo, DaemonBootstrapError> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| {
            DaemonBootstrapError::Client(
                anyhow::Error::new(error).context("failed to build daemon probe client"),
            )
        })?;

    match probe_daemon_health(&client, expected_package_version).await? {
        ProbeOutcome::Compatible(_) => {
            ownership.set_external();
        }
        ProbeOutcome::Absent => {
            spawn_external_and_wait_health(
                ownership,
                &client,
                expected_package_version,
                health_check_timeout,
                health_poll_interval,
            )
            .await?;
        }
        ProbeOutcome::Incompatible { details, .. } => {
            terminate_incompatible_daemon_from_pid_file()?;
            let mut probe_fn =
                || async { probe_daemon_health(&client, expected_package_version).await };
            wait_for_endpoint_absent(
                &mut probe_fn,
                incompatible_exit_timeout,
                health_poll_interval,
                &details,
            )
            .await?;
            spawn_external_and_wait_health(
                ownership,
                &client,
                expected_package_version,
                health_check_timeout,
                health_poll_interval,
            )
            .await?;
        }
    }

    load_daemon_connection_info()
}

/// Detached-spawn the external `uniclipd` binary, mark ownership `External`, then
/// poll `/health` until the daemon is reachable. The GUI does not own the
/// spawned process's lifecycle (it survives GUI quit — ADR-008 D3 interim).
async fn spawn_external_and_wait_health(
    ownership: &DaemonOwnership,
    client: &reqwest::Client,
    expected_package_version: &str,
    health_check_timeout: Duration,
    health_poll_interval: Duration,
) -> Result<(), DaemonBootstrapError> {
    // ADR-008 D3: tag the spawn as GUI-owned so its PID file records
    // `spawned_by = gui` — this (or another) GUI may stop it on full quit.
    spawn_detached_daemon(DaemonSpawnOrigin::Gui).map_err(|error| {
        DaemonBootstrapError::Spawn(
            anyhow::Error::new(error).context("detached daemon spawn failed"),
        )
    })?;
    ownership.set_external();

    let mut probe_fn = || async { probe_daemon_health(client, expected_package_version).await };
    wait_for_daemon_health(&mut probe_fn, health_check_timeout, health_poll_interval).await
}

/// 终止 PID 文件指向的不兼容 daemon——但**绝不**对 in-process daemon 动手。
///
/// in-process daemon 是另一个 GUI shell 自己进程内的 worker；SIGTERM 它会把
/// 那个 GUI 一起带挂。这里读 `DaemonPidMetadata` 而不是 raw PID，正是为了
/// 对 [`DaemonProcessMode::InProcess`] 留出"我们不能自己处理，请用户去关
/// 那个 GUI"的拒绝路径。
///
/// D22: verifies PID identity (liveness + exe match) before signaling.
pub fn terminate_incompatible_daemon_from_pid_file() -> Result<(), DaemonBootstrapError> {
    use uc_daemon_local::process_metadata::{verify_pid_identity, PidVerification};

    let metadata = read_pid_metadata()
        .map_err(|error| DaemonBootstrapError::IncompatibleDaemon {
            details: format!("failed to read daemon pid metadata: {error}"),
        })?
        .ok_or_else(|| DaemonBootstrapError::IncompatibleDaemon {
            details: "expected incompatible daemon pid metadata was missing".to_string(),
        })?;

    // D22: verify PID identity before sending any signal.
    if let PidVerification::Stale(reason) = verify_pid_identity(&metadata) {
        tracing::info!(
            pid = metadata.pid,
            %reason,
            "incompatible daemon PID is stale — skipping terminate"
        );
        return Ok(());
    }

    terminate_incompatible_daemon_with(|| Ok(Some(metadata)), terminate_local_daemon_pid)
}

/// ADR-008 D3 (P4-3): on a full GUI quit ("彻底退出"), stop the daemon **only**
/// when a GUI spawned it (its PID file records `spawned_by = gui`) and it is
/// genuinely live. A user's own `uniclip start` daemon, an unknown launcher, or
/// a stale PID file is left untouched — the GUI never kills a daemon it does not
/// own.
///
/// Reading `spawned_by` from the PID file (not in-memory ownership) makes this
/// correct across GUI restarts: GUI-A spawns the daemon, goes lightweight, and
/// a cold-restarted GUI-B can still stop it on full quit.
///
/// Best-effort: sends SIGTERM and returns whether a stop was signaled. The
/// daemon's own graceful-shutdown handler (D21) drains in-flight transfer/sync;
/// the GUI does not block — it is exiting anyway.
pub fn stop_gui_spawned_daemon() -> bool {
    use uc_daemon_local::process_metadata::verify_pid_identity;
    stop_gui_spawned_daemon_with(
        read_pid_metadata,
        verify_pid_identity,
        terminate_local_daemon_pid,
    )
}

/// Inner implementation with injected reader / verifier / terminator closures so
/// the GUI-ownership + identity gating can be unit-tested without a real PID
/// file or real signals.
pub(crate) fn stop_gui_spawned_daemon_with<R, V, T>(
    read_metadata: R,
    verify: V,
    terminate: T,
) -> bool
where
    R: FnOnce() -> anyhow::Result<Option<DaemonPidMetadata>>,
    V: FnOnce(&DaemonPidMetadata) -> uc_daemon_local::process_metadata::PidVerification,
    T: FnOnce(u32) -> Result<(), TerminateDaemonError>,
{
    use uc_daemon_local::process_metadata::PidVerification;

    let metadata = match read_metadata() {
        Ok(Some(metadata)) => metadata,
        Ok(None) => return false,
        Err(error) => {
            tracing::warn!(%error, "full-quit: failed to read daemon pid metadata; leaving daemon running");
            return false;
        }
    };

    if !metadata.is_gui_spawned() {
        tracing::info!(
            pid = metadata.pid,
            origin = ?metadata.spawned_by,
            "full-quit: daemon was not GUI-spawned — leaving it running"
        );
        return false;
    }

    // D22 rule #11: never signal a PID that failed identity verification.
    if let PidVerification::Stale(reason) = verify(&metadata) {
        tracing::info!(
            pid = metadata.pid,
            %reason,
            "full-quit: GUI-spawned daemon PID is stale — nothing to stop"
        );
        return false;
    }

    match terminate(metadata.pid) {
        Ok(()) => {
            tracing::info!(
                pid = metadata.pid,
                "full-quit: sent SIGTERM to GUI-spawned daemon"
            );
            true
        }
        Err(error) => {
            tracing::warn!(pid = metadata.pid, %error, "full-quit: failed to terminate GUI-spawned daemon");
            false
        }
    }
}

/// Inner implementation that takes injected reader/terminator closures so the
/// `InProcess` refusal can be unit-tested without touching the real PID file
/// or sending real signals.
pub(crate) fn terminate_incompatible_daemon_with<R, T>(
    read_metadata: R,
    terminate: T,
) -> Result<(), DaemonBootstrapError>
where
    R: FnOnce() -> anyhow::Result<Option<DaemonPidMetadata>>,
    T: FnOnce(u32) -> Result<(), TerminateDaemonError>,
{
    let metadata = read_metadata()
        .map_err(|error| DaemonBootstrapError::IncompatibleDaemon {
            details: format!("failed to read daemon pid metadata: {error}"),
        })?
        .ok_or_else(|| DaemonBootstrapError::IncompatibleDaemon {
            details: "expected incompatible daemon pid metadata was missing".to_string(),
        })?;

    if matches!(metadata.mode, DaemonProcessMode::InProcess) {
        return Err(DaemonBootstrapError::IncompatibleDaemon {
            details: format!(
                "incompatible in-process daemon (pid {}) belongs to another GUI shell; \
                 SIGTERM would tear down that GUI — quit it from its tray menu first",
                metadata.pid
            ),
        });
    }

    terminate(metadata.pid).map_err(|e| DaemonBootstrapError::IncompatibleDaemon {
        details: e.to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_PACKAGE_VERSION: &str = "0.6.0";

    fn ok_health() -> HealthResponse {
        HealthResponse {
            status: "ok".into(),
            package_version: TEST_PACKAGE_VERSION.into(),
            api_revision: DAEMON_API_REVISION.into(),
        }
    }

    // ------- classify_health_response: pure decision table -------

    #[test]
    fn classify_compatible_when_all_fields_match() {
        let outcome = classify_health_response(ok_health(), TEST_PACKAGE_VERSION);
        assert_eq!(outcome, ProbeOutcome::Compatible(ok_health()));
    }

    #[test]
    fn classify_incompatible_when_status_not_ok() {
        let mut health = ok_health();
        health.status = "degraded".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible {
                details,
                observed_package_version,
                observed_api_revision,
            } => {
                assert!(details.contains("degraded"));
                assert_eq!(
                    observed_package_version.as_deref(),
                    Some(TEST_PACKAGE_VERSION)
                );
                assert_eq!(observed_api_revision.as_deref(), Some(DAEMON_API_REVISION));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn classify_incompatible_when_package_version_empty() {
        let mut health = ok_health();
        health.package_version = "   ".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible { details, .. } => {
                assert!(
                    details.contains("packageVersion"),
                    "details must point at the missing field, got: {details}"
                );
            }
            other => panic!("expected Incompatible for empty packageVersion, got {other:?}"),
        }
    }

    #[test]
    fn classify_incompatible_when_api_revision_empty() {
        let mut health = ok_health();
        health.api_revision = "".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible { details, .. } => {
                assert!(
                    details.contains("apiRevision"),
                    "details must point at the missing field, got: {details}"
                );
            }
            other => panic!("expected Incompatible for empty apiRevision, got {other:?}"),
        }
    }

    #[test]
    fn classify_incompatible_when_package_version_mismatches_shell() {
        let mut health = ok_health();
        health.package_version = "0.5.99".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible {
                details,
                observed_package_version,
                ..
            } => {
                assert_eq!(observed_package_version.as_deref(), Some("0.5.99"));
                assert!(
                    details.contains("0.5.99") && details.contains(TEST_PACKAGE_VERSION),
                    "details must surface both observed and expected versions: {details}"
                );
            }
            other => panic!("expected Incompatible for version mismatch, got {other:?}"),
        }
    }

    #[test]
    fn classify_incompatible_when_api_revision_mismatches_constant() {
        let mut health = ok_health();
        health.api_revision = "rev-from-the-future".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible {
                details,
                observed_api_revision,
                ..
            } => {
                assert_eq!(
                    observed_api_revision.as_deref(),
                    Some("rev-from-the-future")
                );
                assert!(details.contains("rev-from-the-future"));
                assert!(details.contains(DAEMON_API_REVISION));
            }
            other => panic!("expected Incompatible for revision mismatch, got {other:?}"),
        }
    }

    // ------- probe_daemon_health_at: mocked HTTP transport -------

    fn build_test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .expect("client build")
    }

    #[tokio::test]
    async fn probe_at_compatible_daemon_returns_compatible() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(ApiEnvelope::now(ok_health())))
            .mount(&server)
            .await;

        let url: url::Url = server.uri().parse().expect("mock server URL");
        let addr = std::net::SocketAddr::new(
            url.host_str()
                .expect("host present")
                .parse()
                .expect("host is ip"),
            url.port().expect("port present"),
        );

        let outcome = probe_daemon_health_at(&build_test_client(), addr, TEST_PACKAGE_VERSION)
            .await
            .expect("probe must succeed against healthy daemon");
        assert_eq!(outcome, ProbeOutcome::Compatible(ok_health()));
    }

    #[tokio::test]
    async fn probe_at_decodes_enveloped_health_body() {
        // Regression (ADR-008 P2): `/health` returns the canonical
        // `{ "data": HealthResponse, "ts": <i64> }` envelope. The probe must
        // unwrap `.data`; decoding a bare `HealthResponse` fails with
        // `missing field \`status\`` and classifies a healthy daemon Incompatible,
        // which hangs cold-start bootstrap forever. Use a raw wire body (not
        // serialize-then-parse) so the exact envelope shape is what's asserted.
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"data":{{"status":"ok","packageVersion":"{TEST_PACKAGE_VERSION}","apiRevision":"{DAEMON_API_REVISION}"}},"ts":1717368000000}}"#
        );
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let url: url::Url = server.uri().parse().unwrap();
        let addr = std::net::SocketAddr::new(
            url.host_str().unwrap().parse().unwrap(),
            url.port().unwrap(),
        );

        let outcome = probe_daemon_health_at(&build_test_client(), addr, TEST_PACKAGE_VERSION)
            .await
            .expect("enveloped health body must decode");
        assert_eq!(outcome, ProbeOutcome::Compatible(ok_health()));
    }

    #[tokio::test]
    async fn probe_at_rejects_legacy_bare_health_body() {
        // The pre-envelope bare `{ status, packageVersion, apiRevision }` shape is
        // retired (ADR-008 P2). A daemon still emitting it is genuinely on the old
        // contract, so it must classify Incompatible rather than slip through.
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"status":"ok","packageVersion":"{TEST_PACKAGE_VERSION}","apiRevision":"{DAEMON_API_REVISION}"}}"#
        );
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let url: url::Url = server.uri().parse().unwrap();
        let addr = std::net::SocketAddr::new(
            url.host_str().unwrap().parse().unwrap(),
            url.port().unwrap(),
        );

        let outcome = probe_daemon_health_at(&build_test_client(), addr, TEST_PACKAGE_VERSION)
            .await
            .expect("legacy bare body must classify, not error out");
        match outcome {
            ProbeOutcome::Incompatible { details, .. } => {
                assert!(
                    details.contains("decode") || details.contains("status"),
                    "details should surface the decode failure: {details}"
                );
            }
            other => panic!("expected Incompatible for legacy bare body, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_at_returns_absent_when_no_listener() {
        // Bind a TCP socket and immediately drop it so we know the port is
        // unused on this host. reqwest's connect attempt will then ECONNREFUSED.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        drop(listener);

        let outcome = probe_daemon_health_at(&build_test_client(), addr, TEST_PACKAGE_VERSION)
            .await
            .expect("probe of dead port must succeed with Absent");
        assert_eq!(
            outcome,
            ProbeOutcome::Absent,
            "connection refused must be classified as Absent so bootstrap can decide to spawn"
        );
    }

    #[tokio::test]
    async fn probe_at_returns_incompatible_when_health_returns_500() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let url: url::Url = server.uri().parse().unwrap();
        let addr = std::net::SocketAddr::new(
            url.host_str().unwrap().parse().unwrap(),
            url.port().unwrap(),
        );

        let outcome = probe_daemon_health_at(&build_test_client(), addr, TEST_PACKAGE_VERSION)
            .await
            .expect("non-2xx must classify, not error out");
        match outcome {
            ProbeOutcome::Incompatible { details, .. } => {
                assert!(
                    details.contains("500"),
                    "details should mention the HTTP status: {details}"
                );
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_at_returns_incompatible_when_body_is_unparseable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
            .mount(&server)
            .await;
        let url: url::Url = server.uri().parse().unwrap();
        let addr = std::net::SocketAddr::new(
            url.host_str().unwrap().parse().unwrap(),
            url.port().unwrap(),
        );

        let outcome = probe_daemon_health_at(&build_test_client(), addr, TEST_PACKAGE_VERSION)
            .await
            .expect("malformed body must classify, not error out");
        match outcome {
            ProbeOutcome::Incompatible { details, .. } => {
                assert!(
                    details.contains("decode") || details.contains("expected"),
                    "details should mention decode failure: {details}"
                );
            }
            other => panic!("expected Incompatible for malformed body, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_at_returns_incompatible_when_version_mismatch() {
        let server = MockServer::start().await;
        let mut bad = ok_health();
        bad.package_version = "9.9.9".into();
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(ApiEnvelope::now(bad)))
            .mount(&server)
            .await;
        let url: url::Url = server.uri().parse().unwrap();
        let addr = std::net::SocketAddr::new(
            url.host_str().unwrap().parse().unwrap(),
            url.port().unwrap(),
        );

        let outcome = probe_daemon_health_at(&build_test_client(), addr, TEST_PACKAGE_VERSION)
            .await
            .expect("probe ok, classifier rejects");
        match outcome {
            ProbeOutcome::Incompatible {
                observed_package_version,
                ..
            } => {
                assert_eq!(observed_package_version.as_deref(), Some("9.9.9"));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    // ------- terminate_incompatible_daemon: InProcess refusal contract -------

    use std::cell::Cell;

    fn metadata_with_mode(pid: u32, mode: DaemonProcessMode) -> DaemonPidMetadata {
        DaemonPidMetadata {
            pid,
            mode,
            started_at_ms: 0,
            spawned_by: DaemonSpawnOrigin::Unknown,
        }
    }

    #[test]
    fn terminate_refuses_in_process_daemon_without_signaling() {
        // Killing an InProcess daemon would SIGTERM the GUI shell that owns it.
        // The replacement path must surface a clear error and never invoke the
        // terminator — even though `terminate_local_daemon_pid` itself happens
        // to refuse pid 0, the GUI's bootstrap must not get that close.
        let signaled = Cell::new(false);

        let result = terminate_incompatible_daemon_with(
            || Ok(Some(metadata_with_mode(4242, DaemonProcessMode::InProcess))),
            |_pid| {
                signaled.set(true);
                Ok(())
            },
        );

        let err = result.expect_err("InProcess metadata must produce a refusal");
        match err {
            DaemonBootstrapError::IncompatibleDaemon { details } => {
                assert!(
                    details.contains("in-process") && details.contains("4242"),
                    "refusal must name the in-process mode and the pid: {details}"
                );
            }
            other => panic!("expected IncompatibleDaemon, got {other:?}"),
        }
        assert!(
            !signaled.get(),
            "terminator must not be invoked for InProcess daemons — \
             that would SIGTERM another GUI shell's process"
        );
    }

    #[test]
    fn terminate_signals_standalone_daemon_with_pid() {
        // The historical happy path: an external `cli start` daemon at a
        // mismatched version. We must hand its pid to the terminator.
        let captured = Cell::new(None);

        let result = terminate_incompatible_daemon_with(
            || {
                Ok(Some(metadata_with_mode(
                    7777,
                    DaemonProcessMode::Standalone,
                )))
            },
            |pid| {
                captured.set(Some(pid));
                Ok(())
            },
        );

        result.expect("Standalone metadata must reach the terminator");
        assert_eq!(
            captured.get(),
            Some(7777),
            "standalone daemons must be terminated by their recorded pid"
        );
    }

    #[test]
    fn terminate_surfaces_missing_metadata_as_incompatible() {
        let result = terminate_incompatible_daemon_with(|| Ok(None), |_pid| Ok(()));
        match result.expect_err("missing metadata must error out") {
            DaemonBootstrapError::IncompatibleDaemon { details } => {
                assert!(
                    details.contains("missing"),
                    "details must point at the missing PID file: {details}"
                );
            }
            other => panic!("expected IncompatibleDaemon, got {other:?}"),
        }
    }

    // ------- stop_gui_spawned_daemon: full-quit ownership + identity gating ----

    fn metadata_origin(pid: u32, origin: DaemonSpawnOrigin) -> DaemonPidMetadata {
        DaemonPidMetadata {
            pid,
            mode: DaemonProcessMode::Standalone,
            started_at_ms: 0,
            spawned_by: origin,
        }
    }

    #[test]
    fn full_quit_stops_live_gui_spawned_daemon() {
        use uc_daemon_local::process_metadata::PidVerification;
        let captured = Cell::new(None);

        let stopped = stop_gui_spawned_daemon_with(
            || Ok(Some(metadata_origin(5050, DaemonSpawnOrigin::Gui))),
            |_m| PidVerification::Active,
            |pid| {
                captured.set(Some(pid));
                Ok(())
            },
        );

        assert!(
            stopped,
            "a live GUI-spawned daemon must be stopped on full quit"
        );
        assert_eq!(captured.get(), Some(5050));
    }

    #[test]
    fn full_quit_leaves_cli_started_daemon_running() {
        use uc_daemon_local::process_metadata::PidVerification;
        for origin in [DaemonSpawnOrigin::Cli, DaemonSpawnOrigin::Unknown] {
            let signaled = Cell::new(false);
            let stopped = stop_gui_spawned_daemon_with(
                || Ok(Some(metadata_origin(6060, origin))),
                |_m| PidVerification::Active,
                |_pid| {
                    signaled.set(true);
                    Ok(())
                },
            );
            assert!(
                !stopped,
                "{origin:?} daemon must be left running on full quit"
            );
            assert!(!signaled.get(), "{origin:?} daemon must never be signaled");
        }
    }

    #[test]
    fn full_quit_skips_stale_gui_spawned_pid() {
        use uc_daemon_local::process_metadata::{PidVerification, StaleReason};
        let signaled = Cell::new(false);

        let stopped = stop_gui_spawned_daemon_with(
            || Ok(Some(metadata_origin(7070, DaemonSpawnOrigin::Gui))),
            |_m| PidVerification::Stale(StaleReason::ProcessNotRunning),
            |_pid| {
                signaled.set(true);
                Ok(())
            },
        );

        assert!(!stopped);
        assert!(
            !signaled.get(),
            "D22: a stale PID must never be signaled even if marked GUI-spawned"
        );
    }

    #[test]
    fn full_quit_with_no_pid_file_is_a_noop() {
        use uc_daemon_local::process_metadata::PidVerification;
        let stopped =
            stop_gui_spawned_daemon_with(|| Ok(None), |_m| PidVerification::Active, |_pid| Ok(()));
        assert!(!stopped);
    }
}
