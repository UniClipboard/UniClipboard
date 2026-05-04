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
use uc_daemon_contract::api::types::HealthResponse;
use uc_daemon_contract::DAEMON_API_REVISION;
use uc_daemon_local::contract::{terminate_local_daemon_pid, DaemonBootstrapError, ProbeOutcome};
use uc_daemon_local::health_wait::{wait_for_daemon_health, wait_for_endpoint_absent};
use uc_daemon_local::process_metadata::read_pid_file;
use uc_daemon_local::socket::try_resolve_daemon_http_addr;

use crate::daemon::run_mode::DaemonRunMode;
use crate::daemon::{start_in_process, DaemonOwnership};

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
    let url = format!("http://{}:{}{}", addr.ip(), addr.port(), HEALTH_PATH);

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
    let health = match serde_json::from_str::<HealthResponse>(&body) {
        Ok(health) => health,
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
/// 行为：
/// 1. 探测本机 daemon HTTP 端点；
/// 2. **Compatible** —— 已有外部 daemon（如 `cli start` 拉起的独立进程）
///    在跑且版本匹配，把 `ownership` 标记为 [`DaemonOwnership::set_external`]
///    并返回连接信息；
/// 3. **Absent** —— 没有 daemon，调 [`start_in_process`] in-process 拉起
///    （[`DaemonRunMode::GuiInProcess`]），把 handle 存进 `ownership`，
///    等到 daemon 健康再返回连接信息；
/// 4. **Incompatible** —— 旧版 daemon（决策 B1：legacy "杀并替换"）：
///    SIGTERM 旧 daemon → 等端点消失 → in-process 拉起 → 等健康。
///
/// 调用方（shell）持有 `ownership` 的 clone；GUI 退出 hook 里调
/// [`DaemonOwnership::take_owned`] 拿到 handle 触发 shutdown，仅在
/// `Owned` 状态生效——`External` 状态下 daemon 是别人的不动它。
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
            start_owned_in_process(
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
            start_owned_in_process(
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

async fn start_owned_in_process(
    ownership: &DaemonOwnership,
    client: &reqwest::Client,
    expected_package_version: &str,
    health_check_timeout: Duration,
    health_poll_interval: Duration,
) -> Result<(), DaemonBootstrapError> {
    let handle = start_in_process(DaemonRunMode::GuiInProcess)
        .await
        .map_err(|error| {
            DaemonBootstrapError::Spawn(error.context("in-process daemon start failed"))
        })?;
    ownership.set_owned(handle);

    let mut probe_fn = || async { probe_daemon_health(client, expected_package_version).await };
    wait_for_daemon_health(&mut probe_fn, health_check_timeout, health_poll_interval).await
}

pub fn terminate_incompatible_daemon_from_pid_file() -> Result<(), DaemonBootstrapError> {
    let pid = read_pid_file()
        .map_err(|error| DaemonBootstrapError::IncompatibleDaemon {
            details: format!("failed to read daemon pid metadata: {error}"),
        })?
        .ok_or_else(|| DaemonBootstrapError::IncompatibleDaemon {
            details: "expected incompatible daemon pid metadata was missing".to_string(),
        })?;

    terminate_local_daemon_pid(pid).map_err(|e| DaemonBootstrapError::IncompatibleDaemon {
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
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_health()))
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
            .respond_with(ResponseTemplate::new(200).set_body_json(bad))
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
}
