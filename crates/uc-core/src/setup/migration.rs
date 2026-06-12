//! Switch-space 重加密迁移的领域类型。
//!
//! 设备已经完成 setup 后再加入另一个 sponsor 的空间时，要把本地剪贴板
//! 历史从旧 master_key 转加密到新 master_key（详见
//! `docs/architecture/ports.md` 的 switch-space 章节）。整个流程分四阶段，
//! 每完成一个阶段就把状态写回 `MigrationStatePort` 的实现：
//!
//! | 阶段        | 已落盘的事实                                         |
//! |-------------|------------------------------------------------------|
//! | `Prepared`  | backup 表已写满，migration_key 在 keyring，主表未动  |
//! | `HandshakeDone` | session/磁盘 keyslot/keyring KEK 已切到新空间        |
//! | `Swapped`   | 主表已用新 master_key 重写，backup 表 + migration_key 仍在 |
//! | `None`      | 阶段 4 完成：backup 表清空、migration_key 销毁       |
//!
//! 故障恢复策略由 `SwitchSpaceUseCase` 在启动时按当前阶段判断，详见
//! 各变体 doc。
//!
//! 这两个类型只承载领域语义，不绑定具体存储实现——具体落盘位置由
//! `MigrationStatePort` 的 adapter 决定。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ids::SpaceId;

/// 一次 switch-space 迁移运行的稳定标识。
///
/// 由 `KeyMigrationPort::prepare_migration_key` 在阶段 1 开始时生成
/// （时间戳 + 随机后缀），随后被序列化进 `MigrationPhase`，落盘后即使
/// daemon 崩溃重启也能用同一 id 找回 keyring 里的 migration key。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MigrationRunId(String);

impl MigrationRunId {
    /// 由 adapter 在生成 migration key 时构造。app/use-case 层不应自己
    /// 拼造——run_id 与 keyring entry 名一一对应，乱造会破坏密钥定位。
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MigrationRunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 一次迁移当前所处的阶段。
///
/// 值唯一持久化点是 `MigrationStatePort`，整个生命周期里只有 4 种合法值：
/// `None` / `Prepared` / `HandshakeDone` / `Swapped`。所有变体都带 `run_id`，
/// 但 `target_space_id` 只在 `HandshakeDone` 起出现——阶段 1 (备份) 跑在
/// handshake 之前，那时还没和 sponsor 通过任何信，无法得知目标空间 id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationPhase {
    /// 阶段 1 已完成：所有 representation 已用 migration_key 重加密
    /// 写入 backup 表。主表内容未变，旧 master_key 仍在 session/磁盘/keyring。
    /// 此时仍未与 sponsor 握手，所以目标空间 id 尚未知。
    ///
    /// 故障恢复：可以选择继续走阶段 2，也可以直接放弃（清空 backup 表 +
    /// 销毁 migration_key），旧空间数据完整。
    Prepared { run_id: MigrationRunId },

    /// 阶段 2 已完成：sponsor handshake 走完，session / 磁盘 keyslot /
    /// keyring KEK 三处都换成新空间的 master_key，`target_space_id` 由
    /// sponsor 在 `Confirm` 消息里给出。主表里的密文都是用旧 master_key
    /// 加密的——此时主表已不可读，必须依靠 backup 表 + migration_key
    /// 走完阶段 3 才能恢复。
    ///
    /// 故障恢复：自动重试阶段 3。无法回退（旧 master_key 已不可恢复）。
    HandshakeDone {
        run_id: MigrationRunId,
        target_space_id: SpaceId,
        /// 进入阶段 4 时本机要切到的目标 telemetry person。`Some(uuid)`
        /// 表示 sponsor 在 handshake 中派发了一个 `space_person_id`，
        /// `None` 表示 sponsor 尚未持有（v1→v2 升级未配对场景），本机
        /// 回退到 Solo。落盘的目的是让 phase-4 续跑时不再依赖
        /// in-memory 的 handshake outcome——daemon 在 phase 4 之前崩溃，
        /// 重启后仍能按这个意图完成 telemetry 身份切换。
        ///
        /// 旧版本写入的状态文件没有该字段，反序列化时按 `None` 处理
        /// （`#[serde(default)]`），等同于"按 Solo 回退"。
        #[serde(default)]
        sponsor_space_person_id: Option<Uuid>,
    },

    /// 阶段 3 已完成：主表所有 representation 都用新 master_key 重写。
    /// backup 表 + migration_key 仍在，等待阶段 4 清理。
    ///
    /// 故障恢复：自动补做阶段 4（写 admit/trust/setup_status，清掉
    /// backup 表和 migration_key）。
    Swapped {
        run_id: MigrationRunId,
        target_space_id: SpaceId,
        /// 与 `HandshakeDone.sponsor_space_person_id` 同义，跨阶段 3→4
        /// 边界继续承载切换意图。语义与序列化兼容策略一致。
        #[serde(default)]
        sponsor_space_person_id: Option<Uuid>,
    },
}

