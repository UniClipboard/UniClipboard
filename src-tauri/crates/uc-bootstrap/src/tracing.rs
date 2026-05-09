//! Tracing configuration for UniClipboard
//!
//! Composes the uc-observability dual-output layer builders (pretty console +
//! flat JSON file) with Sentry's `tracing` integration, then registers a
//! single global subscriber.
//!
//! ## Architecture
//!
//! - **uc-observability** provides `build_console_layer` + `build_json_layer`
//!   (profile-driven, local-only outputs) and the redaction blocklist used by
//!   the Sentry `before_send_log` hook below.
//! - **This module** initializes Sentry (Issues + Logs + Performance), adds
//!   `sentry::integrations::tracing::layer()` on top of the local layers, and
//!   registers the composed subscriber via `try_init()`.
//!
//! All remote telemetry — errors, structured logs, performance spans — flows
//! through Sentry. There is no separate OTLP / Seq pipeline anymore; the local
//! JSON file remains as offline diagnostics.
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
use std::sync::{Arc, OnceLock};

use sentry::integrations::tracing::EventFilter;
use tracing_subscriber::prelude::*;
use uc_application::facade::AppPaths;
use uc_infra::settings::repository::load_settings_snapshot;
use uc_observability::redact::{is_sensitive_key, REDACTED_PLACEHOLDER};
use uc_observability::{LogProfile, WorkerGuard};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::AppDirsPort;

static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();
static JSON_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

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

