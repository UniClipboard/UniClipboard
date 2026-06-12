//! Application-layer errors for `MemberRosterFacade`.

use thiserror::Error;

/// Failure modes of [`crate::facade::roster::MemberRosterFacade::list_with_presence`].
///
/// `PresencePort::current_state` 故意**不**在这里出现——它的签名是
/// `async fn current_state(...) -> ReachabilityState`(无 Result),
/// 表示"读缓存不可能失败"。若未来 adapter 想在缓存失效时回退到
/// `Unknown`,也应继续在 port 内部消化,保持本 error 小而稳定。
#[derive(Debug, Error)]
pub enum RosterError {
    /// `MemberRepositoryPort::list` 故障。消息面向日志;UI 上层一般
    /// 展示一句"无法加载成员列表"+ 原样 error 字符串调试。
    #[error("failed to list members: {0}")]
    MemberRepository(String),

    /// `LocalIdentityPort::get_current_fingerprint` 故障——adapter 在读
    /// 身份存储(keychain / 文件)时出错。区别于"还没创建身份"
    /// (那返回 `Ok(None)`,不是 error),此 variant 表示存储本身故障。
    #[error("failed to read local identity: {0}")]
    LocalIdentity(String),

    /// `PeerAddressRepositoryPort` 故障。`revoke_member` 在删除成员后
    /// 还需要清理同设备的 peer 地址条目,否则 dispatch / presence 仍会
    /// 把已撤销设备当作目标(见 `dispatch_entry.rs` module doc 关于
    /// "peer_addr_repo 是 paired members 权威集合" 的不变量)。
    #[error("failed to remove peer address: {0}")]
    PeerAddressRepository(String),

    /// `TrustedPeerRepositoryPort` 故障。`revoke_member` 在删除成员后
    /// 还要清掉对应的 trust 记录,维持 `trusted_peer ⊆ member` 不变量,
    /// 否则本机会继续把已撤销设备当可信对端(#1023 之后残留行不再挡死
    /// 重新配对——`TrustPeerUseCase` 重配时显式替换,见 `trust_peer.rs`)。
    #[error("failed to remove trusted peer: {0}")]
    TrustedPeerRepository(String),

    /// 目标成员不存在。
    #[error("member `{0}` not found")]
    NotFound(String),

    /// 成员 roster 入口尚未接入。通常表示 daemon/CLI 组合阶段没有注入该能力。
    #[error("member roster facade unavailable")]
    Unavailable,
}
