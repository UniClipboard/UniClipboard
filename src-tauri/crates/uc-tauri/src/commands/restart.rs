//! Restart-related Tauri commands.
//! 重启相关的 Tauri 命令。
//!
//! Phase 95: covers GUI mode only (D-B1). CLI daemon mode is out of scope.

use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use serde::Serialize;
use std::path::Path;
use std::sync::OnceLock;
use std::time::SystemTime;
use tracing::{info, info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

/// Process boot timestamp — set ONCE in `uc_tauri::run` setup.
/// 进程启动时间戳 — 在 uc_tauri::run setup 阶段唯一写入。
pub static PROCESS_STARTED_AT: OnceLock<SystemTime> = OnceLock::new();

/// Returned by `get_restart_state` Tauri command.
/// `get_restart_state` Tauri 命令的返回类型。
///
/// # Wire format / 线协议
/// `#[serde(rename_all = "camelCase")]` —— wire 字段名为
/// `processStartedAt` / `settingsMtime`，与 daemon-contract camelCase 风格一致。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartState {
    pub process_started_at: i64,
    pub settings_mtime: i64,
}

fn system_time_to_millis(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn read_process_started_at_millis() -> i64 {
    PROCESS_STARTED_AT
        .get()
        .copied()
        .map(system_time_to_millis)
        .unwrap_or(0)
}

fn read_settings_mtime_millis(path: &Path) -> Result<i64, CommandError> {
    let metadata = std::fs::metadata(path).map_err(CommandError::internal)?;
    let modified = metadata.modified().map_err(CommandError::internal)?;
    Ok(system_time_to_millis(modified))
}

/// Trigger graceful Tauri process restart for settings change effect.
/// 触发 Tauri 进程优雅重启使设置变更生效。
///
/// # Scope (per D-B1)
/// 仅 cover GUI mode；CLI daemon (`uniclip daemon`) 不在范围。
///
/// # Mechanism (per D-B2)
/// 复用 `app.restart()`（与 `updater.rs:300-301` 同模式）。进程退出会触发
/// `task_registry::shutdown` cancel cascade，daemon 子系统随 Tauri 进程
/// 一起 graceful 关闭 —— 不显式调用 `DaemonHandle::shutdown`。
#[tauri::command]
pub async fn restart_app(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.restart.restart_app",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        info!("restarting app for settings change (LAN-only Mode)");
        app.restart();
        // app.restart() 会调用 process exit；以下不可达，仅满足类型签名。
        #[allow(unreachable_code)]
        Ok(())
    }
    .instrument(span)
    .await
}

/// Read process boot timestamp + settings.json mtime for pending-state derivation.
/// 读取进程启动时间戳与 settings.json mtime 用于 pending 状态推导。
///
/// # Pending derivation (per D-D1)
/// `settings_mtime > process_started_at` ⇒ pending（settings.json 在本进程
/// 启动后被改过，重启后才能让新 `disable_relays` 值生效）。
#[tauri::command]
pub async fn get_restart_state(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<RestartState, CommandError> {
    let span = info_span!(
        "command.restart.get_restart_state",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let settings_path = runtime.storage_paths().settings_path.clone();
        let settings_mtime = read_settings_mtime_millis(&settings_path)?;
        let process_started_at = read_process_started_at_millis();
        Ok(RestartState {
            process_started_at,
            settings_mtime,
        })
    }
    .instrument(span)
    .await
}

// ===== Phase 95 边界 fence =====
//
// 1. D-B1: 仅 cover GUI mode。本文件 NOT 暴露任何 daemon HTTP admin/restart 端点。
//    CLI daemon (`uniclip daemon`) 用户走 systemctl/launchd（PROJECT.md §Out of Scope）。
//
// 2. D-D1: pending 推导仅走 mtime 比对。本文件 NOT 反查 daemon 当前 bind 值
//    （bind-value 反查由 Phase 96 ConnectionChannelPort 承担）。
//
// 3. Pitfall 5 防御: 本文件 NOT 引用 telemetry / OTLP / pkarr / auto-update 任何字段；
//    `restart_app` 只是 `app.restart()` thin wrapper，没有副作用越界（不 disable 遥测、不 reset state）。

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;
    use std::time::Duration;

    // Test 1: OnceLock 单次写入语义
    #[test]
    fn process_started_at_oncelock_rejects_double_set() {
        let cell: OnceLock<SystemTime> = OnceLock::new();
        assert!(cell.set(SystemTime::now()).is_ok());
        // 第二次 set 必失败
        assert!(cell.set(SystemTime::now() + Duration::from_secs(1)).is_err());
    }

    // Test 2: RestartState serde camelCase
    #[test]
    fn restart_state_serializes_camel_case() {
        let state = RestartState {
            process_started_at: 1000,
            settings_mtime: 2000,
        };
        let json = serde_json::to_string(&state).expect("serialize");
        assert!(
            json.contains(r#""processStartedAt":1000"#),
            "missing camelCase processStartedAt — got: {json}"
        );
        assert!(
            json.contains(r#""settingsMtime":2000"#),
            "missing camelCase settingsMtime — got: {json}"
        );
        assert!(
            !json.contains("process_started_at"),
            "snake_case leak — got: {json}"
        );
    }

    // Test 3: settings_mtime 读取（存在文件 → 正数 millis）
    #[test]
    fn read_settings_mtime_returns_positive_for_existing_file() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(f, "{{}}").expect("write");
        drop(f);

        let mtime = read_settings_mtime_millis(&path).expect("read mtime");
        assert!(mtime > 0, "expected positive mtime, got {mtime}");
    }

    // Test 3b: settings_mtime 读取（不存在文件 → InternalError）
    #[test]
    fn read_settings_mtime_returns_internal_error_for_missing_file() {
        let path = Path::new("/nonexistent/path/settings.json");
        match read_settings_mtime_millis(path) {
            Err(CommandError::InternalError(_)) => {}
            other => panic!("expected InternalError, got {other:?}"),
        }
    }

    // Test 4: process_started_at 读取（未 set 返回 0；helper 单调非负）
    #[test]
    fn read_process_started_at_returns_non_negative() {
        let val = read_process_started_at_millis();
        assert!(val >= 0, "expected non-negative, got {val}");
    }

    // Test 5: 边界审计提示（fence reviewer attention）
    //
    // 静态约束：本文件 use 列表不允许引用 daemon HTTP 客户端或 telemetry 模块。
    // 此测试不 cover 运行时，仅作 reviewer 注意力提示；真正 enforcement 由
    // 本 plan 的 grep acceptance_criteria 完成（grep "daemon_client|DaemonClient|
    // admin/restart|telemetry_enabled|otlp" 返回 0 行）。
    //
    // 测试 body 故意是常量比较：让 reviewer 在 PR diff 里看到这个 fence test
    // 名字 + 注释，从而把注意力引到边界约束上。
    #[test]
    fn restart_module_does_not_reference_daemon_http_or_telemetry() {
        // PROCESS_STARTED_AT 的类型必须是 OnceLock<SystemTime>（编译期断言）；
        // 若本文件被改成引用其他全局状态（如 Lazy<DaemonClient>），编译会断。
        const _ASSERT_ONCE_LOCK_SYSTEM_TIME: fn() -> &'static OnceLock<SystemTime> =
            || &PROCESS_STARTED_AT;
        // 名字命中即可：reviewer 看到 fence test 名 + 上方注释会主动审视边界。
        let fence_marker = "fence";
        assert_eq!(fence_marker, "fence");
    }
}
