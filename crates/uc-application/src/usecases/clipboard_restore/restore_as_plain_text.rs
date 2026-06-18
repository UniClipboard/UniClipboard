//! 「以纯文本形式恢复条目到系统剪贴板」用例。
//!
//! 目的：用户按住 Option / Alt 触发粘贴时，希望目标应用粘出纯文本，而不是
//! Markdown 源码 / HTML 标签 / RTF 等富文本。系统剪贴板的"目标应用挑哪个
//! 格式粘"决定权来自源端"摆了哪些菜"——本用例只往剪贴板写入 `text/plain`
//! 一种表示，让目标应用别无选择。
//!
//! 与 `RestoreClipboardSelectionUseCase` 的边界：
//! - 该用例只尝试 plain 路径：在 selection 的候选 rep 中找出 mime 为
//!   `text/plain`（或等价的 `public.utf8-plain-text` UTI / `text` format_id）
//!   的那一份，单独打包成 snapshot 写入。
//! - 若条目根本不存在 plain 表示（纯图片 / 纯文件 / 纯 HTML 等极少数情况），
//!   返回 `PlainRestoreOutcome::NoPlainTextAvailable` 让 facade 决定降级。
//!   本用例**不**自己回退——降级是编排职责，留给 facade。
//!
//! 自写抑制：复用 `ClipboardWriteCoordinator` + `ClipboardWriteIntent::LocalRestore`。
//! `origin_guard_key` 在仅含 plain rep 的 snapshot 上计算结果为 `text:<hash>`，
//! 与 watcher 回声端口计算结果一致，hash 守卫稳定命中；即便遇到极端 hash miss，
//! coordinator 的一次性 `set_next_origin(LocalRestore, 2s)` 兜底，watcher 不会
//! 把这次恢复重新当作新条目落库。
//!
//! payload 读取：必须通过 `ClipboardPayloadResolverPort.resolve()`——Staged 状态
//! 下 `rep.inline_data` 是 normalizer 留下的 500 字符预览截断版，直接读会粘出
//! 残缺文本。这一约束与 `RestoreClipboardSelectionUseCase` 一致。

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, warn};

use uc_core::{
    blob::ports::BlobReaderPort,
    clipboard::{
        is_plain_text_mime_or_format, ClipboardIntegrationMode, ObservedClipboardRepresentation,
        PersistedClipboardRepresentation, SystemClipboardSnapshot,
    },
    ids::EntryId,
    ports::{
        clipboard::{
            ClipboardPayloadResolverPort, GetClipboardEntryPort, GetRepresentationPort,
            ResolvedClipboardPayload,
        },
        ClipboardSelectionRepositoryPort,
    },
};

use crate::clipboard_write::{ClipboardWriteCoordinator, ClipboardWriteIntent};

/// 本用例的可观察执行结果。
///
/// `Done`：成功把纯文本写入了系统剪贴板。
/// `NoPlainTextAvailable`：条目下没有任何可用的 `text/plain` 表示，facade
/// 应当降级到多格式恢复路径。这是预期的业务结果，不是错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlainRestoreOutcome {
    Done,
    NoPlainTextAvailable,
}

pub(crate) struct RestoreClipboardEntryAsPlainTextUseCase {
    clipboard_repo: Arc<dyn GetClipboardEntryPort>,
    coordinator: Arc<ClipboardWriteCoordinator>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn GetRepresentationPort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    blob_store: Arc<dyn BlobReaderPort>,
    mode: ClipboardIntegrationMode,
}

impl RestoreClipboardEntryAsPlainTextUseCase {
    pub(crate) fn new(
        clipboard_repo: Arc<dyn GetClipboardEntryPort>,
        coordinator: Arc<ClipboardWriteCoordinator>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn GetRepresentationPort>,
        payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
        blob_store: Arc<dyn BlobReaderPort>,
        mode: ClipboardIntegrationMode,
    ) -> Self {
        Self {
            clipboard_repo,
            coordinator,
            selection_repo,
            representation_repo,
            payload_resolver,
            blob_store,
            mode,
        }
    }

    pub(crate) async fn execute(&self, entry_id: &EntryId) -> Result<PlainRestoreOutcome> {
        info!(entry_id = %entry_id, "restore_plain.execute requested");

        if !self.mode.allow_os_write() {
            return Err(anyhow::anyhow!(
                "System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)"
            ));
        }

        let snapshot = match self.build_plain_snapshot(entry_id).await? {
            Some(snapshot) => snapshot,
            None => {
                info!(
                    entry_id = %entry_id,
                    "restore_plain.execute: no text/plain representation available — caller should fall back"
                );
                return Ok(PlainRestoreOutcome::NoPlainTextAvailable);
            }
        };

        self.coordinator
            .write(snapshot, ClipboardWriteIntent::LocalRestore)
            .await?;

        Ok(PlainRestoreOutcome::Done)
    }

