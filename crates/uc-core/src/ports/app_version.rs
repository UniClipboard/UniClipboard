//! `AppVersionStatePort` —— 持久化"上次运行的应用版本"游标。
//!
//! 这是 P1 升级检测模块（`uc-application/src/facade/upgrade/`）的领域端口。
//! 应用启动时读取该游标，与当前编译版本比较，得出 `UpgradeStatus`；用户/调用方
//! 确认后写回，把游标推进到当前版本。
//!
//! 端口语义保持极薄：
//! * 版本字符串格式（如 semver）对端口透明，由 use case 负责解析与比较。
//! * 不存在 = `None`（fresh install 或 P1 之前的老用户），由 use case 配合
//!   `SetupStatusPort.has_completed` 做 fallback 推断。
//! * 解析失败由 use case 处理，不在端口层报错。
//!
//! 持久化格式与具体落点（独立小文件 / settings 子键 / DB）属于 infra 实现细节。

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppVersionStateError {
    /// 读取失败（IO、权限、文件系统）。语义上等价于"无可信来源"，
    /// 调用方一般可以保守地视作 `None` 继续推进，但仍要打日志。
    #[error("read app version cursor failed: {0}")]
    Read(String),
    /// 写入失败。调用方应记录日志；下次启动时游标维持不变。
    #[error("write app version cursor failed: {0}")]
    Write(String),
    /// 文件存在但内容损坏（非 UTF-8、JSON 不合法等）。
    /// 与 `Read` 区分开是为了让 use case 可以选择"清理后重写"或仅日志告警。
    #[error("app version cursor content is corrupt: {0}")]
    Corrupt(String),
}

/// 游标读写端口。"上次运行的应用版本"是 profile 范围内的事实，
/// 实现应落在与 `SetupStatusPort` 同等粒度的 profile 数据目录下。
#[async_trait]
pub trait AppVersionStatePort: Send + Sync {
    /// 读取已记录的版本字符串。
    ///
    /// * `Ok(Some(version))` —— 读到游标。
    /// * `Ok(None)` —— 游标不存在（fresh install 或 P1 之前的老用户）。
    /// * `Err(_)` —— IO / 损坏，调用方决定如何降级。
    async fn read(&self) -> Result<Option<String>, AppVersionStateError>;

    /// 把游标写为给定版本字符串。约定调用方传入合法 semver；
    /// 端口本身不做格式校验，以便未来格式演进不破坏端口契约。
    async fn write(&self, version: &str) -> Result<(), AppVersionStateError>;
}
