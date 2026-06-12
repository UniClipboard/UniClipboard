use std::fmt;
use std::future::Future;
use std::time::Duration;

use reqwest::Client;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::types::{DaemonResidency, HealthResponse};
use uc_daemon_contract::probe::{
    classify_health_response, running_daemon_is_strictly_newer, ProbeOutcome,
};
use uc_daemon_process::process_metadata::DaemonSpawnOrigin;
use uc_daemon_process::socket::try_resolve_daemon_http_addr;
use uc_daemon_process::spawn::{spawn_detached_daemon, SpawnDaemonError};

const HEALTH_PATH: &str = "/health";
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Total budget to wait for a spawned daemon to become healthy. Derived in the
/// cross-process timing contract: covers a replacement waiting out an exiting
/// predecessor's instance-lock release AND THEN bootstrapping from scratch.
const STARTUP_TIMEOUT: Duration = uc_daemon_process::timing::DAEMON_STARTUP_TIMEOUT;

const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// ADR-008 P5-L L8d-2: how long to wait for a controlled-restart predecessor's
/// endpoint to go absent before spawning the promoted replacement.
///
/// Derived in the cross-process timing contract from the daemon-side drain
/// window (`timing::CONTROLLED_RESTART_DRAIN_TIMEOUT`): `POST
/// /lifecycle/restart` returns immediately, but the predecessor keeps
/// `/health` UP for the entire bounded drain and only then cancels — followed
/// by its normal teardown. The doubled window leaves headroom so a
/// legitimately slow drain does not hard-fail the promotion before the old
/// daemon exits.
const PROMOTE_DRAIN_TIMEOUT: Duration = uc_daemon_process::timing::PROMOTE_DRAIN_TIMEOUT;

/// The package version this CLI build expects the daemon to report. Mirrors the
/// GUI host, which passes its own shell `CARGO_PKG_VERSION` to the classifier
/// (ADR-008 P5-L L2). A daemon reporting any other version is Incompatible.
const EXPECTED_PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDaemonSession {
    pub base_url: String,
    pub spawned: bool,
}

#[derive(Debug)]
pub enum LocalDaemonError {
    ProbeClient(anyhow::Error),
    ResolveAddress(anyhow::Error),
    Probe(anyhow::Error),
    ResolveBinary(anyhow::Error),
    Spawn(anyhow::Error),
    StartupTimeout {
        timeout_ms: u64,
        profile: Option<String>,
        base_url: String,
    },
    /// ADR-008 P5-L L2: a daemon is running for this profile but reports a
    /// version/contract that does not match this CLI build. We surface a clear
    /// error instead of silently attaching (or silently spawning a competitor).
    ///
    /// `newer` distinguishes the two directions, mirroring the GUI's
    /// downgrade-rollback guard: a strictly-newer incumbent must NOT be acted
    /// against (this CLI is the stale one), whereas an older/unprovable daemon
    /// is the side that needs upgrading/restarting. L2 only reports — restart /
    /// takeover orchestration is L8.
    IncompatibleDaemon {
        details: String,
        observed_package_version: Option<String>,
        expected_package_version: String,
        newer: bool,
    },
    /// ADR-008 P5-L L8d-2: a controlled-restart promotion failed — either the
    /// typed lifecycle client could not be built from the environment, or the
    /// daemon rejected / could not service the `POST /lifecycle/restart` request.
    /// Any restart error is a hard failure (we never silently fall back to
    /// attaching to the Oneshot daemon).
    PromoteRestart(anyhow::Error),
    /// ADR-008 P5-L L8d-2: the controlled-restart predecessor's endpoint did not
    /// go absent within [`PROMOTE_DRAIN_TIMEOUT`]. The daemon is still
    /// draining/serving rather than failing to come up, so this reads
    /// differently from [`Self::StartupTimeout`] (which means a fresh daemon
    /// never became healthy). The old daemon is left running.
    PromoteDrainTimeout {
        timeout_ms: u64,
        profile: Option<String>,
        base_url: String,
    },
}

