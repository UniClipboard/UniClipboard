//! 应用版本边界检测 + 游标推进的 use case 模块。
//!
//! P1 thin 版本只做两件事：
//!
//! 1. [`DetectUpgradeUseCase`] —— 启动期一次性比较 `AppVersionStatePort`
//!    游标 vs 当前构建版本，输出结构化 [`UpgradeStatus`]。
//!    fallback：游标缺失 + `SetupStatusPort.has_completed == true`
//!    一律视为"老用户升级"（`Upgraded { from: None, to: current }`），
//!    与"是否真在 P1 之前的版本上跑过"无关——按"非 fresh 安装即老用户"
//!    的策略一刀切。
//!
//! 2. [`AcknowledgeUseCase`] —— 调用方（UI / CLI）确认用户已知晓后，
//!    把游标推进到当前版本，下次启动得到 `NoChange`。
//!
//! 本模块**不**承担：版本路由表、迁移步骤注册、产品级文案 / UI 决策。
//! 这些归调用方 (UI / CLI) 自行决定。

pub(crate) mod acknowledge;
pub(crate) mod detect;
pub(crate) mod status;

pub(crate) use acknowledge::{AcknowledgeError, AcknowledgeUseCase};
pub(crate) use detect::{DetectUpgradeError, DetectUpgradeUseCase};
pub(crate) use status::UpgradeStatus;
