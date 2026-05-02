//! Tracing configuration for UniClipboard
//!
//! Thin wrapper that composes uc-observability layer builders with the
//! application-specific Sentry layer, then registers a single global
//! tracing subscriber.
//!
//! ## Architecture
//!
//! - **uc-observability** provides `build_console_layer` + `build_json_layer`
//!   (profile-driven, dual-output: pretty console + flat JSON file) and
//!   `otlp::init_otlp_pipeline` (optional OTLP telemetry export, Phase 87)
//! - **This module** adds the Sentry layer on top, optionally wires OTLP, and
//!   registers the composed subscriber via `try_init()`
//!
//! ## Idempotency
//!
//! `init_tracing_subscriber()` can be called multiple times safely.
//! Only the first call initializes the subscriber; subsequent calls return `Ok(())`.
//!
//! ## Call Site
//!
//! Call `init_tracing_subscriber()` in `main.rs` **before** Tauri Builder setup.

use std::path::Path;
use std::sync::OnceLock;

use tracing_subscriber::prelude::*;
use uc_application::facade::AppPaths;
use uc_infra::settings::repository::load_settings_snapshot;
use uc_observability::{otlp::OtlpGuard, LogProfile, WorkerGuard};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::AppDirsPort;

static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();
static JSON_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
/// Keeps the OTLP TracerProvider alive for the lifetime of the process.
///
/// Stored behind a `ManuallyDrop` inside the `OnceLock` so that the guard is
/// NEVER dropped, even if `set` were to fail (which would otherwise trigger
/// `provider.shutdown()` and poison the shared inner state of every clone held
/// by the registered `tracing_subscriber` layer — producing the infamous
/// "Spans are being emitted even after Shutdown" warning). Static globals are
/// not dropped at program exit, so wrapping in `ManuallyDrop` loses nothing.
static OTLP_GUARD: OnceLock<std::mem::ManuallyDrop<OtlpGuard>> = OnceLock::new();

/// Guard that ensures tracing is initialized exactly once across all entry points.
static TRACING_INITIALIZED: OnceLock<()> = OnceLock::new();

/// Guard that ensures the panic logging hook is installed exactly once.
static PANIC_HOOK_INSTALLED: OnceLock<()> = OnceLock::new();

/// Read the `telemetry_enabled` setting from persisted settings.
///
/// Uses the canonical settings repository read path so that defaults,
/// deserialization rules, and migrations stay in one place.
///
/// Falls back to `true` (the model default) if the file doesn't exist
/// or cannot be loaded.
fn resolve_telemetry_enabled(settings_path: &Path) -> bool {
    load_settings_snapshot(settings_path)
        .unwrap_or_default()
        .general
        .telemetry_enabled
}