impl fmt::Display for LocalDaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProbeClient(error) => write!(
                f,
                "failed to prepare local daemon probe client for setup: {error}"
            ),
            Self::ResolveAddress(error) => {
                write!(
                    f,
                    "failed to resolve profile-aware local daemon address: {error}"
                )
            }
            Self::Probe(error) => {
                write!(f, "failed to probe local daemon health for setup: {error}")
            }
            Self::ResolveBinary(error) => {
                write!(
                    f,
                    "failed to resolve CLI executable for daemon spawn: {error}"
                )
            }
            Self::Spawn(error) => write!(f, "failed to spawn daemon process: {error}"),
            Self::StartupTimeout {
                timeout_ms,
                profile,
                base_url,
            } => {
                let profile = profile.as_deref().unwrap_or("default");
                write!(
                    f,
                    "local daemon did not become healthy within {timeout_ms}ms for profile {profile} at {base_url}"
                )
            }
            Self::IncompatibleDaemon {
                details,
                observed_package_version,
                expected_package_version,
                newer,
            } => {
                let observed = observed_package_version.as_deref().unwrap_or("unknown");
                if *newer {
                    write!(
                        f,
                        "a newer daemon (version {observed}) is already running for this profile; \
                         this CLI is {expected_package_version} — refusing to act against a newer \
                         daemon. Re-upgrade the CLI, or restart the daemon to converge ({details})"
                    )
                } else {
                    write!(
                        f,
                        "an incompatible daemon (version {observed}) is already running for this \
                         profile; this CLI expects {expected_package_version}. Stop it with \
                         `uniclip stop` and restart, or upgrade the daemon to match ({details})"
                    )
                }
            }
            Self::PromoteRestart(error) => {
                write!(
                    f,
                    "failed to promote the transient local daemon to a persistent one: {error}"
                )
            }
            Self::PromoteDrainTimeout {
                timeout_ms,
                profile,
                base_url,
            } => {
                let profile = profile.as_deref().unwrap_or("default");
                write!(
                    f,
                    "the transient local daemon did not drain and exit within {timeout_ms}ms \
                     for profile {profile} at {base_url}; promotion aborted (the old daemon is \
                     left running)"
                )
            }
        }
    }
}

impl std::error::Error for LocalDaemonError {}

impl From<SpawnDaemonError> for LocalDaemonError {
    fn from(error: SpawnDaemonError) -> Self {
        match error {
            SpawnDaemonError::ResolveBinary(error) => Self::ResolveBinary(error),
            SpawnDaemonError::Spawn(error) => Self::Spawn(error),
        }
    }
}

/// Probe-only check: classifies the daemon currently bound to this profile's
/// HTTP endpoint. Does NOT spawn a daemon process.
///
/// ADR-008 P5-L L2: this used to return a bare `bool` keyed only on
/// `status == "ok"`, which silently attached to a mismatched-version daemon.
/// It now mirrors the GUI host and returns a [`ProbeOutcome`] so callers can
/// distinguish `Compatible` / `Absent` / `Incompatible` and surface a clear
/// error on a version/contract mismatch.
pub async fn probe_running() -> Result<ProbeOutcome, LocalDaemonError> {
    let client = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| LocalDaemonError::ProbeClient(error.into()))?;
    let base_url = resolve_base_url()?;
    probe_daemon_health(&client, &base_url).await
}

/// Probe-then-reuse-or-spawn entry used by the `#[autostop]` business-command
/// seam (rewritten to `ensure_local_daemon_running_capture` by
/// `uc-cli-macros`). The background `start` path now goes through the
/// promote-aware [`ensure_or_promote_local_daemon`]; this one stays the
/// non-promoting reference path with byte-identical Compatible/Incompatible/
/// Absent semantics (the autostop machinery is dormant in this tree, so it has
/// no in-crate caller yet — hence `#[allow(dead_code)]`).
#[allow(dead_code)]
pub async fn ensure_local_daemon_running() -> Result<LocalDaemonSession, LocalDaemonError> {
    let client = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| LocalDaemonError::ProbeClient(error.into()))?;
    let base_url = resolve_base_url()?;

    // Classify the daemon (if any) already bound to this profile (ADR-008 P5-L
    // L2). Compatible → reuse it; Incompatible → clear error (do NOT spawn a
    // competitor or kill it — restart/takeover is L8); Absent → spawn below.
    match probe_daemon_health(&client, &base_url).await? {
        ProbeOutcome::Compatible(_) => {
            return Ok(LocalDaemonSession {
                base_url,
                spawned: false,
            });
        }
        ProbeOutcome::Incompatible {
            details,
            observed_package_version,
            ..
        } => {
            return Err(incompatible_daemon_error(details, observed_package_version));
        }
        ProbeOutcome::Absent => {}
    }

    spawn_and_wait_healthy(&client, &base_url).await
}

