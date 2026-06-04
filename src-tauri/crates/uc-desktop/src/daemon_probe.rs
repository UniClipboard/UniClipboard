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

use semver::Version;
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

/// Is the running daemon a *proven* strictly-newer version than this client?
///
/// ADR-008 P4-7 (OQ-downgrade-rollback): a lower-version client must never
/// terminate a higher-version incumbent daemon — that would silently downgrade
/// a running daemon to an older build. This guards the one place that kills an
/// incompatible daemon ([`bootstrap_daemon_in_process`]) so the kill is only
/// the sanctioned takeover of an *older-or-equal* daemon.
///
/// Conservative by design: returns `true` **only** when both versions parse as
/// semver and `observed > expected`. A missing or unparseable observed version
/// (corruption, a foreign process on our port, a daemon that never reported a
/// version) is *not* proven-newer, so it keeps the existing terminate-and-replace
/// behavior — we only ever protect a daemon we can prove is ahead of us.
fn running_daemon_is_strictly_newer(observed: Option<&str>, expected: &str) -> bool {
    let (Some(observed), Ok(expected)) = (observed, Version::parse(expected.trim())) else {
        return false;
    };
    match Version::parse(observed.trim()) {
        Ok(observed) => observed > expected,
        Err(_) => false,
    }
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
/// 4. **Incompatible** —— 版本/契约不匹配的 daemon。分两种方向（ADR-008 P4-7
///    OQ-downgrade-rollback）：
///    - 运行中 daemon **更新**（semver 严格大于本 client）→ incumbent 胜，**拒绝
///      接管**：绝不 SIGTERM（否则把运行中的高版本 daemon 静默降级成旧版），返回
///      [`DaemonBootstrapError::RefusedNewerDaemon`]，连接信息不填充 → GUI 走现有
///      "未连接" UX，日志记 error。
///    - 否则（daemon 更旧 / 版本无法解析 / 不健康，决策 B1：legacy "杀并替换"）：
///      SIGTERM 旧 daemon → 等端点消失 → detached spawn → 等健康。
///
/// 所有拉起路径都把 `ownership` 标记为 `External`（拆分后 GUI 与 daemon 永远
/// 两进程）。"彻底退出是否停 daemon" **不**由这个进程内标记决定——见 ADR-008
/// D3（2026-06-03 修订）与 [`stop_local_daemon_on_full_quit`]。
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
        ProbeOutcome::Incompatible {
            details,
            observed_package_version,
            ..
        } => {
            // ADR-008 P4-7 (OQ-downgrade-rollback): never terminate a daemon we
            // can prove is newer than us — the incumbent higher version wins.
            // Refusing here leaves the connection unset, so the GUI surfaces the
            // standard "not connected" state; the error is logged by the caller.
            if running_daemon_is_strictly_newer(
                observed_package_version.as_deref(),
                expected_package_version,
            ) {
                let observed = observed_package_version.unwrap_or_default();
                tracing::error!(
                    observed_package_version = %observed,
                    expected_package_version = %expected_package_version,
                    %details,
                    "running daemon is newer than this client; refusing to downgrade it"
                );
                return Err(DaemonBootstrapError::RefusedNewerDaemon {
                    observed,
                    expected: expected_package_version.to_string(),
                });
            }
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
/// spawned process's lifecycle: it survives GUI quit; only an explicit
/// "full quit" stops it (ADR-008 D3 three-state, landed in P4-3).
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

/// ADR-008 D3 (P4-3, revised 2026-06-03): on an explicit full GUI quit
/// ("彻底退出"), stop the connected daemon **regardless of who spawned it** — an
/// explicit Quit means "shut everything down". Users who want the daemon to keep
/// running have the dedicated 关窗 (hide) and 轻量模式 (lightweight) actions, so
/// Quit is free to mean a thorough teardown — including a user's own `uniclip
/// start` daemon. (This reverses the original D3 "only stop GUI-spawned" rule on
/// the product owner's call; the three-state tray already covers "keep daemon".)
///
/// Two safety carve-outs remain — neither contradicts "kill the daemon":
/// - **D22 identity check**: only ever signal a PID that is alive AND is a real
///   daemon binary — never a stale or recycled PID.
/// - **legacy `InProcess` refusal**: a live PID marked `InProcess` belongs to an
///   OLD GUI hosting its daemon in-process; SIGTERM would kill that *other GUI*
///   process, not a standalone daemon. Mirrors the `cli stop` contract.
///
/// Best-effort: sends SIGTERM and returns whether a stop was signaled. The
/// daemon's own graceful-shutdown handler (D21) drains in-flight transfer/sync;
/// the GUI does not block — it is exiting anyway.
pub fn stop_local_daemon_on_full_quit() -> bool {
    use uc_daemon_local::process_metadata::verify_pid_identity;
    stop_local_daemon_on_full_quit_with(
        read_pid_metadata,
        verify_pid_identity,
        terminate_local_daemon_pid,
    )
}

/// Inner implementation with injected reader / verifier / terminator closures so
/// the identity + InProcess gating can be unit-tested without a real PID file or
/// real signals.
pub(crate) fn stop_local_daemon_on_full_quit_with<R, V, T>(
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

    // D22 rule #11: never signal a stale / recycled PID. Checked first so a
    // crashed old GUI's stale InProcess metadata is treated as "gone", not as a
    // live GUI to protect.
    if let PidVerification::Stale(reason) = verify(&metadata) {
        tracing::info!(
            pid = metadata.pid,
            %reason,
            "full-quit: daemon PID is stale — nothing to stop"
        );
        return false;
    }

    // Safety: a *live* in-process daemon PID is an OLD GUI process — SIGTERM
    // would kill that GUI, not a standalone daemon. Refuse (cli-stop contract).
    if matches!(metadata.mode, DaemonProcessMode::InProcess) {
        tracing::info!(
            pid = metadata.pid,
            "full-quit: daemon is a legacy in-process GUI worker — refusing to signal"
        );
        return false;
    }

    match terminate(metadata.pid) {
        Ok(()) => {
            tracing::info!(
                pid = metadata.pid,
                origin = ?metadata.spawned_by,
                "full-quit: sent SIGTERM to local daemon"
            );
            true
        }
        Err(error) => {
            tracing::warn!(pid = metadata.pid, %error, "full-quit: failed to terminate daemon");
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

    // ------- running_daemon_is_strictly_newer: downgrade-rollback guard -------

    #[test]
    fn newer_daemon_is_protected_from_downgrade() {
        // The whole point: a lower client must recognise a higher daemon.
        assert!(running_daemon_is_strictly_newer(Some("0.15.0"), "0.14.0"));
        assert!(running_daemon_is_strictly_newer(Some("1.0.0"), "0.14.0"));
        // Pre-release ordering: a later alpha / a stable release both count as
        // newer than an earlier alpha.
        assert!(running_daemon_is_strictly_newer(
            Some("0.14.0-alpha.5"),
            "0.14.0-alpha.4"
        ));
        assert!(running_daemon_is_strictly_newer(
            Some("0.14.0"),
            "0.14.0-alpha.4"
        ));
    }

    #[test]
    fn older_or_equal_daemon_is_not_protected() {
        // Equal → sanctioned takeover path (not a downgrade).
        assert!(!running_daemon_is_strictly_newer(Some("0.14.0"), "0.14.0"));
        // Strictly older → the existing kill-and-replace behavior must stand.
        assert!(!running_daemon_is_strictly_newer(Some("0.13.0"), "0.14.0"));
        assert!(!running_daemon_is_strictly_newer(
            Some("0.14.0-alpha.3"),
            "0.14.0-alpha.4"
        ));
    }

    #[test]
    fn unprovable_versions_are_not_protected() {
        // Missing, blank, or unparseable observed versions are NOT proven-newer,
        // so they fall through to terminate-and-replace (foreign process on our
        // port, corrupted health payload, legacy daemon without a version).
        assert!(!running_daemon_is_strictly_newer(None, "0.14.0"));
        assert!(!running_daemon_is_strictly_newer(Some("   "), "0.14.0"));
        assert!(!running_daemon_is_strictly_newer(
            Some("not-a-version"),
            "0.14.0"
        ));
        // An unparseable *expected* version (should never happen for our own
        // CARGO_PKG_VERSION) also stays conservative: don't protect.
        assert!(!running_daemon_is_strictly_newer(Some("0.15.0"), "garbage"));
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

    // ------- stop_local_daemon_on_full_quit: explicit-quit teardown + safety ----

    fn metadata_origin(pid: u32, origin: DaemonSpawnOrigin) -> DaemonPidMetadata {
        DaemonPidMetadata {
            pid,
            mode: DaemonProcessMode::Standalone,
            started_at_ms: 0,
            spawned_by: origin,
        }
    }

    #[test]
    fn full_quit_stops_any_live_standalone_daemon_regardless_of_origin() {
        // Revised D3: explicit Quit kills the daemon no matter who spawned it —
        // GUI-spawned, `uniclip start`, or a manually-run uniclipd.
        use uc_daemon_local::process_metadata::PidVerification;
        for origin in [
            DaemonSpawnOrigin::Gui,
            DaemonSpawnOrigin::Cli,
            DaemonSpawnOrigin::Unknown,
        ] {
            let captured = Cell::new(None);
            let stopped = stop_local_daemon_on_full_quit_with(
                || Ok(Some(metadata_origin(5050, origin))),
                |_m| PidVerification::Active,
                |pid| {
                    captured.set(Some(pid));
                    Ok(())
                },
            );
            assert!(
                stopped,
                "{origin:?} daemon must be stopped on explicit quit"
            );
            assert_eq!(captured.get(), Some(5050), "must SIGTERM the recorded pid");
        }
    }

    #[test]
    fn full_quit_refuses_live_in_process_daemon() {
        // A live InProcess PID is an OLD GUI hosting its daemon — SIGTERM would
        // kill that GUI, not a standalone daemon. Refuse even on explicit quit.
        use uc_daemon_local::process_metadata::PidVerification;
        let signaled = Cell::new(false);

        let stopped = stop_local_daemon_on_full_quit_with(
            || Ok(Some(metadata_with_mode(8080, DaemonProcessMode::InProcess))),
            |_m| PidVerification::Active,
            |_pid| {
                signaled.set(true);
                Ok(())
            },
        );

        assert!(!stopped);
        assert!(
            !signaled.get(),
            "a live in-process daemon (another GUI) must never be signaled"
        );
    }

    #[test]
    fn full_quit_skips_stale_pid() {
        use uc_daemon_local::process_metadata::{PidVerification, StaleReason};
        let signaled = Cell::new(false);

        let stopped = stop_local_daemon_on_full_quit_with(
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
            "D22: a stale / recycled PID must never be signaled"
        );
    }

    #[test]
    fn full_quit_with_no_pid_file_is_a_noop() {
        use uc_daemon_local::process_metadata::PidVerification;
        let stopped = stop_local_daemon_on_full_quit_with(
            || Ok(None),
            |_m| PidVerification::Active,
            |_pid| Ok(()),
        );
        assert!(!stopped);
    }
}
