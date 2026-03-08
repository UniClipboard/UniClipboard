use crate::ports::AutostartPort;
use anyhow::Result;
use std::sync::Arc;

/// Apply OS-level autostart state based on the requested `enabled` flag.
///
/// This use case is responsible only for calling the platform autostart API.
/// Settings persistence is handled by the caller (e.g. `update_settings` command).
///
/// 根据请求的 `enabled` 标志应用 OS 级别的自启动状态。
/// 此用例仅负责调用平台自启动 API，设置持久化由调用方处理。
pub struct ApplyAutostartSetting<A>
where
    A: AutostartPort,
{
    autostart: Arc<A>,
}

impl<A> ApplyAutostartSetting<A>
where
    A: AutostartPort,
{
    /// Create a new ApplyAutostartSetting use case.
    /// 创建新的 ApplyAutostartSetting 用例。
    pub fn new(autostart: Arc<A>) -> Self {
        Self { autostart }
    }

    /// Apply the OS-level autostart setting.
    ///
    /// Checks current OS state first to avoid redundant enable/disable calls.
    /// 先检查当前 OS 状态，避免冗余的 enable/disable 调用。
    pub fn execute(&self, enabled: bool) -> Result<()> {
        let currently_enabled = self.autostart.is_enabled()?;
        if currently_enabled == enabled {
            tracing::debug!(
                enabled,
                "OS autostart already matches requested state, skipping"
            );
            return Ok(());
        }

        if enabled {
            self.autostart.enable()?;
            tracing::info!("OS autostart enabled");
        } else {
            self.autostart.disable()?;
            tracing::info!("OS autostart disabled");
        }

        Ok(())
    }
}