/// Initialize the tracing subscriber with dual-output and optional Sentry.
///
/// ## Idempotency
///
/// This function is idempotent. If called more than once, subsequent calls
/// return `Ok(())` without modifying the global subscriber.
///
/// ## Behavior
///
/// 1. Resolves log directory from platform app-dirs
/// 2. Selects [`LogProfile`] from `UC_LOG_PROFILE` env var (or build-type default)
/// 3. Initializes Sentry if `SENTRY_DSN` is set
/// 4. Builds console + JSON layers via `uc_observability`
/// 5. Composes all layers on a `Registry` and registers globally
///
/// ## Errors
///
/// Returns `Err` if:
/// - Platform app-dirs cannot be resolved
/// - The global subscriber is already registered (and this is the first call)
/// - The logs directory cannot be created
pub fn init_tracing_subscriber() -> anyhow::Result<()> {
    // Idempotency guard: skip if already initialized
    if TRACING_INITIALIZED.get().is_some() {
        ::tracing::debug!("Tracing already initialized, skipping");
        return Ok(());
    }

    // Step 1: Resolve logs directory
    let app_dirs = DirsAppDirsAdapter::new().get_app_dirs()?;
    let paths = AppPaths::from_app_dirs(&app_dirs);
    std::fs::create_dir_all(&paths.logs_dir)?;

    // Step 1b: Resolve device_id for process-wide logging correlation
    let device_id = std::fs::read_to_string(&paths.device_id_path()).ok();

    if let Some(device_id) = device_id.as_ref() {
        let _ = uc_observability::set_global_device_id(device_id.clone());
    }

    // Step 2: Select log profile
    let profile = LogProfile::from_env();

    // Step 3: Initialize Sentry (if SENTRY_DSN is set)
    let sentry_layer = if let Ok(dsn) = std::env::var("SENTRY_DSN") {
        let guard = sentry::init((
            dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                traces_sample_rate: 1.0,
                ..Default::default()
            },
        ));

        if SENTRY_GUARD.set(guard).is_err() {
            eprintln!("Sentry guard already initialized");
        }

        Some(sentry_tracing::layer())
    } else {
        // No eprintln here -- it pollutes CLI output. The absence of Sentry
        // is a normal condition and will be visible in the JSON log file via
        // the tracing::info! at the end of initialization.
        None
    };

    // Step 4: Build layers from uc-observability
    let console_layer = uc_observability::build_console_layer(&profile);
    let (json_layer, guard) = uc_observability::build_json_layer(&paths.logs_dir, &profile)?;

    // Store WorkerGuard to keep non-blocking writer alive
    if JSON_GUARD.set(guard).is_err() {
        ::tracing::debug!("JSON log guard already initialized — skipping");
    }

    // Step 4b: Optionally initialize OTLP provider (phase 1 of 2).
    //
    // `init_otlp_provider` is fully synchronous — the underlying HTTP client
    // is `reqwest::blocking::Client`, which manages its own internal tokio
    // runtime. No outer tokio runtime is required here, and spans are
    // exported from opentelemetry_sdk's own background std::thread
    // (not a tokio task), so the provider is fully self-contained.
    //
    // Provider initialization is separated from layer creation so that the
    // layer can be built with the correct generic subscriber type `S`
    // (determined by the full `.with()` composition in Step 5, not at
    // provider-init time). `SdkTracerProvider::clone()` uses Arc semantics.
    // Step 4c: Read telemetry_enabled from persisted settings.
    // This is a lightweight file read — the full settings are loaded later by
    // the app runtime. We only need the boolean gate here.
    let telemetry_enabled = resolve_telemetry_enabled(&paths.settings_path);

    // Note: OTLP enablement and any compile-time config backfill are handled
    // inside init_otlp_provider. The exporter itself still resolves the final
    // endpoint using OpenTelemetry's standard env-var rules.
    let otlp_providers = {
        match uc_observability::otlp::init_otlp_provider(
            &profile,
            device_id.as_deref(),
            telemetry_enabled,
        ) {
            Ok(Some((providers, guard))) => {
                // Wrap the guard in ManuallyDrop before handing it to the
                // OnceLock. If `set` ever fails (it shouldn't — idempotency
                // guard above ensures single-init), ManuallyDrop prevents a
                // stray drop from calling `provider.shutdown()` and poisoning
                // the layer's cloned provider handle.
                if OTLP_GUARD.set(std::mem::ManuallyDrop::new(guard)).is_err() {
                    eprintln!("[uc-bootstrap] OTLP guard already initialized; leaking new guard");
                }
                Some(providers)
            }
            Ok(None) => None,
            Err(e) => {
                // Log to stderr — the global subscriber isn't set yet.
                eprintln!("[uc-bootstrap] failed to initialize OTLP provider ({e}); continuing without it");
                None
            }
        }
    };

    let otlp_enabled = otlp_providers.is_some();

    // Step 5: Compose all layers and register.
    //
    // Phase 2 of OTLP init: build the typed layers now that the subscriber type `S`
    // is fixed by the `.with()` chain below.
    //
    // Two OTLP layers are composed:
    // - Trace layer: converts tracing spans → OTLP traces (for distributed tracing)
    // - Logs layer: converts tracing events → OTLP logs (for standalone events
    //   that are not attached to an exported span)
    let otlp_trace_layer: Option<uc_observability::otlp::layer::OtlpConcreteLayer<_>> =
        otlp_providers
            .as_ref()
            .map(|p| uc_observability::otlp::layer::build_otlp_layer(&p.tracer_provider, &profile));

    let otlp_logs_layer: Option<uc_observability::otlp::logs_layer::OtlpLogsConcreteLayer<_>> =
        otlp_providers.as_ref().map(|p| {
            uc_observability::otlp::logs_layer::build_otlp_logs_layer(&p.logger_provider, &profile)
        });

    match tracing_subscriber::registry()
        .with(sentry_layer)
        .with(console_layer)
        .with(json_layer)
        .with(otlp_trace_layer)
        .with(otlp_logs_layer)
        .try_init()
    {
        Ok(()) => {}
        Err(e) => {
            // [Codex Review R1+R2] Only swallow on genuine re-entry (TRACING_INITIALIZED already set).
            // If this is the first call and try_init() fails, propagate the error.
            if TRACING_INITIALIZED.get().is_some() {
                ::tracing::warn!("Tracing subscriber already set ({}), skipping re-init", e);
                return Ok(());
            } else {
                return Err(anyhow::anyhow!(
                    "Failed to initialize tracing subscriber: {}",
                    e
                ));
            }
        }
    }

    let _ = TRACING_INITIALIZED.set(());

    ::tracing::info!(
        profile = %profile,
        logs_dir = %paths.logs_dir.display(),
        otlp_enabled = otlp_enabled,
        telemetry_enabled = telemetry_enabled,
        "Tracing initialized with dual output (console + JSON{})",
        if otlp_enabled { " + OTLP" } else { "" }
    );

    // Legacy env var migration warning (D-14, REQ-87-10).
    // Emitted through the now-initialized subscriber for structured capture.
    if std::env::var("UC_SEQ_URL").is_ok() {
        ::tracing::warn!(
            "UC_SEQ_URL is set but legacy Seq ingestion was removed in Phase 87. \
             Migrate to OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp"
        );
    }

    Ok(())
}