/// The action [`ensure_or_promote_local_daemon`] takes for a probed daemon
/// (ADR-008 P5-L L8d-2). Extracted as a pure, total function so the
/// branch-selection logic has a single source of truth the unit tests exercise
/// directly — rather than asserting against a hand-copied mirror that could
/// drift from the real `match`.
#[derive(Debug, PartialEq, Eq)]
enum ProbeAction {
    /// A transient `Oneshot` daemon → promote it via a controlled restart.
    Promote,
    /// An already-persistent compatible daemon → reuse it (`spawned:false`).
    Reuse,
    /// A version/contract-mismatched daemon → surface a clear error.
    Incompatible,
    /// No daemon present → spawn one.
    Spawn,
}

fn classify_probe_action(outcome: &ProbeOutcome) -> ProbeAction {
    match outcome {
        ProbeOutcome::Compatible(health) if health.residency == DaemonResidency::Oneshot => {
            ProbeAction::Promote
        }
        ProbeOutcome::Compatible(_) => ProbeAction::Reuse,
        ProbeOutcome::Incompatible { .. } => ProbeAction::Incompatible,
        ProbeOutcome::Absent => ProbeAction::Spawn,
    }
}

/// ADR-008 P5-L L8d-2: probe-then-reuse-or-promote-or-spawn entry for the
/// background `start` path. Behaviour-equivalent to
/// [`ensure_local_daemon_running`] in production (residency is never `Oneshot`,
/// so the promote branch is dead code → single probe + the same spawn tail), and
/// adds one capability: when the running daemon is a transient `Oneshot` (only
/// reachable once a Oneshot supervisor exists, P5-1), it requests a controlled
/// restart to promote it to the persistent `target` residency.
///
/// * `Compatible(Oneshot)` → [`promote_oneshot_daemon`] (controlled restart).
/// * `Compatible(other)`   → reuse the already-persistent daemon (`spawned:false`).
/// * `Incompatible`        → clear error (restart/takeover is for `Oneshot` only).
/// * `Absent`              → spawn + wait healthy (`spawned:true`).
pub async fn ensure_or_promote_local_daemon(
    target: DaemonResidency,
) -> Result<LocalDaemonSession, LocalDaemonError> {
    let client = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| LocalDaemonError::ProbeClient(error.into()))?;
    let base_url = resolve_base_url()?;

    let outcome = probe_daemon_health(&client, &base_url).await?;
    match classify_probe_action(&outcome) {
        ProbeAction::Promote => promote_oneshot_daemon(&client, &base_url, target).await,
        ProbeAction::Reuse => Ok(LocalDaemonSession {
            base_url,
            spawned: false,
        }),
        ProbeAction::Spawn => spawn_and_wait_healthy(&client, &base_url).await,
        ProbeAction::Incompatible => match outcome {
            ProbeOutcome::Incompatible {
                details,
                observed_package_version,
                ..
            } => Err(incompatible_daemon_error(details, observed_package_version)),
            // `classify_probe_action` only yields `Incompatible` for this variant.
            other => unreachable!("classify_probe_action returned Incompatible for {other:?}"),
        },
    }
}

/// ADR-008 P5-1a: spawn a transient **Oneshot** daemon and wait for it to
/// become healthy. Sets RUN_MODE_ONESHOT before spawning so the child boots
/// as a self-terminating Oneshot (the child inherits this env var, exactly
/// like `start.rs` does for --server). Used by business commands when no
/// daemon is running; does NOT probe/reuse/promote (the caller already
/// classified the daemon as Absent) and does NOT touch the start-only
/// promote path.
pub async fn spawn_oneshot_and_wait() -> Result<LocalDaemonSession, LocalDaemonError> {
    let client = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| LocalDaemonError::ProbeClient(error.into()))?;
    let base_url = resolve_base_url()?;
    std::env::set_var(
        uc_daemon_process::spawn_contract::RUN_MODE_ENV,
        uc_daemon_process::spawn_contract::RUN_MODE_ONESHOT,
    );
    spawn_and_wait_healthy(&client, &base_url).await
}

