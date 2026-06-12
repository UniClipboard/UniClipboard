//! Mobile sync 出站(Mac → iPhone)读到的"最新一条剪贴板 paste-priority
//! representation"领域形态。
//!
//! 与 [`crate::clipboard::SystemClipboardSnapshot`] 的差别:
//!
//! | 维度 | `SystemClipboardSnapshot` | `LatestPasteRepresentation` |
//! |---|---|---|
//! | 用途 | capture 路径(系统剪贴板瞬时全貌) | mobile sync 出站(已选 + 已材化) |
//! | 包含 rep | 全部 representations | 仅 paste-priority 一条 |
//! | 字节状态 | 现观察值,可能未持久化 | adapter 已从 inline / blob 通路材化 |
//!
//! 两者都不互相替代:capture 链需要 multi-rep 让 selection policy 选;
//! mobile sync 出站只需要"现在该粘贴啥",形状要更窄、独立演化(未来可能加
//! `selection_policy_version` 等审计字段而不影响其他通路)。

use crate::clipboard::MimeType;
use crate::ids::{EntryId, FormatId};

/// 最近一条剪贴板条目的 paste-priority representation,字节已被 adapter
/// 完全材化,调用方拿到即可序列化 / 计算 hash / 比较 size。
#[derive(Debug, Clone)]
pub struct LatestPasteRepresentation {
    /// 该 representation 所属的 clipboard entry id —— 仅用于日志 / 诊断。
    /// mobile sync 路由不暴露这个 id 给 iPhone 客户端。
    pub entry_id: EntryId,
    pub format_id: FormatId,
    pub mime: Option<MimeType>,
    pub bytes: Vec<u8>,
}
