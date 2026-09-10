//! Startup discovery stays independent of the normal daemon connection cache.
use uc_daemon_contract::startup::DaemonStartupStatus;
use uc_daemon_process::{
    process_metadata::{
        verify_pid_identity, DaemonPidMetadata, DaemonProcessMode, DaemonSpawnOrigin,
        PidVerification,
    },
    socket,
};

pub async fn read_startup_status(
    client: &reqwest::Client,
) -> anyhow::Result<Option<DaemonStartupStatus>> {
    let Some(conn) = socket::read_daemon_conn_file_at(&socket::resolve_startup_conn_path()?)?
    else {
        return Ok(None);
    };
    let identity = DaemonPidMetadata {
        pid: conn.pid,
        mode: DaemonProcessMode::Standalone,
        started_at_ms: conn.started_at_ms,
        spawned_by: DaemonSpawnOrigin::Unknown,
        package_version: String::new(),
    };
    if !matches!(verify_pid_identity(&identity), PidVerification::Active) {
        return Ok(None);
    }
    let host: std::net::IpAddr = conn.host.parse()?;
    if !host.is_loopback() {
        anyhow::bail!("startup address is not loopback")
    }
    let url = format!(
        "http://{}/startup",
        std::net::SocketAddr::new(host, conn.port)
    );
    let response = match client.get(url).bearer_auth(&conn.token).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !response.status().is_success() {
        anyhow::bail!("startup status request rejected")
    }
    Ok(Some(response.json().await?))
}

pub async fn wait_for_ready(
    client: &reqwest::Client,
    version: &str,
    timeout: std::time::Duration,
    interval: std::time::Duration,
) -> Result<(), uc_daemon_process::contract::DaemonBootstrapError> {
    wait_for_ready_with(
        || crate::daemon_probe::probe_daemon_health(client, version),
        || read_startup_status(client),
        version,
        timeout,
        interval,
    )
    .await
}

async fn wait_for_ready_with<P, PF, S, SF>(
    mut probe: P,
    mut startup: S,
    version: &str,
    timeout: std::time::Duration,
    interval: std::time::Duration,
) -> Result<(), uc_daemon_process::contract::DaemonBootstrapError>
where
    P: FnMut() -> PF,
    PF: std::future::Future<
        Output = Result<
            uc_daemon_contract::probe::ProbeOutcome,
            uc_daemon_process::contract::DaemonBootstrapError,
        >,
    >,
    S: FnMut() -> SF,
    SF: std::future::Future<Output = anyhow::Result<Option<DaemonStartupStatus>>>,
{
    use uc_daemon_contract::probe::{running_daemon_is_strictly_newer, ProbeOutcome};
    use uc_daemon_process::contract::DaemonBootstrapError;
    let mut deadline = tokio::time::Instant::now() + timeout;
    loop {
        match probe().await? {
            ProbeOutcome::Compatible(_) => return Ok(()),
            ProbeOutcome::Incompatible { details, .. } => {
                return Err(DaemonBootstrapError::IncompatibleDaemon { details })
            }
            ProbeOutcome::Absent => {}
        }
        if let Some(status) = startup().await.map_err(DaemonBootstrapError::Probe)? {
            if running_daemon_is_strictly_newer(Some(&status.package_version), version) {
                return Err(DaemonBootstrapError::RefusedNewerDaemon {
                    observed: status.package_version,
                    expected: version.to_owned(),
                });
            }
            if status.is_failed() {
                return Err(DaemonBootstrapError::Probe(anyhow::anyhow!(
                    "daemon startup did not complete; see startup status"
                )));
            }
            // A responding startup owner is not a failed health check. The UI shows its progress.
            deadline = tokio::time::Instant::now() + timeout;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(DaemonBootstrapError::StartupTimeout {
                timeout_ms: timeout.as_millis() as u64,
            });
        }
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use uc_daemon_contract::{probe::ProbeOutcome, startup::*};
    fn status(state: StartupStateDto) -> DaemonStartupStatus {
        DaemonStartupStatus {
            package_version: "1.0.0".into(),
            service_ready: false,
            service_failed: false,
            progress: StartupSnapshotDto {
                attempt_id: "test".into(),
                sequence: 1,
                state,
                elapsed_ms: 0,
                upgrade: None,
                failure: None,
                allowed_actions: StartupActionsDto {
                    retry: false,
                    export_diagnostics: true,
                },
            },
        }
    }
    #[tokio::test(start_paused = true)]
    async fn long_upgrade_waits_for_business_readiness() {
        let started = tokio::time::Instant::now();
        wait_for_ready_with(
            || async {
                if started.elapsed() < Duration::from_secs(147) {
                    Ok(ProbeOutcome::Absent)
                } else {
                    Ok(ProbeOutcome::Compatible(
                        uc_daemon_contract::api::types::HealthResponse {
                            status: "ok".into(),
                            degraded_reason: None,
                            package_version: "1.0.0".into(),
                            api_revision: String::new(),
                            residency: uc_daemon_contract::api::types::DaemonResidency::Standalone,
                        },
                    ))
                }
            },
            || async { Ok(Some(status(StartupStateDto::Upgrading))) },
            "1.0.0",
            Duration::from_secs(45),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(started.elapsed(), Duration::from_secs(147));
    }
    #[tokio::test(start_paused = true)]
    async fn absent_startup_still_times_out() {
        let result = wait_for_ready_with(
            || async { Ok(ProbeOutcome::Absent) },
            || async { Ok(None) },
            "1.0.0",
            Duration::from_secs(45),
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(
            result,
            Err(uc_daemon_process::contract::DaemonBootstrapError::StartupTimeout { .. })
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn interrupted_startup_is_terminal_without_waiting() {
        let started = tokio::time::Instant::now();
        let result = wait_for_ready_with(
            || async { Ok(ProbeOutcome::Absent) },
            || async { Ok(Some(status(StartupStateDto::Interrupted))) },
            "1.0.0",
            Duration::from_secs(45),
            Duration::from_secs(1),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(started.elapsed(), Duration::ZERO);
    }
}
