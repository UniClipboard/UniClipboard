//! Process-isolated benchmark for the mobile LAN `/healthz` endpoint.
//!
//! This binary starts a real `uniclipd` profile, configures a real mobile
//! device through `uniclip`, drives both endpoints from this separate process,
//! samples only the daemon PID, and validates the daemon's exact JSON log file.
//!
//! Build the three release binaries first, then provide an absolute log path:
//!
//! ```bash
//! cargo build --release -p uc-daemon --bin uniclipd
//! cargo build --release -p uc-cli --bin uniclip
//! cargo build --release -p p2p-bench --bin mobile_healthz_process_bench
//! UC_LOG_FILE="$(pwd)/target/mobile-healthz-daemon.jsonl" \
//!   target/release/mobile_healthz_process_bench
//! ```

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, ensure, Context, Result};
use clap::Parser;
use hdrhistogram::Histogram;
use sysinfo::{Pid, ProcessesToUpdate, System, MINIMUM_CPU_UPDATE_INTERVAL};
use tokio::sync::oneshot;
use tokio::task::JoinSet;

const MOBILE_USERNAME: &str = "healthz_bench";
const MOBILE_PASSWORD: &str = "healthz-benchmark-mobile-password";
const SPACE_PASSPHRASE: &str = "healthz-benchmark-space-passphrase";
const LOG_SETTLE_RETRIES: usize = 20;
const LOG_SETTLE_DELAY: Duration = Duration::from_millis(100);
const MAX_RECORDED_LATENCY_MICROS: u64 = 60_000_000;

#[derive(Parser, Debug)]
#[command(
    name = "mobile_healthz_process_bench",
    about = "Measure /healthz and /SyncClipboard.json against a separate uniclipd process"
)]
struct Cli {
    /// Concurrent persistent HTTP workers per endpoint.
    #[arg(long, default_value_t = 20)]
    concurrency: usize,

    /// Measured duration for each endpoint.
    #[arg(long, default_value_t = 30)]
    duration_secs: u64,

    /// Daemon CPU sampling interval in milliseconds.
    #[arg(long, default_value_t = 200)]
    cpu_sample_ms: u64,

    /// Quiet period after setup before measured requests begin.
    #[arg(long, default_value_t = 5)]
    startup_settle_secs: u64,

    /// Mobile LAN port. An ephemeral free port is selected when omitted.
    #[arg(long)]
    mobile_port: Option<u16>,

    /// Isolated daemon profile. A unique profile is generated when omitted.
    #[arg(long)]
    profile: Option<String>,

    /// Path to a prebuilt release `uniclipd` binary.
    #[arg(long)]
    daemon_bin: Option<PathBuf>,

    /// Path to a prebuilt release `uniclip` binary.
    #[arg(long)]
    cli_bin: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LogCounts {
    info: u64,
    warn: u64,
    mobile_lan_info: u64,
    mobile_lan_warn: u64,
}

impl LogCounts {
    fn delta(self, before: Self) -> Result<Self> {
        Ok(Self {
            info: checked_log_delta(self.info, before.info, "daemon INFO")?,
            warn: checked_log_delta(self.warn, before.warn, "daemon WARN")?,
            mobile_lan_info: checked_log_delta(
                self.mobile_lan_info,
                before.mobile_lan_info,
                "mobile LAN INFO",
            )?,
            mobile_lan_warn: checked_log_delta(
                self.mobile_lan_warn,
                before.mobile_lan_warn,
                "mobile LAN WARN",
            )?,
        })
    }
}

fn checked_log_delta(after: u64, before: u64, label: &str) -> Result<u64> {
    after
        .checked_sub(before)
        .with_context(|| format!("{label} log count moved backwards"))
}

struct ArmResult {
    label: &'static str,
    successes: u64,
    errors: u64,
    wall_seconds: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    peak_daemon_cpu_pct: f32,
    cpu_samples: usize,
    logs: LogCounts,
}

impl ArmResult {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.errors == 0,
            "{} returned {} errors",
            self.label,
            self.errors
        );
        ensure!(
            self.successes > 0,
            "{} produced no successful samples",
            self.label
        );
        ensure!(
            self.peak_daemon_cpu_pct.is_finite(),
            "{} produced a non-finite daemon CPU peak",
            self.label
        );
        ensure!(
            self.cpu_samples > 0,
            "{} produced no daemon CPU samples",
            self.label
        );
        Ok(())
    }

    fn report(&self) {
        println!(
            "\n-- {} --\n\
             requests             : {} ({:.0} req/s)\n\
             errors               : {}\n\
             latency p50/p95/p99  : {:.3} / {:.3} / {:.3} ms\n\
             daemon peak CPU      : {:.1}% ({} samples)\n\
             all daemon log delta : INFO +{}, WARN +{}\n\
             mobile LAN log delta : INFO +{}, WARN +{}",
            self.label,
            self.successes,
            self.successes as f64 / self.wall_seconds,
            self.errors,
            self.p50_ms,
            self.p95_ms,
            self.p99_ms,
            self.peak_daemon_cpu_pct,
            self.cpu_samples,
            self.logs.info,
            self.logs.warn,
            self.logs.mobile_lan_info,
            self.logs.mobile_lan_warn,
        );
    }
}

