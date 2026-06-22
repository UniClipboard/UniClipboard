//! Entry delivery —— "本机视角下,某条 entry 对每个对端的投递结果"的领域模型。
//!
//! 为什么需要这个模块:
//! 出站同步是"一对多"的广播,但每次 wire dispatch 只覆盖单个对端;领域里
//! 没有任何已有结构能回答"这条 entry 对端 X 收到了没"。本模块把"投递尝试
//! 及其结果"提升为可被查询的领域事实,让上层可以基于"已成功送达哪些设备
//! 与失败原因"做展示、追踪和未来的重传决策。
//!
//! 本模块只关心**已发生**的投递尝试。`Pending`(还没尝试)不是一个会被
//! 持久化的事实,而是"已知 trusted peer 集合减去已尝试过的目标集合"的差集,
//! 由应用层在拼装视图时合成,不在本模块定义。

use crate::ids::{DeviceId, EntryId};

/// 一条 entry 对单个对端的最新投递结果。
///
/// `Delivered` / `Duplicate` 对用户视角都属于"对端已经持有这条内容",
/// 但保留区分以便排障 / 后续策略需要。`Unreachable` 表示对端不可达
/// (离线或拨号失败)——这不是故障,只是时机不对,后续上线时可自然
/// 恢复;`Failed` 携带细分原因,代表需要关注或干预的真正失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryDeliveryStatus {
    /// 对端节点接收了 bytes(adapter 层 ack)。
    Delivered,
    /// 对端节点报告"已存在",通常因为对端从另一路径已收到同一内容。
    Duplicate,
    /// 对端节点不可达(没有可用地址或拨号失败)。不属于故障——对端
    /// 当前离线,下次上线时可重试送达。与 `Failed` 的区别:
    /// `Unreachable` 是预期的临时状态,`Failed` 是需要关注的异常。
    Unreachable,
    /// 投递失败,`reason` 给出失败类别。
    Failed { reason: DeliveryFailureReason },
}

/// 失败原因的领域分类。每个变体对应 wire 层一类可识别的失败信号,
/// 用于驱动 UI 文案与可能的恢复策略。变体集合与 wire 失败类型保持
/// 1:1 对应,新增 wire 失败类型时同步扩展。
///
/// 注意:`Offline`(对端不可达)不在此枚举中——它已提升为独立的
/// `EntryDeliveryStatus::Unreachable`,不属于"失败"语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryFailureReason {
    /// 本地策略在 wire 前拒绝(例如 payload 超过本地限制)。
    LocalPolicy,
    /// 对端在 wire 层显式拒绝(协议版本不兼容、header 不合法等)。
    PeerRejected,
    /// 流 I/O 故障(连接断开、短读、读写错等)。
    Io,
    /// 其他内部错误。
    Internal,
}

/// 一次投递结果的不可变记录。`(entry_id, target_device_id)` 二元组
/// 唯一标识一条记录;重复投递同一对端时按"最新结果覆盖"语义存储,
/// 由仓储端口的契约保证。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryDeliveryRecord {
    pub entry_id: EntryId,
    pub target_device_id: DeviceId,
    pub status: EntryDeliveryStatus,
    /// 失败时的人类可读补充,通常是 wire 层错误的字符串细节。
    /// `Delivered` / `Duplicate` 状态下应为 `None`。
    pub reason_detail: Option<String>,
    pub updated_at_ms: i64,
}

/// 仓储端口可能返回的领域错误。具体实现侧的底层错误必须被翻译为本枚举,
/// 不得把第三方错误类型暴露给调用方。
#[derive(Debug, thiserror::Error)]
pub enum EntryDeliveryError {
    /// 引用的 entry_id 在系统中不存在(违反 FK)。
    #[error("entry not found: {0}")]
    EntryNotFound(String),
    /// 持久化层操作失败。
    #[error("storage failure: {0}")]
    Storage(String),
}
