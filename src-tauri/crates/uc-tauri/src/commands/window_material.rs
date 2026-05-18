//! `get_main_window_material` —— 前端拿主窗口当前实际生效的 DWM 材质。
//!
//! ## 为什么需要这个命令
//!
//! 主窗口透明背景 token 只有在 Mica 真的装上（Win 11 22H2+）时才该生效，
//! 否则前端走透明 token → 用户看见的是桌面壁纸而不是应用 UI。Mica 是否成功
//! 装配是 Rust 侧的运行期事实（依赖系统版本），前端无法自己判断，因此必须
//! 通过这个 query 拿。
//!
//! 装配本身在 [`crate::run::configure_main_window_for_platform`] 里完成，
//! 结果通过 `app.manage(MainWindowMaterial)` 共享；这个 command 只是把
//! state 取出来回给 webview。

use crate::commands::record_trace_fields;
use crate::window_material::MainWindowMaterial;
use tracing::{info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

#[tauri::command]
#[specta::specta]
pub async fn get_main_window_material(
    state: tauri::State<'_, MainWindowMaterial>,
    _trace: Option<TraceMetadata>,
) -> Result<MainWindowMaterial, crate::commands::CommandError> {
    let span = info_span!(
        "get_main_window_material",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async move { Ok(*state) }.instrument(span).await
}
