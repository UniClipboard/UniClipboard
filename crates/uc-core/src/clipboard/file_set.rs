//! `EntryFileSet` —— 一条文件类 entry 的逐行清单。
//!
//! 为什么需要这个模块:
//! 文件类 entry 的跨设备身份(见 [`crate::clipboard::SystemClipboardSnapshot::snapshot_hash`])
//! 依赖"这条 entry 包含哪些文件、每个文件的内容 hash 是什么",但这个事实
//! 此前只在 capture 那一刻短暂存在于内存里,之后每次需要它(出站、重发、
//! 被动 serve)都要重新解析原始数据、重新计算。本模块把这份清单提升为
//! 一等公民的领域模型:capture 时构建一次并持久化,后续所有读取方读同一份
//! 记录,不再重算。
//!
//! 清单按原始行序逐行保留(含重复行与非文件行),因为身份计算与未来的
//! 展示/重发都需要能区分"多一行"或"行序不同"的情况。

use crate::clipboard::ContentHash;
use crate::ids::BlobId;

/// 一条文件类 entry 的完整逐行清单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFileSet {
    pub lines: Vec<EntryFileSetLine>,
}

/// 清单中的一行,对应原始文件列表表示里的一行文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFileSetLine {
    /// 该行在原始清单中的位置,从 0 开始,保序。
    pub line_index: i64,
    /// 原始行文本,未经解析/归一化。
    pub original_text: String,
    pub kind: EntryFileSetLineKind,
}

/// 一行清单条目的分类结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryFileSetLineKind {
    /// 该行解析为一个文件,内容身份(hash)已确定。
    ///
    /// `blob_id` / `size_bytes` 在内容身份确定的当下不一定已知——把文件
    /// 物化进 blob 仓库是另一个(可能异步、可能延后)步骤,尚未发生时这两
    /// 个字段为 `None`,由后续把该行物化的一方补齐。
    File {
        content_hash: ContentHash,
        blob_id: Option<BlobId>,
        size_bytes: Option<i64>,
    },
    /// 该行不表示文件(空行、注释、无法识别的条目)。
    NonFile,
    /// 该行本可以是文件,但因故未纳入这条 entry 的身份/传输范围。
    Excluded { reason: EntryFileSetExcludeReason },
}

/// 一行被排除在外的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFileSetExcludeReason {
    /// 超出体积/数量上限策略。
    ///
    /// Reserved for the capture-time size-cap gate (ADR-010: per-set total-
    /// size and member-count caps). The capture path does not yet produce
    /// this variant; the persistence codec and identity rules already handle
    /// it so the gate can be added without a schema/format change. Both
    /// exclude reasons are treated identically by
    /// [`EntryFileSet::content_digest_contribution`] (any exclusion → path-
    /// text identity fallback).
    SizeCapExceeded,
    /// 尝试确定其内容身份时失败(如文件不可读)。
    IngestFailed,
}

/// 仓储端口可能返回的领域错误。具体实现侧的底层错误必须被翻译为本枚举,
/// 不得把第三方错误类型暴露给调用方。
#[derive(Debug, thiserror::Error)]
pub enum EntryFileSetError {
    /// 引用的 entry_id 在系统中不存在(违反 FK)。
    #[error("entry not found: {0}")]
    EntryNotFound(String),
    /// 持久化层操作失败。
    #[error("storage failure: {0}")]
    Storage(String),
}

