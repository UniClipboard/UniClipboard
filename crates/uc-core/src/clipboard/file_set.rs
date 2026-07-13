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
    /// Structured member location for file-set rows written by phase 3.
    pub member_location: Option<FileSetMemberLocation>,
    pub kind: EntryFileSetLineKind,
}

impl EntryFileSetLine {
    /// Whether this line marks its selected root as an expanded directory
    /// rather than a loose top-level file.
    ///
    /// Directory members are assigned line indexes after the top-level input
    /// range, so at least the final expanded member sits at or beyond the
    /// persisted `row_count` (pass `EntryFileSet::lines.len()`). That threshold
    /// recognizes even a one-member directory without confusing flat files
    /// preceded by uri-list comments. The empty-directory and nested-path
    /// checks defend the same invariant for data decoded off the wire, where
    /// line indexes are re-derived.
    ///
    /// This predicate is the single source of truth for directory-root
    /// detection; callers deriving per-root state must go through it rather
    /// than re-inlining the condition.
    pub fn indicates_directory_root(&self, row_count: i64) -> bool {
        self.member_location.as_ref().is_some_and(|location| {
            location.kind == FileSetMemberKind::EmptyDirectory
                || location.relative_path.contains('/')
                || self.line_index >= row_count
        })
    }
}

/// Stable location of a selected file or a member expanded from a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSetMemberLocation {
    /// Zero-based position of the selected top-level root.
    pub root_index: i64,
    /// NFC-normalized basename of the selected top-level root.
    pub root_name: String,
    /// NFC-normalized path relative to the selected root, using `/` separators.
    pub relative_path: String,
    /// Member classification needed to reconstruct the selected tree.
    pub kind: FileSetMemberKind,
}

/// Persisted classification tag for a file-set member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSetMemberKind {
    File,
    Executable,
    EmptyDirectory,
}

impl FileSetMemberKind {
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::File => "f",
            Self::Executable => "x",
            Self::EmptyDirectory => "d",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "f" => Some(Self::File),
            "x" => Some(Self::Executable),
            "d" => Some(Self::EmptyDirectory),
            _ => None,
        }
    }
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
    /// A line without file content, including uri-list comments and empty
    /// directory markers distinguished by `member_location.kind`.
    NonFile,
    /// 该行本可以是文件,但因故未纳入这条 entry 的身份/传输范围。
    Excluded { reason: EntryFileSetExcludeReason },
}

