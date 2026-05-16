use super::SystemClipboardSnapshot;
use crate::DeviceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardChangeOrigin {
    LocalCapture,
    LocalRestore,
    /// 由远端推送写入本机剪贴板的入站事件。
    ///
    /// `from_device` 携带的是推送方 device id,持久化路径(把这次入站事件
    /// 转成 `ClipboardEvent` 落库)需要它才能给 `ClipboardEvent.source_device`
    /// 填正确的对端;若不带,持久化层会把入站记录的来源错记为本机,
    /// 上层视图就会把"远端推送进来的 entry"展示成"本机产生"。
    ///
    /// 守卫路径(OS 回写后让 watcher 短路忽略回声)只关心"这次变化属于
    /// 远端推送"这个 tag,不关心是哪个对端,故为 `Option`:守卫端构造时
    /// 传 `None`,持久化端构造时传 `Some(from_device)`。
    RemotePush {
        from_device: Option<DeviceId>,
    },
}

impl ClipboardChangeOrigin {
    /// 是否为远端推送来源,忽略 `from_device` 字段。
    ///
    /// 用于守卫路径 / 出站过滤等仅关心 tag、不关心具体对端的比较场景。
    pub fn is_remote_push(&self) -> bool {
        matches!(self, Self::RemotePush { .. })
    }

    /// 构造"匿名"远端推送 origin。
    ///
    /// 守卫路径在 OS 回写后只想标记"下一次剪贴板变化属于远端推送的回声",
    /// 此时并不需要追溯具体对端 —— 用本构造器表达"我知道是远端推送,但
    /// 来源对此处不重要"的意图。
    pub fn remote_push_anonymous() -> Self {
        Self::RemotePush { from_device: None }
    }
}

#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub snapshot: SystemClipboardSnapshot,
    pub origin: ClipboardChangeOrigin,
}
