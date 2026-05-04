//! GUI-framework agnostic 健康轮询 helpers。
//!
//! 这两个函数只接收一个 `Probe` 闭包返回 `ProbeOutcome`——不绑定
//! `CommandChild`，所以可以放在默认编译路径里给 `uc-desktop` / 其它
//! shell（不启用 `sidecar-lifecycle` feature）直接复用。

use std::future::Future;
use std::time::Duration;

use crate::contract::{DaemonBootstrapError, ProbeOutcome};

/// 轮询 daemon 健康端点，直到 daemon 报告兼容、或超时、或观测到不兼容。
///
/// - `Compatible` → 返回 `Ok(())`
/// - `Incompatible` → 返回 `DaemonBootstrapError::IncompatibleDaemon`
/// - 超时 → 返回 `DaemonBootstrapError::StartupTimeout`
pub async fn wait_for_daemon_health<Probe, ProbeFuture>(
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), DaemonBootstrapError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match probe().await? {
            ProbeOutcome::Compatible(_) => return Ok(()),
            ProbeOutcome::Absent => {}
            ProbeOutcome::Incompatible { details, .. } => {
                return Err(DaemonBootstrapError::IncompatibleDaemon { details });
            }
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(DaemonBootstrapError::StartupTimeout {
                timeout_ms: timeout.as_millis() as u64,
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// 轮询 daemon 健康端点直到观察到 `Absent`（端点消失），或在 `timeout`
/// 内仍能看到 daemon 时返回 `IncompatibleDaemon`。用于不兼容 daemon
/// 替换流程中等待旧进程退出。
pub async fn wait_for_endpoint_absent<Probe, ProbeFuture>(
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
    last_reason: &str,
) -> Result<(), DaemonBootstrapError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        match probe().await? {
            ProbeOutcome::Absent => return Ok(()),
            ProbeOutcome::Compatible(_) | ProbeOutcome::Incompatible { .. } => {}
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(DaemonBootstrapError::IncompatibleDaemon {
                details: format!(
                    "incompatible daemon did not exit within {}ms after replacement attempt: {}",
                    timeout.as_millis(),
                    last_reason
                ),
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use uc_daemon_contract::api::types::HealthResponse;

    fn ok_health() -> HealthResponse {
        HealthResponse {
            status: "ok".into(),
            package_version: "0.6.0".into(),
            api_revision: "rev-1".into(),
        }
    }

    /// Build a probe that returns successive outcomes from `script`. Once
    /// the script is exhausted it keeps returning the last outcome — that
    /// pattern lets us assert "after N polls the test ends" without making
    /// the script worry about loop bounds.
    fn scripted_probe(
        script: Vec<ProbeOutcome>,
    ) -> (
        impl FnMut() -> std::pin::Pin<
            Box<dyn Future<Output = Result<ProbeOutcome, DaemonBootstrapError>> + Send>,
        >,
        Arc<AtomicUsize>,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let script = Arc::new(script);
        let probe = move || {
            let calls = calls_for_closure.clone();
            let script = script.clone();
            Box::pin(async move {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                let idx = n.min(script.len().saturating_sub(1));
                Ok(script[idx].clone())
            })
                as std::pin::Pin<
                    Box<dyn Future<Output = Result<ProbeOutcome, DaemonBootstrapError>> + Send>,
                >
        };
        (probe, calls)
    }

    #[tokio::test]
    async fn wait_for_daemon_health_returns_immediately_on_first_compatible() {
        let (mut probe, calls) = scripted_probe(vec![ProbeOutcome::Compatible(ok_health())]);
        let result = wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
        .await;

        assert!(result.is_ok(), "Compatible probe must succeed: {result:?}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "must NOT poll again once daemon is compatible — wastes startup time"
        );
    }

    #[tokio::test]
    async fn wait_for_daemon_health_polls_through_absent_until_compatible() {
        let (mut probe, calls) = scripted_probe(vec![
            ProbeOutcome::Absent,
            ProbeOutcome::Absent,
            ProbeOutcome::Compatible(ok_health()),
        ]);
        wait_for_daemon_health(&mut probe, Duration::from_secs(5), Duration::from_millis(1))
            .await
            .expect("eventual Compatible must resolve as Ok");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "must poll until Compatible appears"
        );
    }

    #[tokio::test]
    async fn wait_for_daemon_health_propagates_incompatible_immediately() {
        let (mut probe, calls) = scripted_probe(vec![ProbeOutcome::Incompatible {
            details: "bad version".into(),
            observed_package_version: None,
            observed_api_revision: None,
        }]);

        let err =
            wait_for_daemon_health(&mut probe, Duration::from_secs(5), Duration::from_millis(1))
                .await
                .expect_err("Incompatible must surface as IncompatibleDaemon, not get retried");

        match err {
            DaemonBootstrapError::IncompatibleDaemon { details } => {
                assert_eq!(details, "bad version");
            }
            other => panic!("expected IncompatibleDaemon, got: {other}"),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Incompatible is a terminal verdict — must not poll again"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_daemon_health_times_out_when_daemon_never_appears() {
        // start_paused freezes wall clock so the 30s timeout doesn't actually
        // wait — `tokio::time::sleep` advances the paused clock instead.
        let (mut probe, _calls) = scripted_probe(vec![ProbeOutcome::Absent]);
        let err = wait_for_daemon_health(
            &mut probe,
            Duration::from_millis(100),
            Duration::from_millis(10),
        )
        .await
        .expect_err("absent forever must time out");

        match err {
            DaemonBootstrapError::StartupTimeout { timeout_ms } => {
                assert_eq!(timeout_ms, 100, "must report the configured timeout");
            }
            other => panic!("expected StartupTimeout, got: {other}"),
        }
    }

    #[tokio::test]
    async fn wait_for_endpoint_absent_returns_when_daemon_disappears() {
        // Replacement flow: legacy daemon visible at first, then SIGTERM
        // takes effect and probe sees Absent.
        let (mut probe, calls) = scripted_probe(vec![
            ProbeOutcome::Compatible(ok_health()),
            ProbeOutcome::Compatible(ok_health()),
            ProbeOutcome::Absent,
        ]);

        wait_for_endpoint_absent(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "test reason",
        )
        .await
        .expect("Absent must resolve waiter");

        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_endpoint_absent_times_out_with_reason_in_details() {
        // Daemon refuses to die — ensure the failure surfaces both the
        // timeout and the original `last_reason` string so caller logs are
        // diagnosable.
        let (mut probe, _calls) = scripted_probe(vec![ProbeOutcome::Compatible(ok_health())]);

        let err = wait_for_endpoint_absent(
            &mut probe,
            Duration::from_millis(50),
            Duration::from_millis(5),
            "stuck legacy daemon",
        )
        .await
        .expect_err("daemon staying healthy past timeout must error out");

        match err {
            DaemonBootstrapError::IncompatibleDaemon { details } => {
                assert!(
                    details.contains("50ms"),
                    "details must surface the timeout: {details}"
                );
                assert!(
                    details.contains("stuck legacy daemon"),
                    "details must surface the original reason: {details}"
                );
            }
            other => panic!("expected IncompatibleDaemon timeout, got: {other}"),
        }
    }

    #[tokio::test]
    async fn wait_for_daemon_health_propagates_probe_error() {
        // Probe returning a transport-level error (not a ProbeOutcome) must
        // bubble up — wait loops can't recover from a broken HTTP client.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<ProbeOutcome, _>(DaemonBootstrapError::Probe(anyhow::anyhow!(
                    "transport down"
                )))
            })
                as std::pin::Pin<
                    Box<dyn Future<Output = Result<ProbeOutcome, DaemonBootstrapError>> + Send>,
                >
        };

        let err =
            wait_for_daemon_health(&mut probe, Duration::from_secs(5), Duration::from_millis(1))
                .await
                .expect_err("transport error must propagate");

        assert!(matches!(err, DaemonBootstrapError::Probe(_)));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "must not retry through probe errors"
        );
    }
}
