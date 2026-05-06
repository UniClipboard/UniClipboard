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
//! ## `read_by_uri` 白名单(P5a.10 真机回归后扩展)
//!
//! P5a.3.5 初版只允许 `<cache_root>/...` 之下的 URI(假设所有 File rep
//! 都来自 mobile_sync 入站派生)。真机踩坑:Windows 资源管理器 / Finder
//! 用户主动复制本地文件到剪贴板时,paste rep 里的 URI 是真实文件路径
//! (`file:///D:/Downloads/...` / `file:///Users/.../Documents/...`),
//! 不在 cache_root 之下,被严格白名单挡掉,iOS Shortcut 拿到 HTTP 500。
//!
//! 扩展后白名单 = `cache_root ∪ home_dir`:
//!
//! - **`cache_root`**:覆盖 mobile_sync 入站派生 + 后续 P2P blob 派生
//!   (`<cache_root>/iroh-files/...`)等系统内部生成的 URI
//! - **`home_dir`**:覆盖系统剪贴板原生 file URI(用户在 Explorer/Finder
//!   主动复制的真实文件)。语义安全模型:用户主动复制 = 主动授权 iPhone
//!   可读,与桌面 OS 的剪贴板权限模型一致(任何运行中的 app 都能读这些
//!   字节)
//!
//! 仍然挡掉的:`/etc/passwd`、`C:\Windows\System32\...`、`/root/...`、其它
//! 用户的 home(多用户机器)等系统/管理路径 —— canonicalize 后 starts_with
//! 检查不命中任何 root → `NotFound`。
//!
//! `home_dir` 解析失败(env 变量不存在 / canonicalize 失败)时,白名单
//! 退化到 `cache_root` 单根,行为等同 P5a.3.5 初版,系统剪贴板原生 URI
//! 仍会被拒 —— 可接受的降级,不会泄露任何字节。

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
    /// `stage_file` 写盘用的 root,字面 PathBuf。`canonical_cache_root`
    /// 派生自它(canonicalize),启动时一次性算好缓存。
    cache_root: PathBuf,
    /// `read_by_uri` 白名单根之一:cache_root 的 canonical 形态(macOS 上
    /// `/var` → `/private/var` 之类符号链接已展开),用于 starts_with 校验。
    canonical_cache_root: PathBuf,
    /// `read_by_uri` 白名单根之二:用户家目录的 canonical 形态。可能为
    /// `None` —— env 变量缺失 / canonicalize 失败时降级到 cache-root-only
    /// 白名单(系统剪贴板原生 file URI 会被拒,但不泄露字节)。
    canonical_home_root: Option<PathBuf>,
}

impl FilesystemMobileFileStaging {
    /// 用 `cache_root`(典型: `AppPaths.file_cache_dir`)构造 adapter。
    ///
    /// **不**做启动 wipe(见模块文档"清理策略"):已落库的 clipboard entry
    /// 引用的 file URI 必须跨进程持久,wipe 会让它们失效。
    ///
    /// 启动期做两件 best-effort 准备:
    /// 1. `create_dir_all(cache_root)` —— 让首启 / 全新机器上首笔 read_by_uri
    ///    不会因目录还没建过而 canonicalize 失败
    /// 2. canonicalize cache_root 与 home dir,缓存为两根白名单(每次
    ///    read_by_uri 不再重做 IO + 不会因运行期 cache_root 被删而失败)
    pub fn new(cache_root: PathBuf) -> Arc<Self> {
        if let Err(err) = std::fs::create_dir_all(&cache_root) {
            warn!(
                cache_root = %cache_root.display(),
                error = %err,
                "mobile_sync staging: failed to ensure cache_root exists at startup (will fall back to literal path)"
            );
        }
        let canonical_cache_root = std::fs::canonicalize(&cache_root).unwrap_or_else(|err| {
            warn!(
                cache_root = %cache_root.display(),
                error = %err,
                "mobile_sync staging: failed to canonicalize cache_root, falling back to literal path"
            );
            cache_root.clone()
        });
        let canonical_home_root = detect_home_dir().and_then(|home| {
            std::fs::canonicalize(&home)
                .map_err(|err| {
                    warn!(
                        home = %home.display(),
                        error = %err,
                        "mobile_sync staging: failed to canonicalize home dir; system clipboard file URIs in home tree will be rejected"
                    );
                    err
                })
                .ok()
        });
        debug!(
            cache_root = %canonical_cache_root.display(),
            home_root = ?canonical_home_root.as_ref().map(|p| p.display().to_string()),
            "mobile_sync staging: adapter ready"
        );
        Arc::new(Self {
            cache_root,
            canonical_cache_root,
            canonical_home_root,
        })
    }