/// Promote a transient `Oneshot` daemon to a persistent `target` residency via a
/// controlled restart (ADR-008 P5-L L8d-2).
///
/// 1. Request `POST /lifecycle/restart` through the typed CLI lifecycle client;
///    the daemon raises quiescing, drains, writes the handover record, and
///    self-terminates. Any restart error is a hard failure (no silent fallback).
/// 2. Wait for the old daemon's endpoint to go absent (it has exited).
/// 3. Spawn a fresh detached daemon. The handover record the old daemon wrote at
///    terminate drives the new daemon's run mode, so we do NOT pass the target
///    explicitly (`spawn_detached_daemon` reads it from the handover).
/// 4. Wait until the new daemon is healthy AND reports the requested `target`
///    residency before returning `spawned:true`.
///
/// Dead code in production: nothing spawns a `Oneshot` daemon until P5-1, so this
/// is never reached (residency is always `Standalone`/`ServerHeadless`).
async fn promote_oneshot_daemon(
    client: &Client,
    base_url: &str,
    target: DaemonResidency,
) -> Result<LocalDaemonSession, LocalDaemonError> {
    // Spinner spans the whole promotion — including the restart round-trip — so
    // the user sees progress from the first network call, not just from the
    // drain wait. Every early return below finishes it.
    let spinner = crate::ui::spinner("Promoting daemon…");

    // (1) Request the controlled restart through the typed lifecycle client.
    let ctx = match uc_daemon_client::DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(error) => {
            crate::ui::spinner_finish_error(&spinner, "Failed to reach local daemon for promotion");
            return Err(LocalDaemonError::PromoteRestart(error));
        }
    };
    let accepted = match ctx.lifecycle_client().restart(target).await {
        Ok(accepted) => accepted,
        Err(error) => {
            crate::ui::spinner_finish_error(&spinner, "Daemon rejected the promotion request");
            return Err(LocalDaemonError::PromoteRestart(error));
        }
    };
    tracing::info!(
        generation = accepted.generation,
        ?target,
        "controlled restart accepted; promoting daemon"
    );

    // (2) Wait for the old daemon to exit (endpoint goes absent).
    let mut probe = || probe_daemon_health(client, base_url);
    if let Err(error) =
        wait_for_endpoint_absent(&mut probe, PROMOTE_DRAIN_TIMEOUT, POLL_INTERVAL, base_url).await
    {
        crate::ui::spinner_finish_error(&spinner, "Daemon did not drain for promotion");
        return Err(error);
    }

    // (3) Spawn the replacement. The handover record written by the old daemon at
    // terminate selects the run mode — do NOT pass the target explicitly.
    if let Err(error) =
        spawn_detached_daemon(DaemonSpawnOrigin::Cli, None).map_err(LocalDaemonError::from)
    {
        crate::ui::spinner_finish_error(&spinner, "Failed to spawn promoted daemon");
        return Err(error);
    }

    // (4) Wait until the replacement is healthy AND reports the requested target
    // residency (not merely "any Compatible") before claiming success.
    let mut probe = || probe_daemon_health(client, base_url);
    match wait_for_daemon_health(
        &mut probe,
        STARTUP_TIMEOUT,
        POLL_INTERVAL,
        base_url,
        Some(target),
    )
    .await
    {
        Ok(()) => {
            crate::ui::spinner_finish_success(&spinner, "Daemon promoted");
            Ok(LocalDaemonSession {
                base_url: base_url.into(),
                spawned: true,
            })
        }
        Err(error) => {
            crate::ui::spinner_finish_error(&spinner, "Promoted daemon failed to start");
            Err(error)
        }
    }
}

/// Slow path shared by the Absent arms of [`ensure_local_daemon_running`] and
/// [`ensure_or_promote_local_daemon`]: spawn a detached daemon and wait for it to
/// become healthy. Returns `spawned:true`. Show a spinner so the user sees
/// progress — daemon cold start can take many seconds in debug builds.
async fn spawn_and_wait_healthy(
    client: &Client,
    base_url: &str,
) -> Result<LocalDaemonSession, LocalDaemonError> {
    let spinner = crate::ui::spinner("Starting local daemon…");

    if let Err(error) =
        spawn_detached_daemon(DaemonSpawnOrigin::Cli, None).map_err(LocalDaemonError::from)
    {
        crate::ui::spinner_finish_error(&spinner, "Failed to spawn local daemon");
        return Err(error);
    }
    // After `spawn_daemon_process` returns, the daemon is its own session
    // leader / process group — the CLI is no longer holding a wait-able
    // handle. The probe loop below is the only proof of life.

    let mut probe = || probe_daemon_health(client, base_url);
    match wait_for_daemon_health(&mut probe, STARTUP_TIMEOUT, POLL_INTERVAL, base_url, None).await {
        Ok(()) => {
            crate::ui::spinner_finish_success(&spinner, "Local daemon ready");
            Ok(LocalDaemonSession {
                base_url: base_url.into(),
                spawned: true,
            })
        }
        Err(error) => {
            crate::ui::spinner_finish_error(&spinner, "Local daemon failed to start");
            Err(error)
        }
    }
}