/// 安装全局 panic hook,把 panic 信息镜像到 tracing。
///
/// 必须在 [`init_tracing_subscriber`] 之后调用 —— 这样 panic 才能进 jsonl。
/// 调用幂等:重复调用静默返回。
///
/// # 行为
///
/// 1. 用 [`std::panic::take_hook`] 拿到当前 default hook 并保留。
/// 2. 用 [`tracing::error!`] 记录 panic message + thread name + source
///    location + 完整 backtrace,target 设为 `panic`,日志会按结构化字段进
///    JSON 文件。
/// 3. 末尾调用原 default hook,保持 stderr 输出和默认行为不变(测试输出、
///    panic abort 等)。
///
/// # 为什么需要这个
///
/// 没有这个 hook,panic 只走 stderr,GUI / daemon 进程的 stderr 不进 jsonl。
/// 当 iroh-blobs 一类的 silent-poison 设计把第一次 IO 错误 swallow 掉之
/// 后,后续的 panic 还是能在 stderr 看到,但首因 panic 永远丢失。装上这个
/// hook 后,所有 panic 都进 jsonl 的 `panic` target,可以离线追溯首因。
///
/// # 跨进程
///
/// 该函数在 `build_core` 中被三个入口共用(GUI / CLI / daemon),所以三种
/// 运行模式下的 panic 都会被捕获到各自进程的 jsonl 中。
pub fn install_panic_logging_hook() {
    // 幂等保护:第一次调用拿走令牌、安装 hook;后续调用直接返回。
    if PANIC_HOOK_INSTALLED.set(()).is_err() {
        return;
    }

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // 强制抓 backtrace,不依赖 RUST_BACKTRACE 环境变量 —— 用户在 dev
        // 环境很少设这个变量,真正出现 panic 时 backtrace 是黄金信息。
        let backtrace = std::backtrace::Backtrace::force_capture();

        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()));

        // panic payload 通常是 &str 或 String,其他类型用占位串避免再 panic。
        let payload = panic_info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&'static str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };

        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();

        ::tracing::error!(
            target: "panic",
            thread = %thread,
            location = location.as_deref().unwrap_or("<unknown>"),
            message = %message,
            backtrace = %backtrace,
            "thread panicked"
        );

        // 保留原 hook 的 stderr 输出 —— 终端用户、test runner、Sentry 旁路
        // 都依赖这一行可见的 panic 文本。
        prev_hook(panic_info);
    }));

    ::tracing::debug!("panic logging hook installed");
}
