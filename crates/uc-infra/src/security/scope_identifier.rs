//! `KeyScope` 的持久化标识符——`"profile:{profile_id}"`。
//!
//! 该字面值参与 keyring key 前缀 (`kek:v1:profile:{profile_id}`),
//! 属于磁盘兼容不变量的一部分,不可变更。
//!
//! 历史上 impl 在 `uc-core/src/crypto/model.rs::KeyScope::to_identifier`,
//! Slice 4 (U4-D) 起作为 uc-infra adapter 私有 helper,避免 uc-core 泄漏
//! 持久化格式细节。

use super::crypto_model::KeyScope;

pub(crate) fn scope_identifier(scope: &KeyScope) -> String {
    format!("profile:{}", scope.profile_id)
}