struct CpuPeak {
    pct: f32,
    samples: usize,
}

struct DaemonChild {
    child: Child,
}

impl DaemonChild {
    fn spawn(binary: &Path, profile: &str, log_file: &Path) -> Result<Self> {
        let child = Command::new(binary)
            .env("UC_PROFILE", profile)
            .env("UC_LOG_FILE", log_file)
            .env("UC_LOG_PROFILE", "prod")
            .env("UC_PORTABLE", "1")
            .env("UNICLIPBOARD_ENV", "development")
            .env_remove("RUST_LOG")
            .env_remove("SENTRY_DSN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn daemon {}", binary.display()))?;
        Ok(Self { child })
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn ensure_running(&mut self) -> Result<()> {
        if let Some(status) = self.child.try_wait().context("query daemon status")? {
            bail!("daemon exited early with {status}");
        }
        Ok(())
    }
}

impl Drop for DaemonChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct ProfileArtifacts {
    paths: Vec<PathBuf>,
}

impl ProfileArtifacts {
    fn resolve(profile: &str) -> Result<Self> {
        let data = uc_app_paths::app_data_root()
            .context("failed to resolve isolated profile data path")?;
        let cache = uc_app_paths::app_cache_root()
            .context("failed to resolve isolated profile cache path")?;
        let expected = benchmark_binary_dir()?
            .join("data")
            .join(format!("{}-{profile}", uc_app_paths::APP_DIR_NAME));
        ensure!(
            data == expected && cache == expected,
            "refusing to clean unexpected portable profile paths: data={}, cache={}, expected={}",
            data.display(),
            cache.display(),
            expected.display()
        );
        ensure!(
            !expected.exists(),
            "benchmark profile unexpectedly already exists: {}",
            expected.display()
        );
        Ok(Self {
            paths: vec![expected],
        })
    }
}

impl Drop for ProfileArtifacts {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn exact_log_file() -> Result<PathBuf> {
    let path = std::env::var_os("UC_LOG_FILE")
        .map(PathBuf::from)
        .context("UC_LOG_FILE must name an absolute, readable daemon JSON log file")?;
    ensure!(
        path.is_absolute(),
        "UC_LOG_FILE must be absolute: {}",
        path.display()
    );
    let parent = path
        .parent()
        .context("UC_LOG_FILE must have a parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create UC_LOG_FILE parent {}", parent.display()))?;
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| {
            format!(
                "UC_LOG_FILE is not readable and writable: {}",
                path.display()
            )
        })?;
    fs::read_to_string(&path)
        .with_context(|| format!("UC_LOG_FILE is not readable UTF-8: {}", path.display()))?;
    Ok(path)
}

fn validate_profile(profile: &str) -> Result<()> {
    ensure!(!profile.is_empty(), "--profile must not be empty");
    ensure!(
        profile.len() <= 64,
        "--profile must be at most 64 ASCII characters"
    );
    ensure!(
        profile
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "--profile may contain only ASCII letters, digits, `-`, and `_`"
    );
    Ok(())
}

fn generated_profile() -> Result<String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis();
    Ok(format!("healthz-bench-{}-{timestamp}", std::process::id()))
}

fn benchmark_binary_dir() -> Result<PathBuf> {
    let executable = std::env::current_exe()
        .context("resolve benchmark executable")?
        .canonicalize()
        .context("canonicalize benchmark executable")?;
    executable
        .parent()
        .map(Path::to_path_buf)
        .context("benchmark executable has no parent")
}

