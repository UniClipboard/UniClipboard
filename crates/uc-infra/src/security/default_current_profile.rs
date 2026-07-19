//! Default `CurrentProfilePort` implementation with an explicit profile option.
//! The no-argument constructor preserves the single-user `"default"` profile.
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
        Self::for_profile(ProfileId::from("default"))
    }

    pub fn for_profile(profile: ProfileId) -> Self {
        Self { profile }
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

#[cfg(test)]
mod tests {
    use uc_core::ids::ProfileId;
    use uc_core::ports::security::current_profile::CurrentProfilePort;

    use super::DefaultCurrentProfile;

    #[tokio::test]
    async fn explicit_profile_is_returned_without_replacement() {
        let profile = DefaultCurrentProfile::for_profile(ProfileId::from("mobile-primary"));

        assert_eq!(
            profile.current_profile().await.unwrap(),
            ProfileId::from("mobile-primary")
        );
    }
}