    /// 测试专用构造入口:跳过 home dir env 探测,直接注入两根。让 unit test
    /// 在 TempDir 上模拟"home dir 之下任意路径"白名单行为,无需真实 `$HOME`。
    #[cfg(test)]
    pub(crate) fn new_for_tests(cache_root: PathBuf, home_root: Option<PathBuf>) -> Arc<Self> {
        std::fs::create_dir_all(&cache_root).ok();
        let canonical_cache_root =
            std::fs::canonicalize(&cache_root).unwrap_or_else(|_| cache_root.clone());
        let canonical_home_root = home_root.and_then(|h| std::fs::canonicalize(&h).ok());
        Arc::new(Self {
            cache_root,
            canonical_cache_root,
            canonical_home_root,
        })
    }

    fn staging_root(&self) -> PathBuf {
        self.cache_root.join(STAGING_SUBDIR)
    }

    /// 检查 canonical_path 是否落在任一白名单根下。两根任一命中即放行。
    fn is_path_in_whitelist(&self, canonical_path: &Path) -> bool {
        if canonical_path.starts_with(&self.canonical_cache_root) {
            return true;
        }
        if let Some(home) = self.canonical_home_root.as_ref() {
            if canonical_path.starts_with(home) {
                return true;
            }
        }
        false
    }
}

/// 跨平台拿用户家目录。Unix-like 用 `$HOME`,Windows 用 `%USERPROFILE%`。
/// 失败返 `None` —— 调用方降级到 cache-root-only 白名单。
///
/// 不引入 `dirs` / `directories` crate:env 变量在 daemon 实际运行环境
/// (login 用户上下文 / Tauri / CLI fallback)都可用,边界 case(USERPROFILE
/// 缺失退到 HOMEDRIVE+HOMEPATH 等)对真机不构成实际影响。后续若有 Windows
/// 服务账号场景再考虑切到 `dirs`。
fn detect_home_dir() -> Option<PathBuf> {
    let env_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var(env_var).ok().map(PathBuf::from)
}

#[async_trait]
impl MobileFileStagingPort for FilesystemMobileFileStaging {
    async fn read_by_uri(&self, uri: &str) -> Result<Vec<u8>, MobileFileStagingError> {
        // 1. 解析 URI → path。url 0.5 的 `to_file_path` 自动 percent decode +
        //    跨平台(Windows 盘符 / Linux/macOS 普通路径都吃)。
        let parsed = url::Url::parse(uri).map_err(|e| {
            MobileFileStagingError::Io(format!("URI parse failed for {uri:?}: {e}"))
        })?;
        let path = parsed.to_file_path().map_err(|_| {
            MobileFileStagingError::Io(format!(
                "URI is not a file:// URL or has no usable path: {uri:?}"
            ))
        })?;

        // 2. 文件不存在 → NotFound(不区分"不在白名单根"vs"路径不存在",
        //    避免暴露 enumeration 信息)。canonicalize 同时帮我们解析符号
        //    链接,防 `file:///<staging>/sym → /etc/passwd` 这种攻击。
        let canonical_path = match tokio::fs::canonicalize(&path).await {
            Ok(p) => p,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                debug!(
                    uri = %uri,
                    path = %path.display(),
                    "mobile_sync staging: read_by_uri path not found"
                );
                return Err(MobileFileStagingError::NotFound);
            }
            Err(err) => {
                return Err(MobileFileStagingError::Io(format!(
                    "canonicalize {} failed: {err}",
                    path.display()
                )));
            }
        };

        // 3. 两根白名单检查(cache_root + home_dir,canonical 形态启动时已
        //    缓存)。任一根命中即放行;都不命中 → NotFound,不暴露具体拒绝
        //    原因(避免 enumeration)。
        if !self.is_path_in_whitelist(&canonical_path) {
            warn!(
                uri = %uri,
                path = %canonical_path.display(),
                cache_root = %self.canonical_cache_root.display(),
                home_root = ?self.canonical_home_root.as_ref().map(|p| p.display().to_string()),
                "mobile_sync staging: read_by_uri rejected path outside whitelisted roots"
            );
            return Err(MobileFileStagingError::NotFound);
        }

