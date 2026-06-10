//! `FilesystemMobileFileStaging` —— [`MobileFileStagingPort`] 的真实实现。
//!
//! 把 mobile 入站(`PUT /file/{name}`)的裸字节落到本机文件系统的 cache_dir
//! 子目录,并派生 `file:///...` 形态的本机 URI 给 file-list rep 使用。
//!
//! ## 路径布局
//!
//! ```text
//! <cache_root>/mobile_inbound/<scope_id>/<sanitized_name>
//! ```
//!
//! - `cache_root`:由 bootstrap 注入的 `AppPaths.file_cache_dir`,与 P2P
//!   入站 blob 缓存(`<cache_root>/iroh-blobs/...`)同根。
//! - `scope_id`:use case 端用 uuid v4 截 12 位生成的 nonce,与 entry_id
//!   解耦(后者在 ApplyInbound 内部才生成)。
//! - `sanitized_name`:basename only(去掉所有 `/` `\` `:` 控制符 + 前后
//!   `.` 与空白);全是非法字符时 fallback `staged.bin`。
//!
//! ## URI 跨平台
//!
//! 直接用 [`url::Url::from_file_path`] 转,自动处理:
//! - macOS / Linux:`file:///Users/.../foo.pdf`
//! - Windows:`file:///C:/Users/.../foo.pdf`(盘符前补斜杠)
//! - 文件名含 spaces / non-ASCII → percent encoding 自动
//!
//! ## 清理策略(v1: 不清)
//!
//! 不在启动期 wipe `<cache_root>/mobile_inbound/`,因为已落库的 clipboard
//! entry 的 file-list rep bytes 是 `file:///<cache_root>/mobile_inbound/...`
//! 形态的 URI —— 进程重启后这些历史 entry 仍可能被前端 / OS paste 引用,
//! wipe 会让它们瞬间失效。
//!
//! 同样的理由:CLI debug fallback 也复用同 adapter,debug subcommand 是多
//! 进程串行执行(每次构造一份 adapter),wipe 会破坏 `put-file` 与后续
//! `get-file` 之间的字节持久性。
//!
//! 运行期 TTL sweep + 体积限制留 v2。v1 假设:cache_root 体积可控(单次
//! PUT 上限 16 MiB,且 mobile sync 实际频次低),累积不会构成 OS 压力。
//!
//! ## `read_by_uri` 信任模型
//!
//! 信任来源是 OS 剪贴板 —— 任何能被 paste rep 携带的 file URI,在桌面 OS
//! 层面已对所有运行中 app 开放读权限。已配对的 iPhone 经 basic auth 通过
//! 后语义上等价于一台已信任的设备,与本机 paste 操作对称。adapter 不做
//! 路径白名单:URI 字面 → `tokio::fs::read`,文件不存在 → `NotFound`,
//! 读盘失败 → `Io`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;

use uc_core::mobile_sync::{StagedFile, StagedFileUri, StagingHandle};
use uc_core::ports::{MobileFileStagingError, MobileFileStagingPort};

/// 子目录名 —— `<cache_root>/mobile_inbound/<scope_id>/<file>`。
const STAGING_SUBDIR: &str = "mobile_inbound";

/// sanitize 失败时的兜底文件名。
const FALLBACK_FILENAME: &str = "staged.bin";

/// 单次 streaming staging 会话的内部状态(adapter 私有,不出 crate)。
///
/// `path` / `sanitized_name` / `scope_segment` 都在 begin 阶段一次性算定,
/// finalize 拼 URI / abort 尝试清空 scope 目录都直接用,避免 finalize/abort
/// 阶段再重算 sanitize。
struct OpenStagingSession {
    file: File,
    path: PathBuf,
    sanitized_name: String,
    scope_segment: String,
}

pub struct FilesystemMobileFileStaging {
    /// `stage_file` 写盘用的 root(典型: `AppPaths.file_cache_dir`)。
    cache_root: PathBuf,
    /// 进行中的 streaming staging 会话:token → 打开的 File + 元数据。
    /// 用 tokio Mutex(append_stage_chunk 在异步上下文写盘,持锁跨 await
    /// 必须用 async-aware mutex 否则会阻塞 runtime worker)。
    open_sessions: Mutex<HashMap<Uuid, OpenStagingSession>>,
}

