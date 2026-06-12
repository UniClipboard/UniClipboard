//! `UpgradeFacade` —— P1 thin 升级检测对外入口。
//!
//! 按 `uc-application/AGENTS.md` §11.4，外部 crate（bootstrap / CLI / GUI）
//! 只能通过本目录下的 [`UpgradeFacade`] 访问升级检测能力；底层
//! `DetectUpgradeUseCase` / `AcknowledgeUseCase` 保持 `pub(crate)`。

mod facade;

pub use facade::{
    AcknowledgeUpgradeError, DetectUpgradeError, UpgradeFacade, UpgradeFacadeDeps, UpgradeStatus,
};