/// Initialize the tracing subscriber with dual-output and Sentry integration.
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
/// 3. Initializes Sentry if a DSN is available — runtime `SENTRY_DSN` env takes
///    priority, falling back to the compile-time `SENTRY_DSN` baked in by CI
///    (mirrors the front-end `VITE_SENTRY_DSN` pattern). Sentry collects three
///    signals through one layer:
///    - **Issues** (panics + tracing ERROR with `error` field)
///    - **Logs** (tracing ERROR + WARN, redacted)
///    - **Performance Spans** (tracing spans, sampled at `traces_sample_rate`)
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

    // Step 2b: Resolve `telemetry_enabled` from persisted settings and push it
    // into the process-wide runtime gate exposed by `uc-observability`.
    //
    // Sentry consults that gate at event time via the `before_send`,
    // `before_breadcrumb`, and `before_send_log` hooks installed below. The
    // user-facing `General › Telemetry` switch therefore takes effect
    // immediately — `uc-webserver`'s PUT /settings handler calls
    // `set_telemetry_enabled` when the field changes, and the next emitted
    // event already honors it.
    //
    // Reading the persisted value here is what makes the *initial* state
    // correct: until the daemon side serves any settings update, the gate
    // would otherwise carry its `true` default and ignore a user who had
    // turned telemetry off in a previous session.
    let telemetry_enabled = resolve_telemetry_enabled(&paths.settings_path);
    uc_observability::set_telemetry_enabled(telemetry_enabled);

    // Step 3: Initialize Sentry whenever a DSN is available.
    //
    // ## DSN 来源优先级
    //
    // 1. **运行时** `SENTRY_DSN` env —— 给 dev / 自部署用户运行时覆盖。
    // 2. **编译期** `SENTRY_DSN` env —— CI 在 release build 时注入,等价前端
    //    `VITE_SENTRY_DSN` 的处理方式。这是必需路径,否则用户机器上没人会
    //    设这个 env,sentry 在终端用户那边永远不会启用。
    //
    // ## 与 telemetry_enabled 的关系
    //
    // Sentry 在有 DSN 时无条件初始化,但每条出站事件 / breadcrumb / log 都会
    // 被对应的 `before_*` 钩子拦截,在用户关闭 telemetry 时一律返回 None。
    // 净效果是用户开关即时生效,不需要重启进程。
    //
    // ## 防双重 panic 上报
    //
    // sentry crate 的默认 `default-integrations` 启用 `sentry-panic`,会自动
    // 把 panic 捕获并上报为 Exception。同时 `install_panic_logging_hook` 把
    // panic 写成 `tracing::error!(target: "panic", ...)` 进 jsonl
    // (jsonl 是离线排障的关键,不能省)。这条 tracing event 默认会再被
    // sentry-tracing layer 转成 sentry Event,导致同一个 panic 在 sentry 上
    // 出现两条 issue。这里用 `event_filter` 让 sentry-tracing 主动忽略
    // `target = "panic"` 的 event,把 panic→sentry 的职责完全交给
    // sentry-panic integration,jsonl 一侧不受影响。
    let runtime_dsn = std::env::var("SENTRY_DSN").ok().filter(|s| !s.is_empty());
    let compile_time_dsn = option_env!("SENTRY_DSN").filter(|s| !s.is_empty());
    let dsn = runtime_dsn.or_else(|| compile_time_dsn.map(String::from));

    let sentry_dsn_present = dsn.is_some();

    let sentry_layer = if let Some(dsn) = dsn {
        let guard = sentry::init((
            dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                // CI 注入的环境标签,默认空 → sentry 显示 "production"。
                environment: option_env!("APP_ENV")
                    .filter(|s| !s.is_empty())
                    .map(Into::into),
                // ERROR / Exception 全采样;performance trace 降到 10% 控制 quota。
                sample_rate: 1.0,
                traces_sample_rate: 0.1,
                // Enable Sentry Logs (replaces the legacy OTLP→Seq pipeline).
                // Tracing ERROR + WARN events are routed to Logs by the
                // `event_filter` below; INFO stays as a breadcrumb only.
                enable_logs: true,
                // Runtime telemetry gate. Drops every outgoing event (incl.
                // panics from the sentry-panic integration) when the user has
                // telemetry off, without un-installing any global hook.
                before_send: Some(Arc::new(|event| {
                    if uc_observability::is_telemetry_enabled() {
                        Some(event)
                    } else {
                        None
                    }
                })),
                // Same gate for the breadcrumb trail — when telemetry is off
                // we drop them at capture time so re-enabling telemetry mid-
                // session doesn't leak the previous quiet period's context.
                before_breadcrumb: Some(Arc::new(|breadcrumb| {
                    if uc_observability::is_telemetry_enabled() {
                        Some(breadcrumb)
                    } else {
                        None
                    }
                })),
                // Per-log sanitizer + telemetry gate. Runs for every Sentry
                // Log payload before transmission, so we can:
                // 1. Drop the log when telemetry is disabled (gate parity with
                //    `before_send` / `before_breadcrumb`).
                // 2. Scrub sensitive attribute values (clipboard text, tokens,
                //    file paths, …) using the shared blocklist defined in
                //    `uc_observability::redact`. The previous OTLP pipeline
                //    relied on a `RedactingExporter` wrapping the OTLP span
                //    exporter; this hook is the Sentry-side equivalent.
                before_send_log: Some(Arc::new(|mut log| {
                    if !uc_observability::is_telemetry_enabled() {
                        return None;
                    }
                    for (key, attr) in log.attributes.iter_mut() {
                        if is_sensitive_key(key) {
                            // sentry::protocol::Value is a re-export of
                            // serde_json::Value; using the re-export keeps
                            // uc-bootstrap free of a direct serde_json dep.
                            attr.0 =
                                sentry::protocol::Value::String(REDACTED_PLACEHOLDER.to_string());
                        }
                    }
                    Some(log)
                })),
                ..Default::default()
            },
        ));

        if SENTRY_GUARD.set(guard).is_err() {
            eprintln!("Sentry guard already initialized");
        }

        // Apply the profile-level EnvFilter to match the JSON file layer.
        //
        // Without this wrapper, NOISE_FILTERS directives like
        // `swarm_discovery::socket=error` would silence the per-tick mDNS
        // EHOSTUNREACH warnings in console / jsonl but leak them straight
        // into Sentry Logs — burning the 5GB/mo quota on infrastructure
        // noise. Symmetry with the jsonl "source of truth" keeps offline
        // diagnostics and remote diagnostics aligned.
        Some(
            sentry::integrations::tracing::layer()
                .event_filter(|md| {
                    if md.target() == "panic" {
                        // panic 由 sentry-panic integration 上报,这里跳过避免重复。
                        EventFilter::Ignore
                    } else if md.target().starts_with("opentelemetry") {
                        // 即便已经从依赖图删掉 opentelemetry-*,任何间接引入的
                        // opentelemetry crate 仍可能 emit 内部错误 —— 这条兜底
                        // 防止它们进 Sentry 噪音。
                        EventFilter::Ignore
                    } else if md.target().starts_with("noq_proto::connection")
                        || md.target().starts_with("noq_udp")
                    {
                        // QUIC 传输层状态机噪音(`PTO expired while unset`,
                        // `failed closing path`,`sendmsg error: No route to host`
                        // 等)。NOISE_FILTERS 里已经按 EnvFilter directive 屏蔽了
                        // 这些 target,但 sentry-tracing 的 layer 在某些版本上
                        // 不完全响应 per-layer EnvFilter,事件仍能漏到 Sentry
                        // (历史报例:UNICLIPBOARD-RUST-3 / `PTO expired while
                        // unset` 在 alpha.3 上 8 次)。这里直接在 event_filter
                        // 里拦住作为双保险 —— iroh 上层仍会把真正的连接失败
                        // 转成自己的结构化事件,没有可观测信号损失。
                        EventFilter::Ignore
                    } else {
                        match *md.level() {
                            // ERROR 同时上报为 Issue(报警)和 Log(可搜索)。
                            // EventFilter 在 sentry 0.48+ 是 bitflags,`|` 即组合。
                            ::tracing::Level::ERROR => EventFilter::Event | EventFilter::Log,
                            // WARN 只进 Logs,不报警;比 OTLP 时代的 INFO+ 更克制,
                            // 留出 5GB/月 配额预算。
                            ::tracing::Level::WARN => EventFilter::Log,
                            // INFO 沿用旧行为做 breadcrumb(下一条 Issue 的上下文),
                            // 不直接产生 Log 防止配额爆炸。
                            ::tracing::Level::INFO => EventFilter::Breadcrumb,
                            _ => EventFilter::Ignore,
                        }
                    }
                })
                .with_filter(profile.json_filter()),
        )
    } else {
        // No eprintln here -- it pollutes CLI output. Absence of a DSN is a
        // normal condition; the closing tracing::info! reports it via the
        // `sentry_dsn_present` field.
        None
    };

    // Step 4: Build local layers from uc-observability
    let console_layer = uc_observability::build_console_layer(&profile);
    let (json_layer, guard) = uc_observability::build_json_layer(&paths.logs_dir, &profile)?;

    // Store WorkerGuard to keep non-blocking writer alive
    if JSON_GUARD.set(guard).is_err() {
        ::tracing::debug!("JSON log guard already initialized — skipping");
    }

    // Step 5: Compose all layers and register.
    //
    // Sentry's tracing integration is a single layer that handles three
    // telemetry concerns at once (Issues / Logs / Performance Spans), routed
    // by the `event_filter` and the bundled span tracking. No separate OTLP
    // trace or logs layer is needed.
    match tracing_subscriber::registry()
        .with(sentry_layer)
        .with(console_layer)
        .with(json_layer)
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

    // `sentry_dsn_present` describes whether the export pipeline was *constructed*;
    // `telemetry_enabled` is the runtime gate that decides whether events
    // actually flow through it. The pipeline can be initialized but currently
    // silent (telemetry off) and become live the instant the user toggles it
    // on, with no restart.
    ::tracing::info!(
        profile = %profile,
        logs_dir = %paths.logs_dir.display(),
        sentry_dsn_present = sentry_dsn_present,
        telemetry_enabled = telemetry_enabled,
        "Tracing initialized with dual output (console + JSON{})",
        if sentry_dsn_present { " + Sentry" } else { "" }
    );

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