impl FilesystemMobileFileStaging {
    /// 用 `cache_root`(典型: `AppPaths.file_cache_dir`)构造 adapter。
    ///
    /// **不**做启动 wipe(见模块文档"清理策略"):已落库的 clipboard entry
    /// 引用的 file URI 必须跨进程持久,wipe 会让它们失效。
    ///
    /// 启动期 best-effort `create_dir_all(cache_root)` —— `stage_file` 首次
    /// 写盘前需要 staging 子目录的父目录存在,避免首启 / 全新机器上首笔
    /// stage 因父目录缺失而失败。
    pub fn new(cache_root: PathBuf) -> Arc<Self> {
        if let Err(err) = std::fs::create_dir_all(&cache_root) {
            warn!(
                cache_root = %cache_root.display(),
                error = %err,
                "mobile_sync staging: failed to ensure cache_root exists at startup"
            );
        }
        debug!(
            cache_root = %cache_root.display(),
            "mobile_sync staging: adapter ready"
        );
        Arc::new(Self {
            cache_root,
            open_sessions: Mutex::new(HashMap::new()),
        })
    }

    fn staging_root(&self) -> PathBuf {
        self.cache_root.join(STAGING_SUBDIR)
    }
}

#[async_trait]
impl MobileFileStagingPort for FilesystemMobileFileStaging {
    async fn read_by_uri(&self, uri: &str) -> Result<Vec<u8>, MobileFileStagingError> {
        // 解析 URI → path。url 的 `to_file_path` 自动 percent decode +
        // 跨平台(Windows 盘符 / Linux/macOS 普通路径都吃)。
        let parsed = url::Url::parse(uri).map_err(|e| {
            MobileFileStagingError::Io(format!("URI parse failed for {uri:?}: {e}"))
        })?;
        let path = parsed.to_file_path().map_err(|_| {
            MobileFileStagingError::Io(format!(
                "URI is not a file:// URL or has no usable path: {uri:?}"
            ))
        })?;

        let bytes = tokio::fs::read(&path).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                debug!(
                    uri = %uri,
                    path = %path.display(),
                    "mobile_sync staging: read_by_uri path not found"
                );
                MobileFileStagingError::NotFound
            } else {
                MobileFileStagingError::Io(format!("read {} failed: {err}", path.display()))
            }
        })?;
        let bytes_len = bytes.len();
        if matches!(bytes_len, 0) {
            debug!(uri = %uri, "mobile_sync staging: read_by_uri served empty file");
        } else {
            debug!(
                uri = %uri,
                path = %path.display(),
                bytes = bytes_len,
                "mobile_sync staging: read_by_uri served file bytes"
            );
        }
        Ok(bytes)
    }

    async fn stage_file(
        &self,
        scope_id: &str,
        data_name: &str,
        mime: &str,
        bytes: Vec<u8>,
    ) -> Result<StagedFile, MobileFileStagingError> {
        // sanitize_basename 永远返回非空字符串(失败兜底 FALLBACK_FILENAME)。
        // adapter 不抛 InvalidDataName —— 该变体保留给未来更严格的 sanitize
        // 策略(比如禁止 fallback 兜底)。
        let sanitized = sanitize_basename(data_name);

        let scope_segment = sanitize_scope(scope_id);
        let entry_dir = self.staging_root().join(&scope_segment);
        tokio::fs::create_dir_all(&entry_dir).await.map_err(|e| {
            MobileFileStagingError::Io(format!(
                "create staging dir {} failed: {e}",
                entry_dir.display()
            ))
        })?;

        let file_path = entry_dir.join(&sanitized);
        let bytes_len = bytes.len();
        tokio::fs::write(&file_path, &bytes).await.map_err(|e| {
            MobileFileStagingError::Io(format!(
                "write staging file {} failed: {e}",
                file_path.display()
            ))
        })?;

        let uri = path_to_file_uri(&file_path)?;
        debug!(
            scope_id = %scope_segment,
            data_name = %data_name,
            sanitized = %sanitized,
            mime = %mime,
            bytes = bytes_len,
            uri = %uri,
            "mobile_sync staging: file written"
        );

        Ok(StagedFile {
            uri: StagedFileUri::new(uri),
            sanitized_name: sanitized,
        })
    }

    async fn begin_stage(
        &self,
        scope_id: &str,
        data_name: &str,
        mime: &str,
    ) -> Result<StagingHandle, MobileFileStagingError> {
        let sanitized = sanitize_basename(data_name);
        let scope_segment = sanitize_scope(scope_id);
        let entry_dir = self.staging_root().join(&scope_segment);
        tokio::fs::create_dir_all(&entry_dir).await.map_err(|e| {
            MobileFileStagingError::Io(format!(
                "create staging dir {} failed: {e}",
                entry_dir.display()
            ))
        })?;

        let file_path = entry_dir.join(&sanitized);
        // create(true) + truncate(true):同一 scope 内重名 → 后者覆盖。scope_id
        // 由调用方按"每次入站事件取 uuid nonce"语义生成,正常路径下不会撞;
        // 撞了也是上游 bug,本层不藏。
        let file = tokio::fs::File::create(&file_path).await.map_err(|e| {
            MobileFileStagingError::Io(format!(
                "create staging file {} failed: {e}",
                file_path.display()
            ))
        })?;

        let handle = StagingHandle::new();
        let token = handle.token();
        let mut sessions = self.open_sessions.lock().await;
        sessions.insert(
            token,
            OpenStagingSession {
                file,
                path: file_path.clone(),
                sanitized_name: sanitized.clone(),
                scope_segment: scope_segment.clone(),
            },
        );
        debug!(
            handle = %token,
            scope_id = %scope_segment,
            data_name = %data_name,
            sanitized = %sanitized,
            mime = %mime,
            path = %file_path.display(),
            "mobile_sync staging: streaming session opened"
        );
        Ok(handle)
    }

    async fn append_stage_chunk(
        &self,
        handle: &StagingHandle,
        chunk: &[u8],
    ) -> Result<(), MobileFileStagingError> {
        if chunk.is_empty() {
            return Ok(());
        }
        let token = handle.token();
        let mut sessions = self.open_sessions.lock().await;
        let session = sessions.get_mut(&token).ok_or_else(|| {
            MobileFileStagingError::Io(format!(
                "append_stage_chunk: unknown or already-consumed handle {token}"
            ))
        })?;
        session.file.write_all(chunk).await.map_err(|e| {
            MobileFileStagingError::Io(format!(
                "append_stage_chunk write {} bytes to {} failed: {e}",
                chunk.len(),
                session.path.display()
            ))
        })?;
        Ok(())
    }

    async fn finalize_stage(
        &self,
        handle: StagingHandle,
    ) -> Result<StagedFile, MobileFileStagingError> {
        let token = handle.token();
        let mut session = {
            let mut sessions = self.open_sessions.lock().await;
            sessions.remove(&token).ok_or_else(|| {
                MobileFileStagingError::Io(format!(
                    "finalize_stage: unknown or already-consumed handle {token}"
                ))
            })?
        };
        // flush + fsync:后续 SyncDoc 阶段会立即 add_path 给 iroh 发布,
        // 未 sync 的 page cache 在 crash 时可能丢,显式 sync_all 把这条 race
        // 关掉。代价是大文件多几十 ms,可接受。
        session.file.flush().await.map_err(|e| {
            MobileFileStagingError::Io(format!(
                "finalize_stage flush {} failed: {e}",
                session.path.display()
            ))
        })?;
        session.file.sync_all().await.map_err(|e| {
            MobileFileStagingError::Io(format!(
                "finalize_stage sync_all {} failed: {e}",
                session.path.display()
            ))
        })?;
        // 显式 drop 让 fd 释放(否则要等 session 出作用域)。
        drop(session.file);

        let uri = path_to_file_uri(&session.path)?;
        debug!(
            handle = %token,
            scope_id = %session.scope_segment,
            sanitized = %session.sanitized_name,
            path = %session.path.display(),
            uri = %uri,
            "mobile_sync staging: streaming session finalized"
        );
        Ok(StagedFile {
            uri: StagedFileUri::new(uri),
            sanitized_name: session.sanitized_name,
        })
    }

    async fn abort_stage(&self, handle: StagingHandle) {
        let token = handle.token();
        let removed = {
            let mut sessions = self.open_sessions.lock().await;
            sessions.remove(&token)
        };
        let Some(session) = removed else {
            // 幂等:已被消费过(可能是先 finalize 后又收到失败回滚 / 双 abort)
            // → 静默 no-op。
            return;
        };
        // best-effort:先关 fd,再删文件,再尝试删空 scope 目录。任何一步失败
        // 都只 warn,不向上抛(本方法处在已经失败的路径上,二次失败不值得
        // 扰动上层错误处理)。
        drop(session.file);

        if let Err(err) = tokio::fs::remove_file(&session.path).await {
            // NotFound 不算异常 —— 半写入文件可能根本没创建过(create 后立即
            // 触发的 abort)。
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    handle = %token,
                    path = %session.path.display(),
                    error = %err,
                    "mobile_sync staging: abort_stage failed to remove partial file"
                );
            }
        }
        // 尝试删 scope 目录(只有空时才会成功;非空表明同 scope 还有别的文件,
        // 留着不动)。failure 是预期的,不报警。
        let scope_dir = self.staging_root().join(&session.scope_segment);
        let _ = tokio::fs::remove_dir(&scope_dir).await;

        debug!(
            handle = %token,
            scope_id = %session.scope_segment,
            sanitized = %session.sanitized_name,
            "mobile_sync staging: streaming session aborted"
        );
    }
}