        // 4. 读盘。
        let bytes = tokio::fs::read(&canonical_path).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                // 路径在 canonicalize 后又被并发删除 —— 罕见,翻 NotFound。
                MobileFileStagingError::NotFound
            } else {
                MobileFileStagingError::Io(format!(
                    "read {} failed: {err}",
                    canonical_path.display()
                ))
            }
        })?;
        let bytes_len = bytes.len();
        if matches!(bytes_len, 0) {
            debug!(uri = %uri, "mobile_sync staging: read_by_uri served empty file");
        } else {
            debug!(
                uri = %uri,
                path = %canonical_path.display(),
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
    async fn read_by_uri_rejects_path_outside_whitelisted_roots() {
        // 用 new_for_tests 注入受控两根,确保 `/etc/hosts` 既不在 cache_root
        // 也不在我们指定的"假 home"(临时另一目录)。攻击向量:恶意 entry
        // 的 file URI 指向 /etc/hosts —— 合法路径,真实存在,但落在两根
        // 白名单之外。adapter 必须返 NotFound,不暴露字节。
        let cache_tmp = TempDir::new().unwrap();
        let fake_home = TempDir::new().unwrap();
        let adapter = FilesystemMobileFileStaging::new_for_tests(
            cache_tmp.path().to_path_buf(),
            Some(fake_home.path().to_path_buf()),
        );

        let outside_uri = url::Url::from_file_path("/etc/hosts").unwrap().to_string();
        let err = adapter.read_by_uri(&outside_uri).await.unwrap_err();
        assert!(
            matches!(err, MobileFileStagingError::NotFound),
            "expected NotFound for path outside whitelisted roots, got {err:?}"
        );
    }

    #[tokio::test]
    async fn read_by_uri_accepts_path_in_home_root() {
        // P5a.10: Windows 资源管理器复制本地文件 → paste rep URI 是
        // `file:///C:/Users/.../Downloads/foo.pdf` 之类用户家目录之下的真实
        // 路径,不在 cache_root 之下。新白名单必须放行 home_root 之下的文件。
        let cache_tmp = TempDir::new().unwrap();
        let home_tmp = TempDir::new().unwrap();
        let adapter = FilesystemMobileFileStaging::new_for_tests(
            cache_tmp.path().to_path_buf(),
            Some(home_tmp.path().to_path_buf()),
        );

        // 在"假 home"下落一份真文件,模拟用户复制的那个文件
        let target = home_tmp.path().join("Downloads").join("real-doc.pdf");
        tokio::fs::create_dir_all(target.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&target, b"%PDF-1.7 real").await.unwrap();
        let uri = url::Url::from_file_path(&target).unwrap().to_string();

        let bytes = adapter
            .read_by_uri(&uri)
            .await
            .expect("home-root file must read");
        assert_eq!(bytes, b"%PDF-1.7 real");
    }

    #[tokio::test]
    async fn read_by_uri_rejects_home_root_path_when_no_home_configured() {
        // 降级行为:未注入 home_root → 白名单只剩 cache_root → 即便文件
        // 真实存在(在系统真 $HOME 之外的某个 tmp 目录),也应拒绝。
        let cache_tmp = TempDir::new().unwrap();
        let other_tmp = TempDir::new().unwrap();
        let adapter =
            FilesystemMobileFileStaging::new_for_tests(cache_tmp.path().to_path_buf(), None);

        let target = other_tmp.path().join("foo.bin");
        tokio::fs::write(&target, b"x").await.unwrap();
        let uri = url::Url::from_file_path(&target).unwrap().to_string();

        let err = adapter.read_by_uri(&uri).await.unwrap_err();
        assert!(
            matches!(err, MobileFileStagingError::NotFound),
            "expected NotFound when path falls outside cache_root and no home root configured, got {err:?}"
        );
    }

    #[tokio::test]
    async fn new_creates_cache_root_when_missing() {
        // 真机踩坑:首启时 cache_root 还没建过,首笔 read_by_uri 走的不是
        // staging 派生的 URI(系统剪贴板原生路径)→ canonicalize cache_root
        // 失败 → IO 错。new() 必须 best-effort 把 cache_root 建出来。
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
