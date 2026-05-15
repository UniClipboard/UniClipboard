use crate::ids::{EntryId, EventId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardEntry {
    pub entry_id: EntryId,
    pub event_id: EventId,
    pub created_at_ms: i64,
    pub active_time_ms: i64,
    pub title: Option<String>,
    pub total_size: i64,
    /// 该 entry 是否纳入了"投递状态"追踪体系。
    ///
    /// 投递状态(`EntryDeliveryRecord`)是后续引入的功能,系统升级前已存在的
    /// 历史 entry 没有可信的投递记录可查。该标志用来在视图层做明确区分:
    /// `false` 表示"历史 entry,投递信息未知",视图不应把它合成为 `Pending`;
    /// `true` 表示"新机制下创建的 entry,缺失投递行就意味着尚未尝试"。
    pub delivery_tracked: bool,
}

impl ClipboardEntry {
    /// 默认 `delivery_tracked = true` —— "新建即追踪"是 most-common path,
    /// 真实新建路径都应该走默认值;只有"从存储重建历史 entry"或测试模拟
    /// 历史场景才显式调 [`Self::with_delivery_tracked`] 覆盖为 `false`。
    ///
    /// 这个默认值的方向选择基于:漏写覆盖时 entry 仍被正确追踪(only loss
    /// 是给老 entry 也合成 Pending —— UI 端可见但不严重);反之若默认
    /// `false`,真实新建处一旦漏写覆盖,entry 会被永久标记为 Historical,
    /// UI 显示"无投递记录"是 silent UI bug。
    pub fn new(
        entry_id: EntryId,
        event_id: EventId,
        created_at_ms: i64,
        title: Option<String>,
        total_size: i64,
    ) -> Self {
        Self {
            entry_id,
            event_id,
            created_at_ms,
            active_time_ms: created_at_ms,
            title,
            total_size,
            delivery_tracked: true,
        }
    }

    pub fn new_with_active_time(
        entry_id: EntryId,
        event_id: EventId,
        created_at_ms: i64,
        active_time_ms: i64,
        title: Option<String>,
        total_size: i64,
    ) -> Self {
        Self {
            entry_id,
            event_id,
            created_at_ms,
            active_time_ms,
            title,
            total_size,
            delivery_tracked: true,
        }
    }

    /// 覆盖 `delivery_tracked` 标志。仅在两种场景需要:
    /// - 从存储重建 entry 时,把表里的真实值灌回来
    /// - 测试模拟 historical entry (建于追踪机制启用前)
    pub fn with_delivery_tracked(mut self, tracked: bool) -> Self {
        self.delivery_tracked = tracked;
        self
    }
}