fn sibling_binary(explicit: Option<PathBuf>, name: &str) -> Result<PathBuf> {
    let benchmark_dir = benchmark_binary_dir()?;
    let path = match explicit {
        Some(path) => path,
        None => {
            let suffix = if cfg!(windows) { ".exe" } else { "" };
            benchmark_dir.join(format!("{name}{suffix}"))
        }
    };
    let path = path
        .canonicalize()
        .with_context(|| format!("required binary is missing: {}", path.display()))?;
    ensure!(
        path.is_file(),
        "required binary is not a file: {}",
        path.display()
    );
    ensure!(
        path.parent() == Some(benchmark_dir.as_path()),
        "{name} must be a sibling of the benchmark binary so portable cleanup is exact: {}",
        path.display()
    );
    Ok(path)
}

fn reserve_mobile_port() -> Result<u16> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .context("reserve an ephemeral mobile LAN port")?;
    Ok(listener.local_addr().context("read reserved port")?.port())
}

fn setup_mobile_listener(binary: &Path, profile: &str, requested_port: Option<u16>) -> Result<u16> {
    let attempts = if requested_port.is_some() { 1 } else { 5 };
    let mut last_error = None;
    for _ in 0..attempts {
        let port = match requested_port {
            Some(port) => port,
            None => reserve_mobile_port()?,
        };
        let port_arg = port.to_string();
        match run_cli(
            binary,
            profile,
            &[
                "--json",
                "mobile",
                "setup",
                "--non-interactive",
                "--label",
                "HealthzBenchmark",
                "--port",
                &port_arg,
                "--username",
                MOBILE_USERNAME,
                "--password-stdin",
                "--accept-network-risk",
            ],
            Some(&format!("{MOBILE_PASSWORD}\n")),
        ) {
            Ok(output) => {
                let setup: serde_json::Value =
                    serde_json::from_slice(&output.stdout).context("parse mobile setup JSON")?;
                ensure!(
                    setup.get("username").and_then(serde_json::Value::as_str)
                        == Some(MOBILE_USERNAME),
                    "mobile setup returned an unexpected username"
                );
                return Ok(port);
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.context("mobile setup failed without an error")?)
        .context("mobile setup failed after exhausting port attempts")
}

fn run_cli(binary: &Path, profile: &str, args: &[&str], stdin: Option<&str>) -> Result<Output> {
    let mut command = Command::new(binary);
    command
        .env("UC_PROFILE", profile)
        .env("UC_PORTABLE", "1")
        .env("UNICLIPBOARD_ENV", "development")
        .env_remove("UC_LOG_FILE")
        .env_remove("RUST_LOG")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("spawn CLI {}", binary.display()))?;
    if let Some(input) = stdin {
        child
            .stdin
            .as_mut()
            .context("CLI stdin pipe is missing")?
            .write_all(input.as_bytes())
            .context("write CLI stdin")?;
    }
    let output = child.wait_with_output().context("wait for CLI command")?;
    ensure!(
        output.status.success(),
        "uniclip {:?} failed with {}: {}",
        args,
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

async fn wait_for_status(
    daemon: &mut DaemonChild,
    client: &reqwest::Client,
    url: &str,
    expected: reqwest::StatusCode,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        daemon.ensure_running()?;
        if let Ok(response) = client.get(url).send().await {
            if response.status() == expected {
                return Ok(());
            }
        }
        ensure!(
            Instant::now() < deadline,
            "timed out waiting for {url} -> {expected}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn read_log_counts(path: &Path) -> Result<LogCounts> {
    let bytes = fs::read(path).with_context(|| format!("read daemon log {}", path.display()))?;
    let text = std::str::from_utf8(&bytes)
        .with_context(|| format!("daemon log is not UTF-8: {}", path.display()))?;
    ensure!(
        text.is_empty() || text.ends_with('\n'),
        "daemon log has an incomplete trailing line"
    );

    let mut counts = LogCounts::default();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .with_context(|| format!("daemon log line {} is not JSON", index + 1))?;
        let level = value
            .get("level")
            .and_then(serde_json::Value::as_str)
            .with_context(|| format!("daemon log line {} has no string level", index + 1))?;
        let is_mobile_lan = value
            .get("target")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|target| target.starts_with("uc_webserver::mobile_lan"));
        match level {
            "INFO" => {
                counts.info += 1;
                if is_mobile_lan {
                    counts.mobile_lan_info += 1;
                }
            }
            "WARN" => {
                counts.warn += 1;
                if is_mobile_lan {
                    counts.mobile_lan_warn += 1;
                }
            }
            _ => {}
        }
    }
    Ok(counts)
}

async fn settled_log_counts(path: &Path) -> Result<LogCounts> {
    let mut previous = None;
    let mut stable_reads = 0_usize;
    let mut last_error = None;
    for _ in 0..LOG_SETTLE_RETRIES {
        match read_log_counts(path) {
            Ok(counts) => {
                last_error = None;
                let length = fs::metadata(path)
                    .with_context(|| format!("read daemon log metadata {}", path.display()))?
                    .len();
                let snapshot = (length, counts);
                if previous == Some(snapshot) {
                    stable_reads += 1;
                    if stable_reads >= 2 {
                        return Ok(counts);
                    }
                } else {
                    previous = Some(snapshot);
                    stable_reads = 0;
                }
            }
            Err(error) => {
                last_error = Some(error);
                previous = None;
                stable_reads = 0;
            }
        }
        tokio::time::sleep(LOG_SETTLE_DELAY).await;
    }
    if let Some(error) = last_error {
        return Err(error.context("daemon log remained unreadable or unstable"));
    }
    bail!("daemon log did not become stable within the settle deadline")
}

async fn one_request(
    client: &reqwest::Client,
    url: &str,
    auth: Option<(&str, &str)>,
    expected: reqwest::StatusCode,
) -> Result<u64> {
    let mut request = client.get(url);
    if let Some((username, password)) = auth {
        request = request.basic_auth(username, Some(password));
    }
    let started = Instant::now();
    let response = request.send().await.context("send request")?;
    ensure!(
        response.status() == expected,
        "{url} returned {}, expected {expected}",
        response.status()
    );
    response.bytes().await.context("drain response body")?;
    Ok(started.elapsed().as_micros() as u64)
}

async fn sample_daemon_cpu(
    raw_pid: u32,
    interval: Duration,
    mut stop: oneshot::Receiver<()>,
) -> Result<CpuPeak> {
    let pid = Pid::from_u32(raw_pid);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

    let mut peak = 0.0_f32;
    let mut samples = 0_usize;
    loop {
        let stopping = tokio::select! {
            _ = tokio::time::sleep(interval) => false,
            _ = &mut stop => true,
        };
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        let process = system
            .process(pid)
            .context("daemon PID disappeared during CPU sampling")?;
        peak = peak.max(process.cpu_usage());
        samples += 1;
        if stopping {
            break;
        }
    }
    Ok(CpuPeak { pct: peak, samples })
}

fn latency_histogram() -> Result<Histogram<u64>> {
    Histogram::new_with_bounds(1, MAX_RECORDED_LATENCY_MICROS, 3)
        .context("create bounded latency histogram")
}

fn histogram_percentile(histogram: &Histogram<u64>, quantile: f64) -> f64 {
    if histogram.is_empty() {
        return f64::NAN;
    }
    histogram.value_at_quantile(quantile) as f64 / 1000.0
}

#[allow(clippy::too_many_arguments)]
async fn run_arm(
    label: &'static str,
    client: &reqwest::Client,
    url: Arc<str>,
    auth: Option<(&'static str, &'static str)>,
    expected: reqwest::StatusCode,
    concurrency: usize,
    duration: Duration,
    daemon_pid: u32,
    cpu_interval: Duration,
    log_file: &Path,
) -> Result<ArmResult> {
    let logs_before = settled_log_counts(log_file).await?;
    let (stop_tx, stop_rx) = oneshot::channel();
    let cpu_task = tokio::spawn(sample_daemon_cpu(daemon_pid, cpu_interval, stop_rx));
    let started = Instant::now();
    let deadline = started + duration;

    let mut workers = JoinSet::new();
    for _ in 0..concurrency {
        let worker_client = client.clone();
        let worker_url = Arc::clone(&url);
        let mut latencies = latency_histogram()?;
        workers.spawn(async move {
            let mut errors = 0_u64;
            while Instant::now() < deadline {
                match one_request(&worker_client, &worker_url, auth, expected).await {
                    Ok(latency) => latencies
                        .record(latency.max(1))
                        .context("record request latency")?,
                    Err(_) => errors += 1,
                }
            }
            Ok::<_, anyhow::Error>((latencies, errors))
        });
    }

    let mut latencies = latency_histogram()?;
    let mut errors = 0_u64;
    while let Some(joined) = workers.join_next().await {
        let (worker_latencies, worker_errors) = joined.context("load worker panicked")??;
        latencies
            .add(&worker_latencies)
            .context("merge worker latency histogram")?;
        errors += worker_errors;
    }
    let wall_seconds = started.elapsed().as_secs_f64();
    let _ = stop_tx.send(());
    let cpu = cpu_task.await.context("CPU sampler task panicked")??;

    let logs = settled_log_counts(log_file).await?.delta(logs_before)?;

    Ok(ArmResult {
        label,
        successes: latencies.len(),
        errors,
        wall_seconds,
        p50_ms: histogram_percentile(&latencies, 0.50),
        p95_ms: histogram_percentile(&latencies, 0.95),
        p99_ms: histogram_percentile(&latencies, 0.99),
        peak_daemon_cpu_pct: cpu.pct,
        cpu_samples: cpu.samples,
        logs,
    })
}

fn machine_model() -> String {
    #[cfg(target_os = "macos")]
    let model = command_text("sysctl", &["-n", "hw.model"])
        .unwrap_or_else(|| "unknown Mac model".to_string());
    #[cfg(target_os = "linux")]
    let model = fs::read_to_string("/sys/devices/virtual/dmi/id/product_name")
        .ok()
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| "unknown Linux model".to_string());
    #[cfg(target_os = "windows")]
    let model = command_text(
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_ComputerSystem).Model",
        ],
    )
    .unwrap_or_else(|| "unknown Windows model".to_string());
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let model = format!("unknown {} model", std::env::consts::OS);

    let system = System::new_all();
    let cpu = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim())
        .filter(|brand| !brand.is_empty())
        .unwrap_or(std::env::consts::ARCH);
    format!("{model} / {cpu}")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn command_text(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    ensure!(
        cli.concurrency > 0,
        "--concurrency must be greater than zero"
    );
    ensure!(
        cli.duration_secs > 0,
        "--duration-secs must be greater than zero"
    );

    let cpu_interval = Duration::from_millis(cli.cpu_sample_ms);
    ensure!(
        cpu_interval >= MINIMUM_CPU_UPDATE_INTERVAL,
        "--cpu-sample-ms must be at least {} ms",
        MINIMUM_CPU_UPDATE_INTERVAL.as_millis()
    );

    let log_file = exact_log_file()?;
    let profile = match cli.profile {
        Some(profile) => profile,
        None => generated_profile()?,
    };
    validate_profile(&profile)?;
    std::env::set_var("UC_PROFILE", &profile);
    std::env::set_var("UC_PORTABLE", "1");
    let _artifacts = ProfileArtifacts::resolve(&profile)?;

    let daemon_binary = sibling_binary(cli.daemon_bin, "uniclipd")?;
    let cli_binary = sibling_binary(cli.cli_bin, "uniclip")?;
    let main_addr = uc_daemon_process::socket::try_resolve_daemon_http_addr()
        .context("resolve isolated daemon HTTP address")?;

    println!(
        "=== mobile LAN process-isolated health benchmark ===\n\
         machine            : {}\n\
         host               : {} / {} / {} logical cores\n\
         profile            : {}\n\
         daemon binary      : {}\n\
         UC_LOG_FILE        : {}\n\
         config             : {} workers x {}s per endpoint\n\
         startup settle     : {}s before measurement\n\
         CPU sampling       : daemon PID only, every {} ms (100% = one core)",
        machine_model(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |count| count.get()),
        profile,
        daemon_binary.display(),
        log_file.display(),
        cli.concurrency,
        cli.duration_secs,
        cli.startup_settle_secs,
        cli.cpu_sample_ms,
    );

    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(cli.concurrency)
        .timeout(Duration::from_secs(10))
        .build()
        .context("build benchmark HTTP client")?;
    let mut daemon = DaemonChild::spawn(&daemon_binary, &profile, &log_file)?;
    let daemon_health = format!("http://{main_addr}/health");
    wait_for_status(
        &mut daemon,
        &client,
        &daemon_health,
        reqwest::StatusCode::OK,
        Duration::from_secs(30),
    )
    .await?;

    run_cli(
        &cli_binary,
        &profile,
        &[
            "init",
            "--passphrase",
            SPACE_PASSPHRASE,
            "--device-name",
            "healthz-benchmark",
        ],
        None,
    )?;
    let mobile_port = setup_mobile_listener(&cli_binary, &profile, cli.mobile_port)?;

    let base = format!("http://127.0.0.1:{mobile_port}");
    let health_url: Arc<str> = format!("{base}/healthz").into();
    let sync_url: Arc<str> = format!("{base}/SyncClipboard.json").into();
    wait_for_status(
        &mut daemon,
        &client,
        &health_url,
        reqwest::StatusCode::NO_CONTENT,
        Duration::from_secs(10),
    )
    .await?;

    one_request(
        &client,
        &sync_url,
        Some((MOBILE_USERNAME, MOBILE_PASSWORD)),
        reqwest::StatusCode::OK,
    )
    .await
    .context("warm up authenticated control endpoint")?;
    tokio::time::sleep(Duration::from_secs(cli.startup_settle_secs)).await;
    settled_log_counts(&log_file).await?;

    let duration = Duration::from_secs(cli.duration_secs);
    let health = run_arm(
        "GET /healthz",
        &client,
        Arc::clone(&health_url),
        None,
        reqwest::StatusCode::NO_CONTENT,
        cli.concurrency,
        duration,
        daemon.pid(),
        cpu_interval,
        &log_file,
    )
    .await?;
    let sync = run_arm(
        "GET /SyncClipboard.json",
        &client,
        Arc::clone(&sync_url),
        Some((MOBILE_USERNAME, MOBILE_PASSWORD)),
        reqwest::StatusCode::OK,
        cli.concurrency,
        duration,
        daemon.pid(),
        cpu_interval,
        &log_file,
    )
    .await?;

    health.validate()?;
    sync.validate()?;
    health.report();
    sync.report();

    ensure!(
        health.logs.mobile_lan_info == 0 && health.logs.mobile_lan_warn == 0,
        "/healthz changed the real mobile LAN INFO/WARN log: INFO +{}, WARN +{}",
        health.logs.mobile_lan_info,
        health.logs.mobile_lan_warn
    );
    ensure!(
        sync.logs.mobile_lan_info > 0,
        "/SyncClipboard.json produced no real mobile LAN INFO log evidence"
    );
    ensure!(
        sync.logs.mobile_lan_warn == 0,
        "/SyncClipboard.json produced {} real mobile LAN WARN logs",
        sync.logs.mobile_lan_warn
    );

    println!(
        "\n-- comparison --\n\
         throughput ratio    : {:.1}x (/healthz over control)\n\
         peak CPU ratio      : {:.2}x (control over /healthz)\n\
         verification        : both arms had successful samples and zero errors; \
         /healthz added zero mobile LAN INFO/WARN lines",
        (health.successes as f64 / health.wall_seconds)
            / (sync.successes as f64 / sync.wall_seconds),
        f64::from(sync.peak_daemon_cpu_pct)
            / f64::from(health.peak_daemon_cpu_pct).max(f64::MIN_POSITIVE),
    );

    daemon.ensure_running()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_is_one_safe_path_segment() {
        for valid in ["healthz-bench-123", "bench_01", "A"] {
            assert!(validate_profile(valid).is_ok(), "valid profile: {valid}");
        }
        for invalid in ["", ".", "..", "x/../logs", "x\\logs", "has space", "中文"] {
            assert!(
                validate_profile(invalid).is_err(),
                "invalid profile: {invalid}"
            );
        }
    }

    #[test]
    fn log_counter_reads_info_and_warn_json_lines() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("daemon.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"level\":\"INFO\",\"target\":\"uc_webserver::mobile_lan::routes::sync_doc\",\"message\":\"one\"}\n",
                "{\"level\":\"DEBUG\",\"target\":\"uc_webserver::mobile_lan\",\"message\":\"ignored\"}\n",
                "{\"level\":\"WARN\",\"target\":\"iroh\",\"message\":\"two\"}\n"
            ),
        )
        .expect("write log fixture");

        let counts = read_log_counts(&path).expect("parse valid daemon log");
        assert_eq!(counts.info, 1);
        assert_eq!(counts.warn, 1);
        assert_eq!(counts.mobile_lan_info, 1);
        assert_eq!(counts.mobile_lan_warn, 0);
    }

    #[test]
    fn log_counter_rejects_malformed_or_incomplete_logs() {
        let temp = tempfile::tempdir().expect("temp directory");
        let malformed = temp.path().join("malformed.jsonl");
        fs::write(&malformed, "not-json\n").expect("write malformed fixture");
        assert!(read_log_counts(&malformed).is_err());

        let incomplete = temp.path().join("incomplete.jsonl");
        fs::write(&incomplete, "{\"level\":\"INFO\"}").expect("write incomplete fixture");
        assert!(read_log_counts(&incomplete).is_err());
    }

    #[test]
    fn log_counter_rejects_unreadable_path() {
        let temp = tempfile::tempdir().expect("temp directory");
        assert!(read_log_counts(&temp.path().join("missing.jsonl")).is_err());
    }
}
