//! Placeholder autostart port implementation
//! 占位符自动启动端口实现

use anyhow::Result;
use uc_core::ports::AutostartPort;

impl AutostartPort for PlaceholderAutostartPort {
    fn is_enabled(&self) -> Result<bool> {
        // TODO: Implement actual autostart check in Task 7
        // 在任务 7 中实现实际的自动启动检查
        Ok(false)
    }

    fn enable(&self) -> Result<()> {
        // TODO: Implement actual autostart enable in Task 7
        // 在任务 7 中实现实际的自动启动启用
        Err(anyhow::anyhow!(
            "AutostartPort not implemented yet - Task 7"
        ))
    }

    fn disable(&self) -> Result<()> {
        // TODO: Implement actual autostart disable in Task 7
        // 在任务 7 中实现实际的自动启动禁用
        Err(anyhow::anyhow!(
            "AutostartPort not implemented yet - Task 7"
        ))
    }
}

/// Placeholder autostart port implementation
/// 占位符自动启动端口实现
#[derive(Debug, Clone)]
pub struct PlaceholderAutostartPort;
