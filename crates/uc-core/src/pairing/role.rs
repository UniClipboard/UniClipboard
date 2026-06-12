use serde::{Deserialize, Serialize};

/// 配对中的角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairingRole {
    /// 发起方 (扫描/主动连接的一方)
    Initiator,
    /// 响应方 (被扫描/被动连接的一方)
    Responder,
}
