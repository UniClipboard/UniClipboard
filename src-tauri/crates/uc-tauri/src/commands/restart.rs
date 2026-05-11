//! Restart-related Tauri commands.
//! 重启相关的 Tauri 命令。
//!
//! 历史: Phase 4 上半场曾引入 `restart_daemon` (in-process daemon reload),
//! 目的是 mobile_sync 配置变更后避免 `app.restart()` 引发的新旧进程同时持
//! 端口窗口期。但 in-process reload 路径与 iroh `IrohNodeBuilder::bind`
//! 进程级单次约束 (Pitfall 3) 根本性冲突 —— `daemon-lifecycle` 重建会
//! 第二次调用 bind 触发 panic。
//!
//! 2026-05-11 决策方案 C: 取消 `restart_daemon`,所有"需要重启"设置走
//! 进程级 `app.restart()`。daemon 概念恢复完整 (含 iroh),"重启 daemon
//! = 重启网络栈 = 重启进程"语义自洽。详见
//! `.planning/quick/260510-phase4-deps-share/findings.md` §0。
//!
//! Phase 95 边界 fence:
//!
//! 1. D-B1: 仅 cover GUI mode。本文件 NOT 暴露任何 daemon HTTP admin/restart
//!    端点。CLI daemon (`uniclip daemon`) 用户走 systemctl/launchd
//!    (PROJECT.md §Out of Scope)。
//! 2. Pitfall 5 防御: 本文件 NOT 引用 telemetry / OTLP / pkarr / auto-update
//!    任何字段;`restart_app` 只是 `app.restart()` thin wrapper,没有副作用
//!    越界(不 disable 遥测、不 reset state)。

use tracing::{info, info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

use crate::commands::record_trace_fields;

/// Restarts the running Tauri application to apply settings changes.
///
/// Process-level restart via `tauri::AppHandle::restart()`. 用户的"需要重启"
/// 设置(LAN-only Mode / mobile_sync 端口等)都走这条路径。
///
/// 旧的"新旧进程同时持端口"窗口由 GUI exit graceful shutdown 配合新进程
/// bind retry 兜底处理 —— 见 `uc-tauri/src/run.rs` 的 `RunEvent::Exit` /
/// `RunEvent::ExitRequested` 处理。
#[tauri::command]
pub async fn restart_app(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), crate::commands::error::CommandError> {
    let span = info_span!(
        "command.restart.restart_app",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        info!("restarting app for settings change");
        app.restart();
        // app.restart() 会调用 process exit；以下不可达，仅满足类型签名。
        #[allow(unreachable_code)]
        Ok(())
    }
    .instrument(span)
    .await
}