    /// 收集 selection 中所有候选 rep，挑出第一份「能解析出字节」的 plain text rep
    /// 打包成单 rep snapshot。返回 `None` 表示没有任何 plain rep 可用，调用方
    /// 应当走降级路径。
    ///
    /// 候选顺序与 `RestoreClipboardSelectionUseCase::build_snapshot` 保持一致：
    /// paste_rep → primary → preview → secondary（整体去重）。这样在 selection
    /// 策略已经把 plain 选为 paste_rep 的常见场景下，第一个候选就是它，省去
    /// 多余的 resolver 调用。
    async fn build_plain_snapshot(
        &self,
        entry_id: &EntryId,
    ) -> Result<Option<SystemClipboardSnapshot>> {
        debug!(entry_id = %entry_id, "restore_plain.build_snapshot start");

        let entry = self
            .clipboard_repo
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Entry not found"))?;

        let selection = self
            .selection_repo
            .get_selection(entry_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Selection not found"))?;

        let mut candidate_ids = Vec::new();
        candidate_ids.push(selection.selection.paste_rep_id.clone());
        candidate_ids.push(selection.selection.primary_rep_id.clone());
        candidate_ids.push(selection.selection.preview_rep_id.clone());
        candidate_ids.extend(selection.selection.secondary_rep_ids.clone());

        let mut seen = std::collections::HashSet::new();
        candidate_ids.retain(|rep_id| seen.insert(rep_id.clone()));

        for rep_id in &candidate_ids {
            let rep = match self
                .representation_repo
                .get_representation(&entry.event_id, rep_id)
                .await?
            {
                Some(rep) => rep,
                None => continue,
            };

            if !Self::is_plain_text(&rep) {
                continue;
            }

            match self.resolve_bytes(&rep).await {
                Ok(bytes) => {
                    let observed = ObservedClipboardRepresentation::new(
                        rep.id.clone(),
                        rep.format_id.clone(),
                        rep.mime_type.clone(),
                        bytes,
                    );

                    debug!(
                        entry_id = %entry_id,
                        event_id = %entry.event_id,
                        plain_rep_id = %rep.id,
                        size_bytes = observed.size_bytes(),
                        "restore_plain.build_snapshot packed plain representation"
                    );

                    return Ok(Some(SystemClipboardSnapshot {
                        ts_ms: chrono::Utc::now().timestamp_millis(),
                        representations: vec![observed],
                    }));
                }
                Err(err) => {
                    // 候选 plain rep 解析失败，继续尝试下一个候选。常见原因：
                    // Staged 状态下 cache+spool 都拿不到字节（与多格式恢复对
                    // secondary rep 的处理对称——跳过 + warn，不打断流程）。
                    warn!(
                        entry_id = %entry_id,
                        rep_id = %rep.id,
                        error = %err,
                        "restore_plain.build_snapshot: skipping plain rep due to resolve failure"
                    );
                    continue;
                }
            }
        }

        Ok(None)
    }

    fn is_plain_text(rep: &PersistedClipboardRepresentation) -> bool {
        is_plain_text_mime_or_format(rep.mime_type.as_ref(), &rep.format_id)
    }

    async fn resolve_bytes(&self, rep: &PersistedClipboardRepresentation) -> Result<Vec<u8>> {
        match self.payload_resolver.resolve(rep).await? {
            ResolvedClipboardPayload::Inline { bytes, .. } => Ok(bytes),
            ResolvedClipboardPayload::BlobRef { blob_id, .. } => {
                self.blob_store.get(&blob_id).await.map_err(|err| {
                    anyhow::anyhow!(
                        "Failed to fetch plain text blob {} for rep {}: {}",
                        blob_id,
                        rep.id,
                        err
                    )
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! 锁定本用例对 facade 的两条契约路径：
    //! - 有 plain rep → `Done` + 系统剪贴板仅收到含单一 plain rep 的 snapshot
    //! - 无 plain rep → `NoPlainTextAvailable` + 系统剪贴板未被写入
    //!
    //! 所有 port 通过 `mockall::mock!` 注入。未在测试中 `expect_*` 的方法
    //! mockall 会在调用时 panic（strict 模式），等同于"不应被调用"的断言。
    use super::*;
    use anyhow::Result as AnyResult;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use uc_core::clipboard::{
        ClipboardChangeOrigin, ClipboardEntry, ClipboardIntegrationMode, ClipboardRepositoryError,
        ClipboardSelection, ClipboardSelectionDecision, MimeType, PersistedClipboardRepresentation,
        SelectionPolicyVersion, SystemClipboardSnapshot,
    };
    use uc_core::ids::{EntryId, EventId, FormatId, RepresentationId};
    use uc_core::ports::clipboard::{
        ClipboardChangeOriginPort, ClipboardPayloadResolverPort, GetClipboardEntryPort,
        GetRepresentationPort, PayloadResolveError, ResolvedClipboardPayload, SystemClipboardPort,
    };
    use uc_core::ports::ClipboardSelectionRepositoryPort;
    use uc_core::BlobId;

    fn make_rep(
        id: &str,
        mime: &str,
        format: &str,
        data: &[u8],
    ) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::from(id),
            FormatId::from(format),
            Some(MimeType(mime.to_string())),
            data.len() as i64,
            Some(data.to_vec()),
            None,
        )
    }

    fn make_entry(entry_id: &str, event_id: &str) -> ClipboardEntry {
        ClipboardEntry::new(EntryId::from(entry_id), EventId::from(event_id), 0, None, 0)
            .with_delivery_tracked(false)
    }

    fn make_selection(
        entry_id: &str,
        paste_rep_id: &str,
        secondary: Vec<&str>,
    ) -> ClipboardSelectionDecision {
        ClipboardSelectionDecision::new(
            EntryId::from(entry_id),
            ClipboardSelection {
                primary_rep_id: RepresentationId::from(paste_rep_id),
                secondary_rep_ids: secondary.into_iter().map(RepresentationId::from).collect(),
                preview_rep_id: RepresentationId::from(paste_rep_id),
                paste_rep_id: RepresentationId::from(paste_rep_id),
                policy_version: SelectionPolicyVersion::V1,
            },
        )
    }

    mockall::mock! {
        EntryRepo {}
        #[async_trait]
        impl GetClipboardEntryPort for EntryRepo {
            async fn get_entry(
                &self,
                entry_id: &EntryId,
            ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError>;
        }
    }

    mockall::mock! {
        SelectionRepo {}
        #[async_trait]
        impl ClipboardSelectionRepositoryPort for SelectionRepo {
            async fn get_selection(
                &self,
                entry_id: &EntryId,
            ) -> AnyResult<Option<ClipboardSelectionDecision>>;
            async fn delete_selection(&self, entry_id: &EntryId) -> AnyResult<()>;
        }
    }

    /// Hand-rolled fake for the narrow `GetRepresentationPort` — the only
    /// representation capability this use case consumes. Returns the matching
    /// rep by id from a fixed list.
    struct FakeRepRepo {
        reps: Vec<PersistedClipboardRepresentation>,
    }

    #[async_trait]
    impl GetRepresentationPort for FakeRepRepo {
        async fn get_representation(
            &self,
            _: &EventId,
            rep_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            Ok(self.reps.iter().find(|r| &r.id == rep_id).cloned())
        }
    }

    mockall::mock! {
        Resolver {}
        #[async_trait]
        impl ClipboardPayloadResolverPort for Resolver {
            async fn resolve(
                &self,
                representation: &PersistedClipboardRepresentation,
            ) -> Result<ResolvedClipboardPayload, PayloadResolveError>;
        }
    }

    mockall::mock! {
        BlobReader {}
        #[async_trait]
        impl uc_core::blob::ports::BlobReaderPort for BlobReader {
            async fn get(&self, blob_id: &BlobId) -> AnyResult<Vec<u8>>;
        }
    }

    mockall::mock! {
        SystemClipboard {}
        #[async_trait]
        impl SystemClipboardPort for SystemClipboard {
            fn read_snapshot(&self) -> AnyResult<SystemClipboardSnapshot>;
            fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> AnyResult<()>;
        }
    }

    mockall::mock! {
        ChangeOrigin {}
        #[async_trait]
        impl ClipboardChangeOriginPort for ChangeOrigin {
            async fn set_next_origin(&self, origin: ClipboardChangeOrigin, ttl: Duration);
            async fn consume_origin_or_default(
                &self,
                default_origin: ClipboardChangeOrigin,
            ) -> ClipboardChangeOrigin;
            async fn has_pending_origin(&self) -> bool;
            async fn remember_remote_snapshot_hash(&self, snapshot_hash: String, ttl: Duration);
            async fn remember_local_snapshot_hash(&self, snapshot_hash: String, ttl: Duration);
            async fn consume_origin_for_snapshot_or_default(
                &self,
                snapshot_hash: &str,
                default_origin: ClipboardChangeOrigin,
            ) -> ClipboardChangeOrigin;
        }
    }

    /// 把 `payload_resolver.resolve` 配置成"按 inline_data 直读"——所有测试 rep
    /// 都走 inline 路径，blob_store 不会被触达。
    fn expect_inline_resolves(resolver: &mut MockResolver) {
        resolver.expect_resolve().returning(|rep| {
            let bytes = rep
                .inline_data
                .clone()
                .ok_or_else(|| PayloadResolveError::Integrity {
                    rep_id: rep.id.clone(),
                    reason: "test resolver requires inline_data".to_string(),
                })?;
            Ok(ResolvedClipboardPayload::Inline {
                mime: rep
                    .mime_type
                    .as_ref()
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                bytes,
            })
        });
    }

    /// coordinator 内部对 origin port 的两层守卫调用，本测试不验证具体守卫
    /// 细节（coordinator 自身已在 `clipboard_write/coordinator.rs` 测过），
    /// 这里只让所有相关方法以 default 行为通过。
    fn expect_permissive_origin(origin: &mut MockChangeOrigin) {
        origin
            .expect_remember_local_snapshot_hash()
            .returning(|_, _| ());
        origin.expect_set_next_origin().returning(|_, _| ());
    }

    /// 用 mockall 把每次 `write_snapshot` 收到的 snapshot 累计起来供断言查阅，
    /// 同时给 `SystemClipboardPort` 套上 Arc 引用以便测试与 coordinator 共享。
    fn recording_system_clipboard() -> (
        Arc<MockSystemClipboard>,
        Arc<Mutex<Vec<SystemClipboardSnapshot>>>,
    ) {
        let writes = Arc::new(Mutex::new(Vec::<SystemClipboardSnapshot>::new()));
        let writes_for_mock = writes.clone();
        let mut clipboard = MockSystemClipboard::new();
        clipboard.expect_write_snapshot().returning(move |snap| {
            writes_for_mock.lock().unwrap().push(snap);
            Ok(())
        });
        (Arc::new(clipboard), writes)
    }

    /// 组装一个 use case + 写入记录句柄。调用方先在 mocks 上设好 `expect_*`，
    /// 再传进来；本 helper 负责注入 coordinator 与 integration mode。
    fn build_use_case(
        entry_repo: MockEntryRepo,
        selection_repo: MockSelectionRepo,
        rep_repo: FakeRepRepo,
        resolver: MockResolver,
        blob_reader: MockBlobReader,
        clipboard: Arc<MockSystemClipboard>,
        origin: MockChangeOrigin,
    ) -> RestoreClipboardEntryAsPlainTextUseCase {
        let coordinator = Arc::new(ClipboardWriteCoordinator::new(clipboard, Arc::new(origin)));
        RestoreClipboardEntryAsPlainTextUseCase::new(
            Arc::new(entry_repo),
            coordinator,
            Arc::new(selection_repo),
            Arc::new(rep_repo),
            Arc::new(resolver),
            Arc::new(blob_reader),
            ClipboardIntegrationMode::Full,
        )
    }

    /// 条目同时持有 plain + html 两份 rep，paste_rep 指向 plain：
    /// 用例应当只把 plain rep 写入系统剪贴板，绝不把 html 一起带上。
    #[tokio::test]
    async fn done_when_plain_rep_is_paste_rep() {
        let entry = make_entry("entry-1", "event-1");
        let plain = make_rep("rep-plain", "text/plain", "text", b"hello world");
        let html = make_rep(
            "rep-html",
            "text/html",
            "html",
            b"<p>hello <b>world</b></p>",
        );
        let decision = make_selection("entry-1", "rep-plain", vec!["rep-html"]);

        let mut entry_repo = MockEntryRepo::new();
        entry_repo
            .expect_get_entry()
            .returning(move |_| Ok(Some(entry.clone())));

        let mut selection_repo = MockSelectionRepo::new();
        selection_repo
            .expect_get_selection()
            .returning(move |_| Ok(Some(decision.clone())));

        let rep_repo = FakeRepRepo {
            reps: vec![plain.clone(), html],
        };

        let mut resolver = MockResolver::new();
        expect_inline_resolves(&mut resolver);

        let blob_reader = MockBlobReader::new();
        let (clipboard, writes) = recording_system_clipboard();
        let mut origin = MockChangeOrigin::new();
        expect_permissive_origin(&mut origin);

        let uc = build_use_case(
            entry_repo,
            selection_repo,
            rep_repo,
            resolver,
            blob_reader,
            clipboard,
            origin,
        );

        let outcome = uc
            .execute(&EntryId::from("entry-1"))
            .await
            .expect("execute should succeed");
        assert_eq!(outcome, PlainRestoreOutcome::Done);

        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), 1, "exactly one OS clipboard write expected");
        let reps = &writes[0].representations;
        assert_eq!(reps.len(), 1, "snapshot must contain only the plain rep");
        assert_eq!(reps[0].id, plain.id);
        assert_eq!(reps[0].expect_inline_bytes(), b"hello world");
    }

    /// 条目根本没有 plain 表示（典型富文本场景：复制自 PDF 等）。用例应当
    /// 返回 `NoPlainTextAvailable` 让 facade 走降级路径，并且**不**写入系统
    /// 剪贴板——否则降级时会出现两次 OS 写入。
    ///
    /// 这里 system clipboard 与 origin 都不 `expect_*` 任何方法，mockall 在
    /// 这两者上的任何调用都会 panic——就是"绝不写入"的断言。
    #[tokio::test]
    async fn no_plain_when_only_rich_text_reps_present() {
        let entry = make_entry("entry-2", "event-2");
        let html = make_rep(
            "rep-html",
            "text/html",
            "html",
            b"<p>only rich text here</p>",
        );
        let rtf = make_rep(
            "rep-rtf",
            "text/rtf",
            "rtf",
            b"{\\rtf1\\ansi only rich text}",
        );
        let decision = make_selection("entry-2", "rep-html", vec!["rep-rtf"]);

        let mut entry_repo = MockEntryRepo::new();
        entry_repo
            .expect_get_entry()
            .returning(move |_| Ok(Some(entry.clone())));

        let mut selection_repo = MockSelectionRepo::new();
        selection_repo
            .expect_get_selection()
            .returning(move |_| Ok(Some(decision.clone())));

        let rep_repo = FakeRepRepo {
            reps: vec![html, rtf],
        };

        let resolver = MockResolver::new();
        let blob_reader = MockBlobReader::new();
        let clipboard = Arc::new(MockSystemClipboard::new());
        let origin = MockChangeOrigin::new();

        let uc = build_use_case(
            entry_repo,
            selection_repo,
            rep_repo,
            resolver,
            blob_reader,
            clipboard,
            origin,
        );

        let outcome = uc
            .execute(&EntryId::from("entry-2"))
            .await
            .expect("execute should succeed");
        assert_eq!(outcome, PlainRestoreOutcome::NoPlainTextAvailable);
    }

    /// 极端但真实的场景：paste_rep 选了 html，plain 只挂在 secondary 列表里。
    /// 用例必须扫完候选才能定位 plain，不能因为 paste_rep 不是 plain 就放弃。
    #[tokio::test]
    async fn done_when_plain_rep_is_only_in_secondary() {
        let entry = make_entry("entry-3", "event-3");
        let html = make_rep("rep-html", "text/html", "html", b"<p>hi</p>");
        let plain = make_rep("rep-plain", "text/plain", "text", b"hi");
        let decision = make_selection("entry-3", "rep-html", vec!["rep-plain"]);

        let mut entry_repo = MockEntryRepo::new();
        entry_repo
            .expect_get_entry()
            .returning(move |_| Ok(Some(entry.clone())));

        let mut selection_repo = MockSelectionRepo::new();
        selection_repo
            .expect_get_selection()
            .returning(move |_| Ok(Some(decision.clone())));

        let rep_repo = FakeRepRepo {
            reps: vec![html, plain.clone()],
        };

        let mut resolver = MockResolver::new();
        expect_inline_resolves(&mut resolver);

        let blob_reader = MockBlobReader::new();
        let (clipboard, writes) = recording_system_clipboard();
        let mut origin = MockChangeOrigin::new();
        expect_permissive_origin(&mut origin);

        let uc = build_use_case(
            entry_repo,
            selection_repo,
            rep_repo,
            resolver,
            blob_reader,
            clipboard,
            origin,
        );

        let outcome = uc
            .execute(&EntryId::from("entry-3"))
            .await
            .expect("execute should succeed");
        assert_eq!(outcome, PlainRestoreOutcome::Done);

        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].representations.len(), 1);
        assert_eq!(writes[0].representations[0].id, plain.id);
    }
}
