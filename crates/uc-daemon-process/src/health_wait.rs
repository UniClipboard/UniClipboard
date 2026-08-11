//! GUI-framework agnostic 健康轮询 helpers。
//!
//! 这两个函数只接收一个 `Probe` 闭包返回 `ProbeOutcome`——不绑定具体的
//! 子进程 handle，所以可以放在默认编译路径里给 `uc-desktop` / `uc-cli` /
//! 其它 shell 直接复用。

use std::future::Future;
use std::time::Duration;

use crate::contract::{DaemonBootstrapError, ProbeOutcome};

/// Liveness of a freshly-spawned daemon child process, as observed by
/// [`wait_for_daemon_health_watching_child`].
///
/// Transport-neutral by design — it carries no `std::process` types — so the
/// wait loop stays decoupled from the spawn primitive and can be unit-tested
/// with a scripted poller. The bridge from a real [`std::process::Child`] lives
/// in [`crate::spawn::poll_child_liveness`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildLiveness {
    /// The child is still running, or its liveness could not be determined
    /// (treated as "keep waiting" — the health timeout still bounds the loop).
    Running,
    /// The child has exited with the given process exit code, if the OS
    /// reported one. On Windows this surfaces the NTSTATUS (e.g. `0xC0000135`
    /// STATUS_DLL_NOT_FOUND for a missing dependency — issue #1259).
    Exited { code: Option<i32> },
}

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

/// Like [`wait_for_daemon_health`] but ALSO watches the freshly-spawned child
/// process: if it exits before `/health` reports healthy, fail fast with
/// [`DaemonBootstrapError::SpawnExitedEarly`] instead of spinning until the
/// (long) startup timeout.
///
/// This is what distinguishes "the daemon binary crashed on launch" (e.g. a
/// missing `vcruntime140.dll` on Windows, where `Command::spawn` still returns
/// `Ok` — issue #1259) from "the daemon is merely slow to start". `poll_child`
/// reports the child's [`ChildLiveness`]; production wraps
/// [`crate::spawn::poll_child_liveness`], tests script it.
///
/// Ordering within each iteration is **probe first, then liveness**: a daemon
/// that has genuinely come up wins even in the rare race where our spawned
/// child has exited but a healthy daemon is reachable (e.g. another actor's).
/// Connection-refused probes return `Absent` immediately, so probe-first adds
/// no latency to detecting a crash — the exit is caught on the next iteration.
pub async fn wait_for_daemon_health_watching_child<Probe, ProbeFuture, ChildPoll>(
    probe: &mut Probe,
    poll_child: &mut ChildPoll,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), DaemonBootstrapError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
    ChildPoll: FnMut() -> ChildLiveness,
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

        // The child exited before serving `/health` — a dead process can never
        // turn healthy, so surface the crash (with its exit code) now rather
        // than after the full startup timeout.
        if let ChildLiveness::Exited { code } = poll_child() {
            return Err(DaemonBootstrapError::SpawnExitedEarly { code });
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
    use uc_daemon_contract::api::types::{DaemonResidency, HealthResponse};

    fn ok_health() -> HealthResponse {
        HealthResponse {
            status: "ok".into(),
            degraded_reason: None,
            package_version: "0.6.0".into(),
            api_revision: "rev-1".into(),
            residency: DaemonResidency::Standalone,
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

    // ---- wait_for_daemon_health_watching_child: early-exit detection ----

    /// A scripted child poller mirroring `scripted_probe`: returns successive
    /// `ChildLiveness` values, then repeats the last one forever.
    fn scripted_child(script: Vec<ChildLiveness>) -> impl FnMut() -> ChildLiveness {
        let mut n = 0usize;
        move || {
            let idx = n.min(script.len().saturating_sub(1));
            n += 1;
            script[idx]
        }
    }

    #[tokio::test]
    async fn watching_child_fails_fast_when_child_exits_before_health() {
        // The issue #1259 case: the daemon binary launches but its process
        // dies before `/health` ever comes up. We must surface the exit code,
        // not wait out the full startup timeout.
        let (mut probe, probe_calls) = scripted_probe(vec![ProbeOutcome::Absent]);
        // First poll: still starting; second poll: dead with a DLL-load code.
        let mut poll_child = scripted_child(vec![
            ChildLiveness::Running,
            ChildLiveness::Exited {
                code: Some(0xC000_0135u32 as i32),
            },
        ]);

        let err = wait_for_daemon_health_watching_child(
            &mut probe,
            &mut poll_child,
            Duration::from_secs(30),
            Duration::from_millis(1),
        )
        .await
        .expect_err("a child that exits before health must fail fast");

        match err {
            DaemonBootstrapError::SpawnExitedEarly { code } => {
                assert_eq!(code, Some(0xC000_0135u32 as i32));
            }
            other => panic!("expected SpawnExitedEarly, got: {other}"),
        }
        // Must NOT have spun for the full timeout — a couple of polls, no more.
        assert!(
            probe_calls.load(Ordering::SeqCst) <= 2,
            "must fail on the exit, not exhaust the startup budget"
        );
    }

    #[tokio::test]
    async fn watching_child_succeeds_when_daemon_becomes_healthy() {
        // Child stays alive and the daemon eventually serves `/health`.
        let (mut probe, _) = scripted_probe(vec![
            ProbeOutcome::Absent,
            ProbeOutcome::Compatible(ok_health()),
        ]);
        let mut poll_child = scripted_child(vec![ChildLiveness::Running]);

        wait_for_daemon_health_watching_child(
            &mut probe,
            &mut poll_child,
            Duration::from_secs(5),
            Duration::from_millis(1),
        )
        .await
        .expect("a live child that becomes healthy must resolve Ok");
    }

    #[tokio::test]
    async fn watching_child_prefers_compatible_over_a_racing_exit() {
        // Probe-first ordering: if `/health` is already healthy, a same-tick
        // child exit (e.g. a different, healthy daemon owns the port) must not
        // mask success.
        let (mut probe, _) = scripted_probe(vec![ProbeOutcome::Compatible(ok_health())]);
        let mut poll_child = scripted_child(vec![ChildLiveness::Exited { code: Some(1) }]);

        wait_for_daemon_health_watching_child(
            &mut probe,
            &mut poll_child,
            Duration::from_secs(5),
            Duration::from_millis(1),
        )
        .await
        .expect("a healthy probe must win over a racing child exit");
    }

    #[tokio::test(start_paused = true)]
    async fn watching_child_still_times_out_when_child_lingers_unhealthy() {
        // A child that stays alive but never serves `/health` must still hit
        // the startup timeout — liveness watching only shortcuts a real crash.
        let (mut probe, _) = scripted_probe(vec![ProbeOutcome::Absent]);
        let mut poll_child = scripted_child(vec![ChildLiveness::Running]);

        let err = wait_for_daemon_health_watching_child(
            &mut probe,
            &mut poll_child,
            Duration::from_millis(100),
            Duration::from_millis(10),
        )
        .await
        .expect_err("a lingering-but-unhealthy child must time out");

        assert!(matches!(
            err,
            DaemonBootstrapError::StartupTimeout { timeout_ms: 100 }
        ));
    }
}
