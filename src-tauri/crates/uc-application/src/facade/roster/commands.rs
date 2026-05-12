//! Result models for `MemberRosterFacade`.
//!
//! Kept minimal by design:
//!
//! * 没有 `ListWithPresenceQuery` —— 当前没有任何过滤维度可暴露(单 space
//!   场景下返回"全部成员"是唯一语义)。plan §4.1 把它列为未来扩展点,等
//!   第一个过滤条件(按在线状态 / 按 fingerprint 前缀)出现再加,避免现
//!   在硬塞一个空 struct。
//! * 没有 `last_seen_at` —— 该字段需要 `PresencePort` 持久追踪"最后一次
//!   Online 的时间戳",但 Slice 2 Phase 1 的 presence port 只暴露
//!   `ReachabilityState`(离散三态),没有时间维度。T7 验收点也只要求
//!   `state` 三值正确,先不加,省得打出一个永远 `None` 的误导字段。

use uc_core::ids::DeviceId;
use uc_core::membership::MemberSyncPreferences;
use uc_core::ports::{ConnectionChannel, ReachabilityState};
use uc_core::settings::model::ContentTypes;

/// One row of the member roster view.
///
/// 字段顺序按"UI 最关心 → 诊断信息"排列,方便 CLI 直接按 `{entry.device_name}
/// ({entry.state})` 打印。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    /// Stable device id(跨 rename 不变)——作为 member 的主键。
    pub device_id: DeviceId,
    /// 用户在 A1 / B2 时设置的可读名字,仅展示用。
    pub device_name: String,
    /// 当前正好有一条 entry 的 `is_local == true` —— 即本机。pre-A1/B2
    /// 状态下本机身份还没生成,此时所有成员都会是 `false`(此窗口期内
    /// `list_with_presence` 返回的 roster 也应该是空,因为还没有 membership
    /// 记录,但防御性仍处理)。
    pub is_local: bool,
    /// 来自 `PresencePort::current_state` 的纯缓存读。首次拨号(F1 hook 触
    /// 发的 `ensure_reachable_all`)完成前,典型值是 `Unknown`。
    pub state: ReachabilityState,
}

/// 应用层 peer 快照。供 daemon HTTP/WS 对外投影,不暴露 core presence 类型。
///
/// `channel` 是 Phase 96 INDIC-01 新增字段:连接通道 4 态(Direct/Relay/
/// Offline/Unknown),由 infra 层 `ConnectionChannelPort` 单点产出,
/// 应用层透传不解释。"Out of LAN" 灰态由 UI 基于 `channel + LAN-only
/// setting` 合成,不在本结构里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSnapshotView {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub addresses: Vec<String>,
    pub is_paired: bool,
    pub connected: bool,
    pub pairing_state: String,
    pub channel: ConnectionChannel,
    pub connection_address: Option<String>,
}

/// 应用层成员摘要。对外只暴露稳定字符串和值对象,不暴露 core 类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSummary {
    pub device_id: String,
    pub device_name: String,
}

/// 应用层内容类型开关。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentTypesView {
    pub text: bool,
    pub image: bool,
    pub link: bool,
    pub file: bool,
    pub code_snippet: bool,
    pub rich_text: bool,
}

/// 应用层内容类型局部更新。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentTypesPatch {
    pub text: Option<bool>,
    pub image: Option<bool>,
    pub link: Option<bool>,
    pub file: Option<bool>,
    pub code_snippet: Option<bool>,
    pub rich_text: Option<bool>,
}

/// 应用层成员同步偏好。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSyncPreferencesView {
    pub send_enabled: bool,
    pub receive_enabled: bool,
    pub send_content_types: ContentTypesView,
    pub receive_content_types: ContentTypesView,
}

/// 应用层成员同步偏好局部更新。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberSyncPreferencesPatch {
    pub send_enabled: Option<bool>,
    pub receive_enabled: Option<bool>,
    pub send_content_types: Option<ContentTypesPatch>,
    pub receive_content_types: Option<ContentTypesPatch>,
}

impl From<ContentTypes> for ContentTypesView {
    fn from(value: ContentTypes) -> Self {
        Self {
            text: value.text,
            image: value.image,
            link: value.link,
            file: value.file,
            code_snippet: value.code_snippet,
            rich_text: value.rich_text,
        }
    }
}

impl From<MemberSyncPreferences> for MemberSyncPreferencesView {
    fn from(value: MemberSyncPreferences) -> Self {
        Self {
            send_enabled: value.send_enabled,
            receive_enabled: value.receive_enabled,
            send_content_types: value.send_content_types.into(),
            receive_content_types: value.receive_content_types.into(),
        }
    }
}

pub(crate) fn apply_member_sync_preferences_patch(
    mut existing: MemberSyncPreferences,
    patch: MemberSyncPreferencesPatch,
) -> MemberSyncPreferences {
    if let Some(v) = patch.send_enabled {
        existing.send_enabled = v;
    }
    if let Some(v) = patch.receive_enabled {
        existing.receive_enabled = v;
    }
    if let Some(send) = patch.send_content_types {
        apply_content_types_patch(&mut existing.send_content_types, send);
    }
    if let Some(receive) = patch.receive_content_types {
        apply_content_types_patch(&mut existing.receive_content_types, receive);
    }
    existing
}

fn apply_content_types_patch(target: &mut ContentTypes, patch: ContentTypesPatch) {
    if let Some(v) = patch.text {
        target.text = v;
    }
    if let Some(v) = patch.image {
        target.image = v;
    }
    if let Some(v) = patch.link {
        target.link = v;
    }
    if let Some(v) = patch.file {
        target.file = v;
    }
    if let Some(v) = patch.code_snippet {
        target.code_snippet = v;
    }
    if let Some(v) = patch.rich_text {
        target.rich_text = v;
    }
}