/// 把 path 转成 `file:///...` URI(跨平台委托给 `url::Url::from_file_path`)。
/// 失败时返回 `MobileFileStagingError::Io`(几乎不会触发: file_path 是
/// adapter 自己拼的绝对路径)。
fn path_to_file_uri(path: &Path) -> Result<String, MobileFileStagingError> {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .map_err(|_| {
            MobileFileStagingError::Io(format!(
                "failed to convert path to file URI: {}",
                path.display()
            ))
        })
}

/// `data_name` 来自 iPhone 上传(可能含 `/` `\` `..` 等),adapter 必须取
/// basename only + 去掉所有不安全字符 + 兜底非空。
///
/// 与 `apply_inbound::materializer::sanitize_path_segment` 等同语义,但本
/// 模块独立实现避免跨 crate import 私有 helper。
fn sanitize_basename(value: &str) -> String {
    // 第一步: 取 basename(去 `/` 与 `\\`)
    let basename = std::path::Path::new(value)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(value);

    // 第二步: 替换危险字符 + 去前后空白 / `.`
    let cleaned: String = basename
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '\0' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();

    if trimmed.is_empty() {
        FALLBACK_FILENAME.to_string()
    } else {
        trimmed
    }
}

/// scope_id 由调用方生成,但仍按基本 path safety 做一次 sanitize ——
/// 不允许它带 `/` 跳出 staging_root。失败兜底 `unscoped`。
fn sanitize_scope(scope: &str) -> String {
    let cleaned: String = scope
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '\0' | '.' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim().to_string();
    if trimmed.is_empty() {
        "unscoped".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_adapter(cache_root: &Path) -> Arc<FilesystemMobileFileStaging> {
        FilesystemMobileFileStaging::new(cache_root.to_path_buf())
    }

    #[tokio::test]
    async fn stage_file_writes_and_returns_file_uri() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let staged = adapter
            .stage_file("scope01", "doc.pdf", "application/pdf", vec![1, 2, 3, 4])
            .await
            .expect("stage_file ok");

        assert_eq!(staged.sanitized_name, "doc.pdf");
        assert!(
            staged.uri.as_str().starts_with("file:///"),
            "uri should be file:///, got {}",
            staged.uri
        );
        assert!(
            staged.uri.as_str().ends_with("/doc.pdf"),
            "uri tail should be /doc.pdf, got {}",
            staged.uri
        );

        // 文件确实落盘
        let expected = tmp
            .path()
            .join("mobile_inbound")
            .join("scope01")
            .join("doc.pdf");
        let bytes = tokio::fs::read(&expected).await.expect("read written file");
        assert_eq!(bytes, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn stage_file_sanitizes_path_separators_in_data_name() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        // iPhone 上传 "../../etc/passwd" —— adapter 必须只取 basename
        let staged = adapter
            .stage_file("scope01", "../../etc/passwd", "text/plain", vec![0])
            .await
            .expect("stage_file ok");

        assert_eq!(staged.sanitized_name, "passwd");
        // 路径上不能有 `etc/`
        assert!(!staged.uri.as_str().contains("/etc/"));
    }

    #[tokio::test]
    async fn stage_file_falls_back_when_data_name_is_only_dots() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let staged = adapter
            .stage_file("scope01", "...", "application/octet-stream", vec![0])
            .await
            .expect("stage_file ok");

        assert_eq!(staged.sanitized_name, FALLBACK_FILENAME);
    }

    #[tokio::test]
    async fn stage_file_handles_unicode_and_spaces_in_uri() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let staged = adapter
            .stage_file("scope01", "我的 文档.pdf", "application/pdf", vec![1])
            .await
            .expect("stage_file ok");

        // url::Url::from_file_path 对 spaces 做 percent encoding
        let uri = staged.uri.as_str();
        assert!(
            uri.contains("%20"),
            "spaces should be percent-encoded: {uri}"
        );
        // 非 ASCII 也会被 percent-encoded
        assert!(
            uri.contains("%E6%88%91"),
            "汉字 should be percent-encoded: {uri}"
        );
    }

    #[tokio::test]
    async fn stage_file_isolates_scope_dirs() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        adapter
            .stage_file("scope-a", "doc.pdf", "application/pdf", vec![0xAA])
            .await
            .unwrap();
        adapter
            .stage_file("scope-b", "doc.pdf", "application/pdf", vec![0xBB])
            .await
            .unwrap();

        let a = tokio::fs::read(
            tmp.path()
                .join("mobile_inbound")
                .join("scope-a")
                .join("doc.pdf"),
        )
        .await
        .unwrap();
        let b = tokio::fs::read(
            tmp.path()
                .join("mobile_inbound")
                .join("scope-b")
                .join("doc.pdf"),
        )
        .await
        .unwrap();
        assert_eq!(a, vec![0xAA]);
        assert_eq!(b, vec![0xBB]);
    }

    // ── read_by_uri tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn read_by_uri_round_trips_freshly_staged_file() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let staged = adapter
            .stage_file("scope-r", "doc.pdf", "application/pdf", vec![0x42; 16])
            .await
            .expect("stage_file ok");

        let bytes = adapter
            .read_by_uri(staged.uri.as_str())
            .await
            .expect("read_by_uri ok");
        assert_eq!(bytes, vec![0x42; 16]);
    }

    #[tokio::test]
    async fn read_by_uri_handles_percent_encoded_uri() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let staged = adapter
            .stage_file(
                "scope01",
                "我的 文档.pdf",
                "application/pdf",
                vec![0xCC, 0xDD],
            )
            .await
            .unwrap();
        // staged URI 自带 percent encoding; adapter 必须能解回真路径
        let bytes = adapter.read_by_uri(staged.uri.as_str()).await.unwrap();
        assert_eq!(bytes, vec![0xCC, 0xDD]);
    }

    #[tokio::test]
    async fn read_by_uri_returns_not_found_for_missing_file() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        // 路径形式合法但文件不存在(scope 目录都没创建)
        let fake_path = tmp
            .path()
            .join("mobile_inbound")
            .join("phantom")
            .join("missing.bin");
        let fake_uri = url::Url::from_file_path(&fake_path).unwrap().to_string();

        let err = adapter.read_by_uri(&fake_uri).await.unwrap_err();
        assert!(matches!(err, MobileFileStagingError::NotFound));
    }

    #[tokio::test]
    async fn read_by_uri_reads_arbitrary_file_outside_cache_root() {
        // 信任模型:OS 剪贴板的 file URI 由用户主动复制建立信任,adapter
        // 不再做路径白名单。系统剪贴板原生 URI(用户在 Explorer/Finder 复制
        // 的真实文件,典型落在 `D:/Downloads/...` / `/Users/.../...`,完全
        // 落在 cache_root 之外)必须能直接读到。
        let cache_tmp = TempDir::new().unwrap();
        let other_tmp = TempDir::new().unwrap();
        let adapter = make_adapter(cache_tmp.path());

        let target = other_tmp.path().join("user-doc.pdf");
        tokio::fs::write(&target, b"%PDF-1.7 user-copy")
            .await
            .unwrap();
        let uri = url::Url::from_file_path(&target).unwrap().to_string();

        let bytes = adapter
            .read_by_uri(&uri)
            .await
            .expect("file outside cache_root must read");
        assert_eq!(bytes, b"%PDF-1.7 user-copy");
    }

    #[tokio::test]
    async fn new_creates_cache_root_when_missing() {
        // stage_file 写盘前需要 cache_root 父目录存在;new() 必须 best-effort
        // 把它建出来,避免首启 / 全新机器上首笔 stage 因父目录缺失而失败。
        let parent = TempDir::new().unwrap();
        let cache_root = parent.path().join("nested").join("file-cache");
        assert!(!cache_root.exists());

        let _adapter = FilesystemMobileFileStaging::new(cache_root.clone());
        assert!(
            cache_root.exists(),
            "new() must create cache_root best-effort"
        );
    }

    #[tokio::test]
    async fn read_by_uri_rejects_non_file_url() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let err = adapter
            .read_by_uri("https://example.com/foo")
            .await
            .unwrap_err();
        assert!(
            matches!(err, MobileFileStagingError::Io(_)),
            "expected Io for non-file:// URI, got {err:?}"
        );
    }

    #[tokio::test]
    async fn read_by_uri_rejects_unparseable_uri() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let err = adapter.read_by_uri("not a valid uri").await.unwrap_err();
        assert!(
            matches!(err, MobileFileStagingError::Io(_)),
            "expected Io for malformed URI, got {err:?}"
        );
    }

    // ── streaming stage tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn streaming_stage_round_trips_chunks_to_final_uri() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let handle = adapter
            .begin_stage("scope-stream", "video.mp4", "video/mp4")
            .await
            .expect("begin_stage ok");

        adapter
            .append_stage_chunk(&handle, &[0xAA; 1024])
            .await
            .expect("append_stage_chunk 1 ok");
        adapter
            .append_stage_chunk(&handle, &[0xBB; 512])
            .await
            .expect("append_stage_chunk 2 ok");
        // 0-byte chunk 必须是 no-op
        adapter
            .append_stage_chunk(&handle, &[])
            .await
            .expect("empty chunk ok");

        let staged = adapter
            .finalize_stage(handle)
            .await
            .expect("finalize_stage ok");

        assert_eq!(staged.sanitized_name, "video.mp4");
        assert!(staged.uri.as_str().ends_with("/video.mp4"));

        // 落盘内容 = chunk1 || chunk2
        let path = tmp
            .path()
            .join("mobile_inbound")
            .join("scope-stream")
            .join("video.mp4");
        let bytes = tokio::fs::read(&path).await.unwrap();
        assert_eq!(bytes.len(), 1024 + 512);
        assert!(bytes[..1024].iter().all(|&b| b == 0xAA));
        assert!(bytes[1024..].iter().all(|&b| b == 0xBB));
    }

    #[tokio::test]
    async fn streaming_stage_abort_removes_partial_file() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let handle = adapter
            .begin_stage("scope-abort", "partial.bin", "application/octet-stream")
            .await
            .expect("begin_stage ok");

        adapter
            .append_stage_chunk(&handle, &[0xCC; 4096])
            .await
            .expect("append ok");

        let path = tmp
            .path()
            .join("mobile_inbound")
            .join("scope-abort")
            .join("partial.bin");
        assert!(path.exists(), "file must exist mid-stream");

        adapter.abort_stage(handle).await;

        assert!(
            !path.exists(),
            "abort must remove partially-written file at {}",
            path.display()
        );
        // 空 scope 目录也应被回收
        let scope_dir = tmp.path().join("mobile_inbound").join("scope-abort");
        assert!(
            !scope_dir.exists(),
            "empty scope dir must be removed after abort"
        );
    }

    #[tokio::test]
    async fn streaming_stage_two_concurrent_handles_do_not_collide() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let h_a = adapter
            .begin_stage("scope-A", "file.bin", "application/octet-stream")
            .await
            .expect("begin A");
        let h_b = adapter
            .begin_stage("scope-B", "file.bin", "application/octet-stream")
            .await
            .expect("begin B");

        // 交叠写入两个 handle —— append 要按各自 token 路由到独立 File
        adapter.append_stage_chunk(&h_a, b"AAAA").await.unwrap();
        adapter.append_stage_chunk(&h_b, b"BBBBBB").await.unwrap();
        adapter.append_stage_chunk(&h_a, b"AA").await.unwrap();
        adapter.append_stage_chunk(&h_b, b"BB").await.unwrap();

        let staged_a = adapter.finalize_stage(h_a).await.expect("finalize A");
        let staged_b = adapter.finalize_stage(h_b).await.expect("finalize B");

        let bytes_a = adapter.read_by_uri(staged_a.uri.as_str()).await.unwrap();
        let bytes_b = adapter.read_by_uri(staged_b.uri.as_str()).await.unwrap();

        assert_eq!(bytes_a, b"AAAAAA");
        assert_eq!(bytes_b, b"BBBBBBBB");
    }

    #[tokio::test]
    async fn streaming_stage_append_after_finalize_returns_error() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let handle = adapter
            .begin_stage("scope-x", "f.bin", "application/octet-stream")
            .await
            .unwrap();
        // clone handle so we can still call append after finalize consumes one
        let stale = handle.clone();
        adapter.append_stage_chunk(&handle, b"hi").await.unwrap();
        adapter.finalize_stage(handle).await.unwrap();

        let err = adapter
            .append_stage_chunk(&stale, b"late")
            .await
            .expect_err("append on consumed handle must fail");
        assert!(matches!(err, MobileFileStagingError::Io(_)));
    }

    #[tokio::test]
    async fn streaming_stage_double_abort_is_silent() {
        let tmp = TempDir::new().unwrap();
        let adapter = make_adapter(tmp.path());

        let handle = adapter
            .begin_stage("scope-dbl", "f.bin", "application/octet-stream")
            .await
            .unwrap();
        let copy = handle.clone();
        adapter.abort_stage(handle).await;
        // 二次 abort 同一 token：不 panic、不报错
        adapter.abort_stage(copy).await;
    }

    // ── sanitize_basename direct path-traversal tests ───────────────────────
    //
    // `stage_file` 已有一条间接穿越用例(`../../etc/passwd`),这里直接打
    // `sanitize_basename` 本体,补全跨平台的穿越 / 危险字符向量。涉及分隔符的
    // 断言以"安全不变量"为主(输出绝不含分隔符 / 冒号 / 控制符),不依赖具体
    // 平台的 `Path::file_name` 行为——`\` 在 Unix 不是分隔符、在 Windows 是,
    // 若断言精确字面值会在 Windows CI 漂移。

    /// 不变量:sanitize 输出必须是一个无法跳出单层目录的、非空的文件名段。
    fn assert_safe_segment(out: &str) {
        assert!(!out.is_empty(), "sanitized name must never be empty");
        assert!(!out.contains('/'), "must not contain '/': {out:?}");
        assert!(!out.contains('\\'), "must not contain backslash: {out:?}");
        assert!(!out.contains(':'), "must not contain ':': {out:?}");
        assert!(!out.contains('\0'), "must not contain NUL: {out:?}");
        assert!(
            !out.chars().any(|c| c.is_control()),
            "must not contain control chars: {out:?}"
        );
    }

    #[test]
    fn sanitize_basename_keeps_plain_filename() {
        assert_eq!(sanitize_basename("doc.pdf"), "doc.pdf");
        assert_eq!(
            sanitize_basename("my report 2026.txt"),
            "my report 2026.txt"
        );
    }

    #[test]
    fn sanitize_basename_strips_unix_traversal_to_basename() {
        // `/` 在所有平台都是分隔符 → file_name 取最后一段
        assert_eq!(sanitize_basename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_basename("/etc/shadow"), "shadow");
        assert_eq!(sanitize_basename("a/b/c/secret.key"), "secret.key");
    }

    #[test]
    fn sanitize_basename_neutralizes_windows_backslash_traversal() {
        // Unix 下 `\` 靠字符替换兜底,Windows 下 file_name 直接取尾段;两个平台
        // 输出不同,但安全不变量一致:绝不残留分隔符。
        for input in [
            "..\\..\\windows\\system32\\drivers\\etc\\hosts",
            "..\\secret.txt",
            "C:\\Users\\victim\\.ssh\\id_rsa",
        ] {
            assert_safe_segment(&sanitize_basename(input));
        }
    }

    #[test]
    fn sanitize_basename_strips_colon_drive_and_ads() {
        // `:` 既是 Windows 盘符也是 NTFS ADS 分隔符 → 必须被打平
        for input in ["file:name.txt", "secret.txt:$DATA", "C:\\x\\f.txt"] {
            let out = sanitize_basename(input);
            assert_safe_segment(&out);
            assert!(!out.contains(':'), "colon must be stripped: {out:?}");
        }
    }

    #[test]
    fn sanitize_basename_replaces_nul_and_control_chars() {
        // 这些输入不含路径分隔符,跨平台结果一致,可断言精确字面值。
        assert_eq!(sanitize_basename("evil\0.txt"), "evil_.txt");
        assert_eq!(sanitize_basename("tab\tname.bin"), "tab_name.bin");
        assert_eq!(sanitize_basename("nl\nfile"), "nl_file");
    }

    #[test]
    fn sanitize_basename_strips_leading_and_trailing_dots() {
        // basename 语义而非 dotfile 语义:前后点被裁掉。
        assert_eq!(sanitize_basename(".gitignore"), "gitignore");
        assert_eq!(sanitize_basename("archive.tar.gz."), "archive.tar.gz");
        assert_eq!(sanitize_basename("...weird..."), "weird");
    }

    #[test]
    fn sanitize_basename_falls_back_when_nothing_safe_remains() {
        // 任何输入都不能产出不安全段,即便整体非法。
        for input in ["", "   ", "...", "..", ".", "/", "////", "\0\0\0"] {
            assert_safe_segment(&sanitize_basename(input));
        }
        // 明确锁定几个会走兜底的输入
        assert_eq!(sanitize_basename("..."), FALLBACK_FILENAME);
        assert_eq!(sanitize_basename(""), FALLBACK_FILENAME);
        assert_eq!(sanitize_basename("   "), FALLBACK_FILENAME);
        assert_eq!(sanitize_basename(".."), FALLBACK_FILENAME);
    }

    // ── sanitize_scope tests ────────────────────────────────────────────────
    //
    // scope_id 由调用方生成,但仍须一次 path-safety sanitize,防止它带 `/`
    // 跳出 staging_root。sanitize_scope 是纯字符映射(不走 Path::file_name),
    // 故输出跨平台确定,可断言精确字面值。

    #[test]
    fn sanitize_scope_neutralizes_separators_dots_and_controls() {
        assert_eq!(sanitize_scope("scope01"), "scope01");
        assert_eq!(sanitize_scope("a/b"), "a_b");
        assert_eq!(sanitize_scope("a\\b"), "a_b");
        // 穿越向量整体被打平,绝不残留分隔符或点
        let out = sanitize_scope("../../evil");
        assert!(!out.contains('/'), "no slash: {out:?}");
        assert!(!out.contains('\\'), "no backslash: {out:?}");
        assert!(!out.contains('.'), "no dot: {out:?}");
    }

    #[test]
    fn sanitize_scope_falls_back_to_unscoped_when_empty() {
        assert_eq!(sanitize_scope(""), "unscoped");
        assert_eq!(sanitize_scope("   "), "unscoped");
    }
}