impl EntryFileSet {
    /// 该清单中 `File` 行的内容 hash,已排序。
    ///
    /// 排序是有意为之:身份计算只关心"这条 entry 含有哪些文件内容",不
    /// 关心它们在清单里的先后顺序,排序让相同的文件内容集合始终产出相同
    /// 的贡献值。
    ///
    /// All-or-nothing 契约:只要清单里存在任何 `Excluded` 行(某个文件本
    /// 该参与身份、却因不可读或超限被排除),就返回空 `Vec`,让调用方整体
    /// 回退到路径文本身份,而不是用"恰好成功的那个子集"算身份。这是有意
    /// 的不变量,而非实现细节:出站发布路径(`publish_file_blob_refs`)对
    /// 文件集是全有或全无的(任一文件发布失败即整体失败),捕获侧必须与之
    /// 对齐——否则同一份文件集在两端算出不同子集 → 不同身份 → 接收端建出
    /// 重复 entry(即历史上的 dual-channel file dedup 分叉)。
    pub fn content_digest_contribution(&self) -> Vec<[u8; 32]> {
        // A partial digest set is worse than none: it silently keys the entry
        // on whichever files happened to be readable at capture time, which
        // can differ across devices / retries. Fall back to path-text identity.
        let has_excluded = self
            .lines
            .iter()
            .any(|line| matches!(line.kind, EntryFileSetLineKind::Excluded { .. }));
        if has_excluded {
            return Vec::new();
        }

        let mut digests: Vec<[u8; 32]> = self
            .lines
            .iter()
            .filter_map(|line| match &line.kind {
                EntryFileSetLineKind::File { content_hash, .. } => Some(content_hash.bytes),
                _ => None,
            })
            .collect();
        digests.sort_unstable();
        digests
    }

    /// 迭代所有 `File` 行。
    pub fn file_lines(&self) -> impl Iterator<Item = &EntryFileSetLine> {
        self.lines
            .iter()
            .filter(|line| matches!(line.kind, EntryFileSetLineKind::File { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::HashAlgorithm;

    fn hash(byte: u8) -> ContentHash {
        ContentHash {
            alg: HashAlgorithm::Blake3V1,
            bytes: [byte; 32],
        }
    }

    fn file_line(index: i64, byte: u8) -> EntryFileSetLine {
        EntryFileSetLine {
            line_index: index,
            original_text: format!("file:///f{index}"),
            kind: EntryFileSetLineKind::File {
                content_hash: hash(byte),
                blob_id: Some(BlobId::from(format!("blob-{index}"))),
                size_bytes: Some(10),
            },
        }
    }

    #[test]
    fn content_digest_contribution_sorts_and_skips_non_file_lines() {
        let set = EntryFileSet {
            lines: vec![
                file_line(0, 3),
                EntryFileSetLine {
                    line_index: 1,
                    original_text: "# a comment".into(),
                    kind: EntryFileSetLineKind::NonFile,
                },
                file_line(2, 1),
            ],
        };

        let contribution = set.content_digest_contribution();
        assert_eq!(contribution, vec![[1u8; 32], [3u8; 32]]);
    }

    #[test]
    fn content_digest_contribution_is_empty_when_any_line_excluded() {
        // All-or-nothing: a set with even one excluded (unreadable / too-big)
        // file line must NOT key its identity on the successful subset — it
        // falls back to path-text identity (empty contribution) so both
        // capture and dispatch sides agree.
        for reason in [
            EntryFileSetExcludeReason::IngestFailed,
            EntryFileSetExcludeReason::SizeCapExceeded,
        ] {
            let set = EntryFileSet {
                lines: vec![
                    file_line(0, 3),
                    file_line(1, 1),
                    EntryFileSetLine {
                        line_index: 2,
                        original_text: "file:///excluded".into(),
                        kind: EntryFileSetLineKind::Excluded { reason },
                    },
                ],
            };

            assert!(
                set.content_digest_contribution().is_empty(),
                "excluded reason {reason:?} must yield empty contribution"
            );
        }
    }

    #[test]
    fn file_lines_filters_to_file_kind_only() {
        let set = EntryFileSet {
            lines: vec![
                file_line(0, 1),
                EntryFileSetLine {
                    line_index: 1,
                    original_text: String::new(),
                    kind: EntryFileSetLineKind::NonFile,
                },
            ],
        };

        assert_eq!(set.file_lines().count(), 1);
    }
}
