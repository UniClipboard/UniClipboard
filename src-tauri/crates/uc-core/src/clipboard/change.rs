use super::SystemClipboardSnapshot;
use crate::DeviceId;

/// 一次剪贴板变化的领域来源。
///
/// 同一份剪贴板内容可能来自本机用户操作,也可能来自远端推送写入本机;
/// 该来源是后续业务判定(归属、去重、过滤等)的依据,而非传输/持久化路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardChangeOrigin {
    /// 本机捕获到的新剪贴板内容。
    LocalCapture,
    /// 本机内部对剪贴板的回填/恢复写入。
    LocalRestore,
    /// 来源于远端设备的剪贴板变化。
    ///
    /// `from_device` 表示这次变化所归属的远端来源对端:
    /// - `Some(device_id)` —— 已知具体对端;
    /// - `None` —— 已知属于远端推送但来源对端未知或对当前消费者无关。
    ///
    /// 消费者据此区分"远端推送"与"本机产生",并在需要追溯归属时取用
    /// `from_device`。
    RemotePush { from_device: Option<DeviceId> },
}

impl ClipboardChangeOrigin {
    /// 是否为远端推送来源,忽略 `from_device` 字段。
    pub fn is_remote_push(&self) -> bool {
        matches!(self, Self::RemotePush { .. })
    }

    /// 构造一个不携带具体来源对端的远端推送 origin。
    ///
    /// 用于"已知本次变化属于远端推送,但来源对端在当前语境无意义"的场景。
    pub fn remote_push_anonymous() -> Self {
        Self::RemotePush { from_device: None }
    }
}

#[derive(Debug, Clone)]
pub struct ClipboardChange {
    pub snapshot: SystemClipboardSnapshot,
    pub origin: ClipboardChangeOrigin,
}