impl MigrationPhase {
    pub fn run_id(&self) -> &MigrationRunId {
        match self {
            Self::Prepared { run_id }
            | Self::HandshakeDone { run_id, .. }
            | Self::Swapped { run_id, .. } => run_id,
        }
    }

    /// 阶段 2 之前没有目标空间——`Prepared` 返回 `None`，`HandshakeDone`
    /// / `Swapped` 返回 `Some`。
    pub fn target_space_id(&self) -> Option<&SpaceId> {
        match self {
            Self::Prepared { .. } => None,
            Self::HandshakeDone {
                target_space_id, ..
            }
            | Self::Swapped {
                target_space_id, ..
            } => Some(target_space_id),
        }
    }

    /// 阶段 4 续跑时要切换到的 telemetry person id。`Prepared` 阶段还
    /// 没和 sponsor 握过手，返回 `None`；`HandshakeDone` / `Swapped`
    /// 返回各自落盘的 `sponsor_space_person_id`（其内值仍可能是
    /// `None`，表示"切到 Solo"）。
    pub fn sponsor_space_person_id(&self) -> Option<Uuid> {
        match self {
            Self::Prepared { .. } => None,
            Self::HandshakeDone {
                sponsor_space_person_id,
                ..
            }
            | Self::Swapped {
                sponsor_space_person_id,
                ..
            } => *sponsor_space_person_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_round_trips_string() {
        let id = MigrationRunId::new("mig-2026-04-27-abc");
        assert_eq!(id.as_str(), "mig-2026-04-27-abc");
        assert_eq!(id.to_string(), "mig-2026-04-27-abc");
    }

    #[test]
    fn migration_phase_serde_round_trip_prepared() {
        let phase = MigrationPhase::Prepared {
            run_id: MigrationRunId::new("run-1"),
        };
        let json = serde_json::to_string(&phase).unwrap();
        let parsed: MigrationPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(phase, parsed);
        // 标签字段名稳定性 ironclad：磁盘格式契约。
        assert!(json.contains("\"kind\":\"prepared\""));
    }

    #[test]
    fn migration_phase_serde_round_trip_handshake_done() {
        let phase = MigrationPhase::HandshakeDone {
            run_id: MigrationRunId::new("run-h"),
            target_space_id: SpaceId::from_str("space-h"),
            sponsor_space_person_id: Some(Uuid::from_u128(0x42)),
        };
        let json = serde_json::to_string(&phase).unwrap();
        let parsed: MigrationPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(phase, parsed);
    }

    #[test]
    fn migration_phase_target_space_id_none_for_prepared() {
        let phase = MigrationPhase::Prepared {
            run_id: MigrationRunId::new("run-x"),
        };
        assert_eq!(phase.target_space_id(), None);
    }

    #[test]
    fn migration_phase_accessors_match_handshake_done() {
        let run_id = MigrationRunId::new("run-x");
        let space = SpaceId::from_str("space-x");
        let person = Uuid::from_u128(0xabcd);
        let phase = MigrationPhase::HandshakeDone {
            run_id: run_id.clone(),
            target_space_id: space.clone(),
            sponsor_space_person_id: Some(person),
        };
        assert_eq!(phase.run_id(), &run_id);
        assert_eq!(phase.target_space_id(), Some(&space));
        assert_eq!(phase.sponsor_space_person_id(), Some(person));
    }

    #[test]
    fn legacy_handshake_done_json_without_person_field_deserializes_as_none() {
        // 旧版本写入的状态文件不携带 sponsor_space_person_id 字段。
        // 反序列化必须按 None 处理（向后兼容契约）。
        let legacy_json = r#"{
            "kind": "handshake_done",
            "run_id": "run-legacy",
            "target_space_id": "space-legacy"
        }"#;
        let parsed: MigrationPhase = serde_json::from_str(legacy_json).unwrap();
        match parsed {
            MigrationPhase::HandshakeDone {
                sponsor_space_person_id,
                ..
            } => {
                assert_eq!(sponsor_space_person_id, None);
            }
            other => panic!("expected HandshakeDone, got {other:?}"),
        }
    }

    #[test]
    fn legacy_swapped_json_without_person_field_deserializes_as_none() {
        let legacy_json = r#"{
            "kind": "swapped",
            "run_id": "run-legacy",
            "target_space_id": "space-legacy"
        }"#;
        let parsed: MigrationPhase = serde_json::from_str(legacy_json).unwrap();
        match parsed {
            MigrationPhase::Swapped {
                sponsor_space_person_id,
                ..
            } => {
                assert_eq!(sponsor_space_person_id, None);
            }
            other => panic!("expected Swapped, got {other:?}"),
        }
    }
}
