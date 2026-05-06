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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, warn};

use uc_core::mobile_sync::{StagedFile, StagedFileUri};
use uc_core::ports::{MobileFileStagingError, MobileFileStagingPort};

/// 子目录名 —— `<cache_root>/mobile_inbound/<scope_id>/<file>`。
const STAGING_SUBDIR: &str = "mobile_inbound";

/// sanitize 失败时的兜底文件名。
const FALLBACK_FILENAME: &str = "staged.bin";

pub struct FilesystemMobileFileStaging {
    /// `stage_file` 写盘用的 root(典型: `AppPaths.file_cache_dir`)。
    cache_root: PathBuf,
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
        Arc::new(Self { cache_root })
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
}
