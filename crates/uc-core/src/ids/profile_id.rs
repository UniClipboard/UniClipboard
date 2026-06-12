use serde::{Deserialize, Serialize};

use super::id_macro::impl_id;

/// Profile identifier——标识当前用户/profile。
///
/// 单用户模式下固定为 `"default"`(`DefaultCurrentProfile` adapter 提供);
/// 未来多 profile 版本可持有实际用户身份 id。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProfileId(String);

impl_id!(ProfileId);
