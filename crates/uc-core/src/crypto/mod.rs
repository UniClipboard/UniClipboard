pub mod aad;
pub mod domain;
pub mod model;
pub mod secret;

pub use aad::*;
pub use model::*;
pub use secret::*;
// 注意：`domain` 不做 `pub use *`——v2 领域类型通过 `crypto::domain::{...}` 显式导入，
// 以便清楚区分 v2 领域纯净层 vs v1 历史类型（见 .planning/.../task_plan.md Phase 2）。
