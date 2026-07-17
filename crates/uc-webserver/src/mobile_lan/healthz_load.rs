//! Load benchmark: `/healthz` vs `/SyncClipboard.json` under probe-like load.
//!
//! This is the evidence behind the claim that `/SyncClipboard.json` cannot
//! serve as a recovery probe while `/healthz` can. It is `#[ignore]`d — it
//! takes a minute and saturates cores by design, so it does not belong in the
//! default test run.
//!
//! ## Running it
//!
//! ```bash
//! cargo test -p uc-webserver --release --lib -- \
//!     --ignored --nocapture healthz_load_vs_sync_clipboard_json
//! ```
//!
//! `--release` is not optional for a quotable number. The workspace already
//! forces `opt-level = 3` on the `argon2` package in dev builds (see the root
//! `Cargo.toml`), so the control arm is roughly honest either way, but axum,
//! hyper and the JSON path around it are not — a debug run overstates
//! `/healthz`'s cost and muddies the comparison.
//!
//! ## What it measures, and what it cannot
//!
//! Both arms run against a real listener over real TCP with a real connection
//! pool. The control arm uses the real Argon2id hasher at production
//! parameters (m=64MB, t=3, p=4), so its cost is the cost a deployed daemon
//! actually pays per probe.
//!
//! CPU is reported as total process CPU time (`getrusage`) across the run and
//! as mean utilisation over wall-clock, not as an instantaneous peak. Peak
//! sampling needs a sampling thread and reports whatever the scheduler happened
//! to be doing at each tick; total CPU time is deterministic, reproducible, and
//! is the quantity that actually matters here — "how much CPU did answering
//! these probes burn". The PRD's "no sustained business CPU" is read as
//! utilisation over the run.
//!
//! The numbers are whole-process, so both arms include the load generator's own
//! CPU. That inflates `/healthz`'s figure (where the client is a large share of
//! the work) and is negligible for the control arm — which biases *against*
//! the result being argued, so it is left uncorrected rather than papered over.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::mobile_lan::server::start_mobile_lan_server;
use crate::mobile_lan::test_support::{
    build_facade_with_real_argon2, build_facade_with_seeded_device, fake_sse_source,
};

const USERNAME: &str = "mobile_alice";
const PASSWORD: &str = "wonderland";

/// PRD "验证预算": 20 concurrent connections, 30 seconds.
const CONCURRENCY: usize = 20;
const DURATION: Duration = Duration::from_secs(30);

/// One arm's measurements. Both arms run the same duration, so the control
/// arm simply produces far fewer samples — that asymmetry is the finding, not
/// a defect to correct for.
struct ArmResult {
    label: &'static str,
    total: u64,
    errors: u64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    cpu_seconds: f64,
    wall_seconds: f64,
}

impl ArmResult {
    fn cpu_utilisation_pct(&self) -> f64 {
        (self.cpu_seconds / self.wall_seconds) * 100.0
    }

    fn rps(&self) -> f64 {
        self.total as f64 / self.wall_seconds
    }

    fn report(&self) {
        println!(
            "\n── {} ──\n\
             requests      : {} ({:.0} req/s)\n\
             errors        : {}\n\
             latency p50   : {:.3} ms\n\
             latency p95   : {:.3} ms\n\
             latency p99   : {:.3} ms\n\
             process CPU   : {:.2} s over {:.1} s wall ({:.1}% of one core)",
            self.label,
            self.total,
            self.rps(),
            self.errors,
            self.p50_ms,
            self.p95_ms,
            self.p99_ms,
            self.cpu_seconds,
            self.wall_seconds,
            self.cpu_utilisation_pct(),
        );
    }
}

/// Total process CPU time (user + system) via `getrusage(RUSAGE_SELF)`.
#[cfg(unix)]
fn process_cpu_seconds() -> f64 {
    // SAFETY: `getrusage` writes a plain POD struct through the pointer and
    // does not retain it. The zeroed value is a valid `rusage`.
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) != 0 {
            return f64::NAN;
        }
        let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1_000_000.0;
        secs(usage.ru_utime) + secs(usage.ru_stime)
    }
}

#[cfg(not(unix))]
fn process_cpu_seconds() -> f64 {
    f64::NAN
}

