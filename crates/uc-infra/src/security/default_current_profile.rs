//! 默认 `CurrentProfilePort` 实现——单用户模式下固定返回 `"default"` profile。
//!
//! 历史演进:
//! - 最初位于 `uc-platform/src/key_scope.rs`(违反平台层定位)
//! - Slice 4 (U4-D) 搬到 `uc-infra/security/default_key_scope.rs`
//! - Slice 7 (U7 候选 B) `KeyScopePort` 改名 `CurrentProfilePort`,类型从
//!   `KeyScope` struct 改为 `ProfileId` 值对象;文件同步改名

use uc_core::ids::ProfileId;
use uc_core::ports::security::current_profile::{CurrentProfileError, CurrentProfilePort};

pub struct DefaultCurrentProfile {
    profile: ProfileId,
}

impl DefaultCurrentProfile {
    pub fn new() -> Self {
        Self {
            profile: ProfileId::from("default"),
        }
    }
}

impl Default for DefaultCurrentProfile {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl CurrentProfilePort for DefaultCurrentProfile {
    async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
        Ok(self.profile.clone())
    }
}
