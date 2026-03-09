//! Placeholder UI port implementation
//! 占位符 UI 端口实现

use anyhow::Result;
use uc_core::ports::UiPort;

#[async_trait::async_trait]
impl UiPort for PlaceholderUiPort {
    async fn open_settings(&self) -> Result<()> {
        // TODO: Implement actual UI integration in Task 7
        // 在任务 7 中实现实际的 UI 集成
        Err(anyhow::anyhow!("UiPort not implemented yet - Task 7"))
    }
}

/// Placeholder UI port implementation
/// 占位符 UI 端口实现
#[derive(Debug, Clone)]
pub struct PlaceholderUiPort;
