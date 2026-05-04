//! Restart-related Tauri commands.
//! 重启相关的 Tauri 命令。
//!
//! Phase 95: covers GUI mode only (D-B1). CLI daemon mode is out of scope.

use crate::commands::error::CommandError;
#[allow(unused_imports)]
use crate::commands::record_trace_fields;
use serde::Serialize;
use std::path::Path;
use std::sync::OnceLock;
use std::time::SystemTime;
#[allow(unused_imports)]
use tracing::{info, info_span, Instrument};
#[allow(unused_imports)]
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

// restart_app + get_restart_state #[tauri::command] 实装在 Task 2 GREEN 阶段加入。

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
}