/// Poll `/health` until the daemon reports Compatible, or time out, or observe
/// an Incompatible daemon.
///
/// `expected_residency` is `None` for the plain spawn path (any `Compatible`
/// daemon is "ready" — byte-identical to the pre-L8d-2 behaviour) and
/// `Some(target)` for the controlled-restart promotion path, where a `Compatible`
/// daemon that still reports a DIFFERENT residency is treated like `Absent`
/// (keep polling) until the freshly-spawned daemon comes up in the requested
/// `target` residency (ADR-008 P5-L L8d-2).
async fn wait_for_daemon_health<Probe, ProbeFuture>(
    probe: &mut Probe,
    startup_timeout: Duration,
    poll_interval: Duration,
    base_url: &str,
    expected_residency: Option<DaemonResidency>,
) -> Result<(), LocalDaemonError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, LocalDaemonError>>,
{
    let deadline = tokio::time::Instant::now() + startup_timeout;
    loop {
        // ADR-008 P5-L L2: Compatible → healthy (done); Absent → daemon still
        // coming up, keep polling; Incompatible → a mismatched daemon raced onto
        // our port, surface a clear error instead of spinning until timeout.
        //
        // L8d-2: when `expected_residency` is set, a Compatible daemon whose
        // residency does not yet match is the old daemon (or a transitional
        // state); keep polling until the promoted daemon reports `target`.
        match probe().await? {
            ProbeOutcome::Compatible(health) => match expected_residency {
                None => return Ok(()),
                Some(expected) if health.residency == expected => return Ok(()),
                Some(_) => {}
            },
            ProbeOutcome::Incompatible {
                details,
                observed_package_version,
                ..
            } => {
                return Err(incompatible_daemon_error(details, observed_package_version));
            }
            ProbeOutcome::Absent => {}
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(LocalDaemonError::StartupTimeout {
                timeout_ms: startup_timeout.as_millis() as u64,
                profile: std::env::var("UC_PROFILE").ok(),
                base_url: base_url.to_string(),
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Poll `/health` until the daemon endpoint goes absent (the old daemon has
/// exited), or time out (ADR-008 P5-L L8d-2).
///
/// Mirrors [`wait_for_daemon_health`] but inverts the verdict: `Absent` → done;
/// `Compatible`/`Incompatible` → the predecessor is still up, keep polling;
/// deadline exceeded → [`LocalDaemonError::PromoteDrainTimeout`] (the predecessor
/// is draining/serving, NOT failing to start). Used by the controlled-restart
/// promotion to confirm the predecessor exited before spawning its replacement.
/// (Reference semantics mirror
/// `uc_daemon_local::health_wait::wait_for_endpoint_absent`; it is NOT imported —
/// uc-cli must not add new uc-daemon-local uses, P5-4.)
async fn wait_for_endpoint_absent<Probe, ProbeFuture>(
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
    base_url: &str,
) -> Result<(), LocalDaemonError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, LocalDaemonError>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match probe().await? {
            ProbeOutcome::Absent => return Ok(()),
            ProbeOutcome::Compatible(_) | ProbeOutcome::Incompatible { .. } => {}
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(LocalDaemonError::PromoteDrainTimeout {
                timeout_ms: timeout.as_millis() as u64,
                profile: std::env::var("UC_PROFILE").ok(),
                base_url: base_url.to_string(),
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Build the [`LocalDaemonError::IncompatibleDaemon`] for an `Incompatible`
/// outcome, applying the GUI's strictly-newer downgrade guard to phrase the
/// error in the right direction (ADR-008 P5-L L2).
fn incompatible_daemon_error(
    details: String,
    observed_package_version: Option<String>,
) -> LocalDaemonError {
    let newer = running_daemon_is_strictly_newer(
        observed_package_version.as_deref(),
        EXPECTED_PACKAGE_VERSION,
    );
    LocalDaemonError::IncompatibleDaemon {
        details,
        observed_package_version,
        expected_package_version: EXPECTED_PACKAGE_VERSION.to_string(),
        newer,
    }
}

/// Convert an `Incompatible` [`ProbeOutcome`] into the matching
/// [`LocalDaemonError::IncompatibleDaemon`] so probe-only consumers
/// (`refuse_if_daemon_running`, `resolve_execution_mode`) can render one
/// consistent, direction-aware message (ADR-008 P5-L L2).
///
/// # Panics
/// Panics if called with a non-`Incompatible` outcome — callers must only reach
/// this on the `Incompatible` arm.
pub(crate) fn incompatible_outcome_error(outcome: ProbeOutcome) -> LocalDaemonError {
    match outcome {
        ProbeOutcome::Incompatible {
            details,
            observed_package_version,
            ..
        } => incompatible_daemon_error(details, observed_package_version),
        other => {
            unreachable!("incompatible_outcome_error called on non-Incompatible outcome: {other:?}")
        }
    }
}

/// Probe `/health` and classify the running daemon (ADR-008 P5-L L2).
///
/// Connect/timeout errors map to [`ProbeOutcome::Absent`] (no daemon to talk
/// to); a non-2xx response or an undecodable / version-mismatched body maps to
/// [`ProbeOutcome::Incompatible`]; a healthy, version-matched daemon maps to
/// [`ProbeOutcome::Compatible`]. The version/contract decision is delegated to
/// the shared `uc-daemon-contract::probe::classify_health_response` so the CLI
/// and GUI host classify byte-identically.
async fn probe_daemon_health(
    client: &Client,
    base_url: &str,
) -> Result<ProbeOutcome, LocalDaemonError> {
    let url = format!("{base_url}{HEALTH_PATH}");
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(ProbeOutcome::Absent),
        Err(error) => {
            return Err(LocalDaemonError::Probe(
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

    // Wire shape (ADR-008 §H): `/health` is enveloped as
    // `{ data: HealthResponse, ts }`. Decode the envelope, take `.data`, then
    // hand it to the shared classifier. A body we cannot decode is a daemon on
    // an incompatible contract, not a transport failure → Incompatible.
    let body = match response.text().await {
        Ok(body) => body,
        Err(error) => {
            return Err(LocalDaemonError::Probe(
                anyhow::Error::new(error).context("failed to read daemon health response body"),
            ))
        }
    };

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

    Ok(classify_health_response(health, EXPECTED_PACKAGE_VERSION))
}

fn resolve_base_url() -> Result<String, LocalDaemonError> {
    let addr = try_resolve_daemon_http_addr().map_err(|error| {
        LocalDaemonError::ResolveAddress(
            error.context("failed to resolve profile-aware daemon HTTP address"),
        )
    })?;
    Ok(format!("http://{}:{}", addr.ip(), addr.port()))
}

/// Detached daemon spawn + binary resolution now live in the shared
/// [`uc_daemon_process::spawn`] module so GUI shells (ADR-008 P3) reuse the exact
/// same `setsid` / `DETACHED_PROCESS` detach semantics. The CLI keeps only the
/// probe→spawn→wait-health orchestration above.

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use uc_daemon_contract::api::types::DaemonResidency;

    /// A `Compatible` outcome carrying a minimal healthy payload — the probe
    /// loop only cares about the variant, not the payload contents.
    fn compatible() -> ProbeOutcome {
        compatible_with_residency(DaemonResidency::Standalone)
    }

    /// A `Compatible` outcome reporting a specific residency — used by the
    /// L8d-2 residency-aware wait tests.
    fn compatible_with_residency(residency: DaemonResidency) -> ProbeOutcome {
        ProbeOutcome::Compatible(HealthResponse {
            status: "ok".into(),
            package_version: EXPECTED_PACKAGE_VERSION.into(),
            api_revision: uc_daemon_contract::DAEMON_API_REVISION.into(),
            residency,
        })
    }

    // ---------- Display impl ----------

    #[test]
    fn display_startup_timeout_includes_profile_and_url() {
        let err = LocalDaemonError::StartupTimeout {
            timeout_ms: 30_000,
            profile: Some("dev".into()),
            base_url: "http://127.0.0.1:7321".into(),
        };
        let s = err.to_string();
        assert!(s.contains("30000"), "must include timeout in ms: {s}");
        assert!(s.contains("dev"), "must include profile name: {s}");
        assert!(
            s.contains("http://127.0.0.1:7321"),
            "must include base URL: {s}"
        );
    }

    #[test]
    fn display_startup_timeout_falls_back_to_default_profile_label() {
        let err = LocalDaemonError::StartupTimeout {
            timeout_ms: 1_000,
            profile: None,
            base_url: "http://localhost:9".into(),
        };
        let s = err.to_string();
        assert!(
            s.contains("default"),
            "missing profile must surface as 'default' label, not blank: {s}"
        );
    }

    #[test]
    fn display_passthrough_for_anyhow_wrapped_variants() {
        let err = LocalDaemonError::Probe(anyhow::anyhow!("connection refused"));
        let s = err.to_string();
        assert!(s.contains("connection refused"));
        assert!(s.contains("probe"), "Probe variant must self-identify: {s}");
    }

    // ---------- LocalDaemonSession PartialEq ----------

    #[test]
    fn session_partial_eq_distinguishes_spawned_flag() {
        let a = LocalDaemonSession {
            base_url: "http://1.2.3.4:5".into(),
            spawned: true,
        };
        let b = LocalDaemonSession {
            base_url: "http://1.2.3.4:5".into(),
            spawned: false,
        };
        assert_ne!(a, b, "spawned=true vs spawned=false must compare unequal");
    }

    // ---------- wait_for_daemon_health ----------

    #[tokio::test]
    async fn wait_returns_immediately_on_first_healthy_probe() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ProbeOutcome, LocalDaemonError>(compatible())
            }
        };

        wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
            None,
        )
        .await
        .expect("first Compatible probe must resolve as Ok");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "must short-circuit on first healthy probe — wastes startup time otherwise"
        );
    }

    #[tokio::test]
    async fn wait_polls_until_probe_turns_healthy() {
        // Simulate cold start: first 2 probes Absent (daemon still spawning),
        // then Compatible.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ProbeOutcome, LocalDaemonError>(if n >= 2 {
                    compatible()
                } else {
                    ProbeOutcome::Absent
                })
            }
        };

        wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
            None,
        )
        .await
        .expect("eventually-healthy probe must resolve");
        assert!(calls.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test]
    async fn wait_errors_immediately_on_incompatible_probe() {
        // ADR-008 P5-L L2: if an incompatible daemon races onto our port while
        // we are waiting for our spawn to come up, surface a clear error rather
        // than spinning until the startup timeout.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ProbeOutcome, LocalDaemonError>(ProbeOutcome::Incompatible {
                    details: "version mismatch".into(),
                    observed_package_version: Some("9.9.9".into()),
                    observed_api_revision: None,
                })
            }
        };

        let err = wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
            None,
        )
        .await
        .expect_err("Incompatible must short-circuit to an error");

        assert!(matches!(err, LocalDaemonError::IncompatibleDaemon { .. }));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Incompatible is terminal — must not keep polling"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn wait_times_out_with_full_diagnostic_context() {
        // start_paused freezes wall clock so the test doesn't actually wait.
        // env var UC_PROFILE is captured into the timeout error — set it to
        // assert it propagates.
        // SAFETY: tests run with `--test-threads=1` for `set_var`/`remove_var` to be safe.
        // In Rust 2024 edition, std::env::set_var is unsafe; this crate is on edition 2021.
        std::env::set_var("UC_PROFILE", "ci-profile");
        let mut probe = || async { Ok::<ProbeOutcome, LocalDaemonError>(ProbeOutcome::Absent) };

        let err = wait_for_daemon_health(
            &mut probe,
            Duration::from_millis(500),
            Duration::from_millis(50),
            "http://example:1234",
            None,
        )
        .await
        .expect_err("never-healthy probe must produce StartupTimeout");

        std::env::remove_var("UC_PROFILE");

        match err {
            LocalDaemonError::StartupTimeout {
                timeout_ms,
                profile,
                base_url,
            } => {
                assert_eq!(timeout_ms, 500);
                assert_eq!(profile.as_deref(), Some("ci-profile"));
                assert_eq!(base_url, "http://example:1234");
            }
            other => panic!("expected StartupTimeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wait_propagates_probe_error_without_retry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<ProbeOutcome, _>(LocalDaemonError::Probe(anyhow::anyhow!("network down")))
            }
        };

        let err = wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
            None,
        )
        .await
        .expect_err("probe error must propagate, not be retried");

        assert!(matches!(err, LocalDaemonError::Probe(_)));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "transport-level errors are not transient — wait must not retry"
        );
    }

    // ---------- residency-aware wait_for_daemon_health (L8d-2) ----------

    #[tokio::test]
    async fn wait_returns_on_compatible_with_matching_residency() {
        // Promotion path: the very first probe already reports the requested
        // target residency → resolve immediately.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ProbeOutcome, LocalDaemonError>(compatible_with_residency(
                    DaemonResidency::ServerHeadless,
                ))
            }
        };

        wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
            Some(DaemonResidency::ServerHeadless),
        )
        .await
        .expect("matching residency must resolve");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn wait_keeps_polling_on_compatible_with_mismatched_residency() {
        // Promotion path: the old Oneshot daemon (or a transitional state) is
        // still answering with the WRONG residency. The wait must keep polling —
        // NOT short-circuit as healthy — until the promoted daemon reports the
        // requested target residency.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ProbeOutcome, LocalDaemonError>(if n >= 2 {
                    compatible_with_residency(DaemonResidency::Standalone)
                } else {
                    // Old daemon still reporting the pre-promotion residency.
                    compatible_with_residency(DaemonResidency::Oneshot)
                })
            }
        };

        wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
            Some(DaemonResidency::Standalone),
        )
        .await
        .expect("must resolve once residency matches the target");
        assert!(
            calls.load(Ordering::SeqCst) >= 3,
            "must keep polling while residency mismatches"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn wait_times_out_when_residency_never_matches() {
        // Promotion path: a Compatible daemon that NEVER converges to the
        // requested residency must time out, not hang forever or resolve early.
        let mut probe = || async {
            Ok::<ProbeOutcome, LocalDaemonError>(compatible_with_residency(
                DaemonResidency::Oneshot,
            ))
        };

        let err = wait_for_daemon_health(
            &mut probe,
            Duration::from_millis(100),
            Duration::from_millis(10),
            "http://test",
            Some(DaemonResidency::Standalone),
        )
        .await
        .expect_err("never-matching residency must time out");

        assert!(matches!(err, LocalDaemonError::StartupTimeout { .. }));
    }

    // ---------- wait_for_endpoint_absent (L8d-2) ----------

    #[tokio::test]
    async fn wait_for_endpoint_absent_returns_on_first_absent() {
        // Drain path: the predecessor is up for a couple of probes, then exits
        // (Absent) → the waiter resolves.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ProbeOutcome, LocalDaemonError>(if n >= 2 {
                    ProbeOutcome::Absent
                } else {
                    compatible()
                })
            }
        };

        wait_for_endpoint_absent(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
        )
        .await
        .expect("Absent must resolve the waiter");
        assert!(calls.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_endpoint_absent_times_out_if_endpoint_stays_compatible() {
        // Drain path: the predecessor never exits (stays Compatible) → time out.
        let mut probe = || async { Ok::<ProbeOutcome, LocalDaemonError>(compatible()) };

        let err = wait_for_endpoint_absent(
            &mut probe,
            Duration::from_millis(100),
            Duration::from_millis(10),
            "http://test",
        )
        .await
        .expect_err("an endpoint that never goes absent must time out");

        assert!(matches!(err, LocalDaemonError::PromoteDrainTimeout { .. }));
    }

    // ---------- ensure_or_promote branch decision (L8d-2) ----------
    //
    // `ensure_or_promote_local_daemon` builds its own reqwest client + resolves
    // the base URL from env, so it is not directly callable without a daemon.
    // Instead we exercise the EXACT decision function the real entry point uses —
    // `classify_probe_action` (no hand-copied mirror): a change to its arms is a
    // change to production routing AND breaks these tests, so they cannot drift.

    #[test]
    fn ensure_or_promote_takes_promote_path_on_oneshot() {
        assert_eq!(
            classify_probe_action(&compatible_with_residency(DaemonResidency::Oneshot)),
            ProbeAction::Promote,
            "a transient Oneshot daemon must be promoted, not reused"
        );
    }

    #[test]
    fn ensure_or_promote_reuses_non_oneshot_compatible() {
        for residency in [DaemonResidency::Standalone, DaemonResidency::ServerHeadless] {
            assert_eq!(
                classify_probe_action(&compatible_with_residency(residency)),
                ProbeAction::Reuse,
                "an already-persistent daemon ({residency:?}) must be reused (spawned:false)"
            );
        }
    }

    #[test]
    fn ensure_or_promote_spawns_when_absent() {
        assert_eq!(
            classify_probe_action(&ProbeOutcome::Absent),
            ProbeAction::Spawn,
            "no daemon present must take the spawn tail"
        );
    }

    #[test]
    fn ensure_or_promote_errors_on_incompatible() {
        let outcome = ProbeOutcome::Incompatible {
            details: "bad version".into(),
            observed_package_version: Some("9.9.9".into()),
            observed_api_revision: None,
        };
        assert_eq!(
            classify_probe_action(&outcome),
            ProbeAction::Incompatible,
            "an incompatible daemon must surface an error, never be promoted"
        );
    }

    // `resolve_daemon_exe_path` moved to `uc_daemon_process::spawn`; its
    // no-panic test lives there now.
}