/// 一行被排除在外的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFileSetExcludeReason {
    /// The whole set exceeded its member-count or total-byte cap.
    SizeCapExceeded,
    /// 尝试确定其内容身份时失败(如文件不可读)。
    IngestFailed,
    /// The file set contains a symlink or non-regular special file.
    UnsupportedMember,
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
    /// Whether this manifest contains members expanded from a directory root.
    ///
    /// Delegates the per-line decision to
    /// [`EntryFileSetLine::indicates_directory_root`] so the detection rule
    /// lives in exactly one place.
    pub fn has_directory_structure(&self) -> bool {
        let row_count = self.lines.len() as i64;
        self.lines
            .iter()
            .any(|line| line.indicates_directory_root(row_count))
    }

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
        if has_excluded || self.has_directory_structure() {
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

    /// Return the structured identity component for a complete directory set.
    pub fn file_set_v1_component(&self) -> Option<[u8; 32]> {
        if !self.has_directory_structure()
            || self
                .lines
                .iter()
                .any(|line| matches!(line.kind, EntryFileSetLineKind::Excluded { .. }))
        {
            return None;
        }

        let mut leaves = Vec::new();
        for line in &self.lines {
            let location = match (&line.member_location, &line.kind) {
                (Some(location), _) => location,
                (None, EntryFileSetLineKind::NonFile) => continue,
                (None, _) => return None,
            };
            let digest = match (&line.kind, location.kind) {
                (
                    EntryFileSetLineKind::File { content_hash, .. },
                    FileSetMemberKind::File | FileSetMemberKind::Executable,
                ) => Some(&content_hash.bytes),
                (EntryFileSetLineKind::NonFile, FileSetMemberKind::EmptyDirectory) => None,
                _ => return None,
            };
            leaves.push(uc_content_hash::file_set_member_v1(
                &location.root_name,
                &location.relative_path,
                location.kind.as_tag(),
                digest,
            ));
        }

        (!leaves.is_empty()).then(|| uc_content_hash::file_set_v1_wrapper(&leaves))
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
            member_location: None,
            kind: EntryFileSetLineKind::File {
                content_hash: hash(byte),
                blob_id: Some(BlobId::from(format!("blob-{index}"))),
                size_bytes: Some(10),
            },
        }
    }

    #[test]
    fn directory_structure_suppresses_content_digest_contribution() {
        let mut line = file_line(1, 7);
        line.member_location = Some(FileSetMemberLocation {
            root_index: 0,
            root_name: "root".to_string(),
            relative_path: "child.txt".to_string(),
            kind: FileSetMemberKind::File,
        });
        let set = EntryFileSet { lines: vec![line] };

        assert!(set.has_directory_structure());
        assert!(set.content_digest_contribution().is_empty());
    }

    #[test]
    fn directory_structure_produces_versioned_component() {
        let mut file = file_line(2, 7);
        file.member_location = Some(FileSetMemberLocation {
            root_index: 0,
            root_name: "folder".to_string(),
            relative_path: "nested/file.txt".to_string(),
            kind: FileSetMemberKind::File,
        });
        let empty = EntryFileSetLine {
            line_index: 3,
            original_text: "file:///folder/empty".to_string(),
            member_location: Some(FileSetMemberLocation {
                root_index: 0,
                root_name: "folder".to_string(),
                relative_path: "empty".to_string(),
                kind: FileSetMemberKind::EmptyDirectory,
            }),
            kind: EntryFileSetLineKind::NonFile,
        };
        let set = EntryFileSet {
            lines: vec![file, empty],
        };

        let file_leaf =
            uc_content_hash::file_set_member_v1("folder", "nested/file.txt", "f", Some(&[7; 32]));
        let empty_leaf = uc_content_hash::file_set_member_v1("folder", "empty", "d", None);
        assert_eq!(
            set.file_set_v1_component(),
            Some(uc_content_hash::file_set_v1_wrapper(&[
                file_leaf, empty_leaf
            ]))
        );
    }

    #[test]
    fn executable_kind_changes_directory_component() {
        let mut regular = file_line(1, 7);
        regular.member_location = Some(FileSetMemberLocation {
            root_index: 0,
            root_name: "folder".to_string(),
            relative_path: "run".to_string(),
            kind: FileSetMemberKind::File,
        });
        let mut executable = regular.clone();
        executable.member_location.as_mut().unwrap().kind = FileSetMemberKind::Executable;

        assert_ne!(
            EntryFileSet {
                lines: vec![regular]
            }
            .file_set_v1_component(),
            EntryFileSet {
                lines: vec![executable]
            }
            .file_set_v1_component()
        );
    }

    #[test]
    fn directory_component_rejects_file_without_member_location() {
        let mut directory_file = file_line(2, 7);
        directory_file.member_location = Some(FileSetMemberLocation {
            root_index: 0,
            root_name: "folder".to_string(),
            relative_path: "nested/file.txt".to_string(),
            kind: FileSetMemberKind::File,
        });
        let missing_location = file_line(0, 8);
        let set = EntryFileSet {
            lines: vec![missing_location, directory_file],
        };

        assert_eq!(set.file_set_v1_component(), None);
    }

    #[test]
    fn flat_member_location_keeps_content_digest_contribution() {
        let mut line = file_line(0, 7);
        line.member_location = Some(FileSetMemberLocation {
            root_index: 0,
            root_name: "f0".to_string(),
            relative_path: "f0".to_string(),
            kind: FileSetMemberKind::File,
        });
        let set = EntryFileSet { lines: vec![line] };

        assert!(!set.has_directory_structure());
        assert_eq!(set.content_digest_contribution(), vec![[7; 32]]);
    }

    #[test]
    fn comment_before_flat_file_does_not_look_like_directory_structure() {
        let mut line = file_line(1, 7);
        line.member_location = Some(FileSetMemberLocation {
            root_index: 0,
            root_name: "f1".to_string(),
            relative_path: "f1".to_string(),
            kind: FileSetMemberKind::File,
        });
        let set = EntryFileSet {
            lines: vec![
                EntryFileSetLine {
                    line_index: 0,
                    original_text: "# comment".to_string(),
                    member_location: None,
                    kind: EntryFileSetLineKind::NonFile,
                },
                line,
            ],
        };

        assert!(!set.has_directory_structure());
        assert_eq!(set.content_digest_contribution(), vec![[7; 32]]);
    }

    #[test]
    fn content_digest_contribution_sorts_and_skips_non_file_lines() {
        let set = EntryFileSet {
            lines: vec![
                file_line(0, 3),
                EntryFileSetLine {
                    line_index: 1,
                    original_text: "# a comment".into(),
                    member_location: None,
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
                        member_location: None,
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
                    member_location: None,
                    kind: EntryFileSetLineKind::NonFile,
                },
            ],
        };

        assert_eq!(set.file_lines().count(), 1);
    }
}
