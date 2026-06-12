//! `CurrentProfilePort`——获取当前 profile 身份的只读端口。
//!
//! Slice 7 (U7 候选 B) 起取代历史 `KeyScopePort`:原 port 名带 "Scope" 在
//! `KeyScope` 结构体下沉到 uc-infra 后语焉不详,新名字直接表达 adapter 的
//! 实际职责——"告诉 adapter 当前谁是 active profile"。单用户模式下 adapter
//! 返回固定 `"default"`;未来多 profile 版本可返回实际用户身份。

use async_trait::async_trait;

use crate::ids::ProfileId;

#[derive(Debug, thiserror::Error)]
pub enum CurrentProfileError {
    #[error("failed to get current profile")]
    Unavailable,
}

#[async_trait]
pub trait CurrentProfilePort: Send + Sync {
    async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError>;
}