fn percentile(sorted_micros: &[u64], p: f64) -> f64 {
    if sorted_micros.is_empty() {
        return f64::NAN;
    }
    let rank = (p / 100.0 * (sorted_micros.len() - 1) as f64).round() as usize;
    sorted_micros[rank] as f64 / 1000.0
}

/// Drive one endpoint with `CONCURRENCY` workers for `DURATION`, sharing a
/// connection pool so the workers behave like persistent probe connections
/// rather than a connect storm.
async fn run_arm(
    label: &'static str,
    url: String,
    auth: Option<(String, String)>,
    expect_status: u16,
) -> ArmResult {
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(CONCURRENCY)
        .build()
        .expect("building the load client must succeed");

    let latencies = Arc::new(std::sync::Mutex::new(Vec::<u64>::with_capacity(100_000)));
    let errors = Arc::new(AtomicU64::new(0));

    let cpu_before = process_cpu_seconds();
    let started = Instant::now();

    let mut workers = Vec::with_capacity(CONCURRENCY);
    for _ in 0..CONCURRENCY {
        let client = client.clone();
        let url = url.clone();
        let auth = auth.clone();
        let latencies = latencies.clone();
        let errors = errors.clone();

        workers.push(tokio::spawn(async move {
            let mut local = Vec::with_capacity(8192);
            while started.elapsed() < DURATION {
                let mut req = client.get(&url);
                if let Some((u, p)) = &auth {
                    req = req.basic_auth(u, Some(p));
                }
                let at = Instant::now();
                match req.send().await {
                    Ok(resp) if resp.status().as_u16() == expect_status => {
                        // Drain the body so the connection returns to the pool
                        // — a dropped un-drained body forces a reconnect and
                        // would silently turn this into a connect benchmark.
                        let _ = resp.bytes().await;
                        local.push(at.elapsed().as_micros() as u64);
                    }
                    Ok(_) | Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            latencies.lock().unwrap().extend(local);
        }));
    }

    for w in workers {
        w.await.expect("load worker must not panic");
    }

    let wall_seconds = started.elapsed().as_secs_f64();
    let cpu_seconds = process_cpu_seconds() - cpu_before;

    let mut sorted = Arc::try_unwrap(latencies)
        .expect("all workers have finished")
        .into_inner()
        .unwrap();
    sorted.sort_unstable();

    ArmResult {
        label,
        total: sorted.len() as u64,
        errors: errors.load(Ordering::Relaxed),
        p50_ms: percentile(&sorted, 50.0),
        p95_ms: percentile(&sorted, 95.0),
        p99_ms: percentile(&sorted, 99.0),
        cpu_seconds,
        wall_seconds,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "load benchmark: ~60s and saturates cores; run explicitly with --ignored --nocapture"]
async fn healthz_load_vs_sync_clipboard_json() {
    // Bail before spending 60s producing nothing. `process_cpu_seconds` has no
    // non-Unix implementation, so every CPU figure would be NaN — and the final
    // assertion, comparing NaN against NaN, fails with a message blaming a
    // regression in /healthz. A misleading diagnosis is worse than no run, so
    // say plainly what is missing instead.
    //
    // This is a runtime `cfg!` rather than `#[cfg(unix)]` on the test: gating
    // the attribute would leave `run_arm` and `ArmResult` dead on Windows and
    // trade this problem for a wall of warnings.
    if cfg!(not(unix)) {
        println!(
            "\n=== mobile LAN /healthz load benchmark ===\n\
             SKIPPED on {}: CPU accounting needs getrusage(2), which this\n\
             benchmark only implements for Unix. Run it on macOS or Linux.",
            std::env::consts::OS,
        );
        return;
    }

    println!(
        "\n=== mobile LAN /healthz load benchmark ===\n\
         host          : {} / {} logical cores\n\
         profile       : {}\n\
         config        : {} concurrent workers x {}s per arm",
        std::env::consts::OS,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0),
        if cfg!(debug_assertions) {
            "debug (numbers are NOT quotable — rerun with --release)"
        } else {
            "release"
        },
        CONCURRENCY,
        DURATION.as_secs(),
    );

    // ── Arm 1: /healthz, no auth, on a listener with the cheap fake hasher.
    // The hasher is irrelevant here by construction: the route never reaches
    // it. That is the property under test.
    let cancel = CancellationToken::new();
    let facade = build_facade_with_seeded_device(USERNAME, PASSWORD).await;
    let handle = start_mobile_lan_server(
        "127.0.0.1:0".parse().unwrap(),
        cancel.clone(),
        facade,
        None,
        fake_sse_source(),
    )
    .await
    .expect("health listener must bind");
    let health_url = format!("http://{}/healthz", handle.bound_addr);

    let health = run_arm(
        "GET /healthz (public, constant cost)",
        health_url,
        None,
        204,
    )
    .await;
    cancel.cancel();
    let _ = handle.join_handle.await;

    // ── Arm 2: /SyncClipboard.json, real Basic Auth, real Argon2id.
    let cancel = CancellationToken::new();
    let facade = build_facade_with_real_argon2(USERNAME, PASSWORD).await;
    let handle = start_mobile_lan_server(
        "127.0.0.1:0".parse().unwrap(),
        cancel.clone(),
        facade,
        None,
        fake_sse_source(),
    )
    .await
    .expect("sync listener must bind");
    let sync_url = format!("http://{}/SyncClipboard.json", handle.bound_addr);

    let sync = run_arm(
        "GET /SyncClipboard.json (Basic Auth + Argon2id m=64MB,t=3,p=4)",
        sync_url,
        Some((USERNAME.to_string(), PASSWORD.to_string())),
        200,
    )
    .await;
    cancel.cancel();
    let _ = handle.join_handle.await;

    health.report();
    sync.report();

    let health_cpu_per_req = health.cpu_seconds / health.total.max(1) as f64;
    let sync_cpu_per_req = sync.cpu_seconds / sync.total.max(1) as f64;

    println!(
        "\n── comparison ──\n\
         throughput    : /healthz sustained {:.0}x the request rate\n\
         CPU per req   : {:.4} ms vs {:.1} ms  ({:.0}x cheaper)\n\
         p99 latency   : {:.3} ms vs {:.1} ms",
        health.rps() / sync.rps().max(f64::MIN_POSITIVE),
        health_cpu_per_req * 1000.0,
        sync_cpu_per_req * 1000.0,
        sync_cpu_per_req / health_cpu_per_req,
        health.p99_ms,
        sync.p99_ms,
    );

    // Both arms above run flat out, which answers "how much can it take" but
    // not the question the PRD actually asks: what a *recovery probe* costs a
    // half-available daemon. The PRD's worst case is 2 candidate URLs at a 2s
    // cadence — 1 req/s, sustained for as long as the daemon stays reachable
    // but broken. Extrapolating per-request CPU to that rate is what decides
    // whether the probe is background noise or a second workload.
    const PROBE_RPS: f64 = 1.0;
    println!(
        "\n── extrapolated to the PRD's probe cadence (2 URLs @ 2s = {PROBE_RPS:.0} req/s) ──\n\
         /healthz            : {:.4}% of one core, sustained\n\
         /SyncClipboard.json : {:.1}% of one core, sustained\n\
         \n\
         Caveat: both figures are whole-process and include the load generator\n\
         itself, which inflates /healthz (where the client is most of the work)\n\
         and is negligible for the Argon2 arm — i.e. the gap is understated.\n",
        health_cpu_per_req * PROBE_RPS * 100.0,
        sync_cpu_per_req * PROBE_RPS * 100.0,
    );

    assert_eq!(health.errors, 0, "/healthz must not error under probe load");

    // The claim under test is a cost *class* difference, not a tuned margin:
    // constant-cost handler vs memory-hard KDF. An order of magnitude is a
    // floor low enough that machine variance cannot produce a false pass, and
    // any regression that re-introduces auth on this path would blow straight
    // through it.
    assert!(
        sync_cpu_per_req > health_cpu_per_req * 10.0,
        "/SyncClipboard.json should cost at least 10x /healthz per request \
         (got {sync_cpu_per_req:.6}s vs {health_cpu_per_req:.6}s) — if this \
         fails, either /healthz grew a business dependency or the control arm \
         stopped running real Argon2"
    );
}
