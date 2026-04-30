//! iroh-blobs backed implementation of [`BlobTransferPort`].
//!
//! Adapter 只处理已经加密好的密文字节:发布到本地 iroh-blobs store、生成
//! ticket、按 ticket 拉取、记录本地保留标签。加解密与明文去重分别由
//! 上层 use case 和 sqlite `BlobReferenceRepositoryPort` 负责。

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use iroh::{Endpoint, EndpointId, Watcher};
use iroh_blobs::{
    api::blobs::{AddPathOptions, ExportMode, ExportOptions, ImportMode},
    api::downloader::{DownloadProgressItem, Downloader},
    store::fs::FsStore,
    ticket::BlobTicket as NativeBlobTicket,
    BlobFormat, Hash, HashAndFormat,
};
use iroh_tickets::Ticket;
use tracing::{debug, info, instrument, warn};

use uc_core::ports::blob::{
    BlobDigest, BlobError, BlobProgressSink, BlobTicket, BlobTransferPort, TagReason,
};

/// Minimum byte advance between two `BlobProgressSink::report` calls.
///
/// 节流阈值——每跨过 256 KB 才再上报一次,避免 iroh-blobs 在大 blob 上每个
/// chunk 触发一次 sink(WS 转发链路上百次/秒会拖累 daemon)。
const PROGRESS_REPORT_BYTES: u64 = 256 * 1024;
/// Minimum wall-clock interval between two `BlobProgressSink::report` calls.
///
/// 即便字节阈值还没达到,长时间没有进度也会让前端的"剩余时间"估算错乱;
/// 200ms 是肉眼能感知到的最小卡顿,所以也强制按时间窗刷新一次。
const PROGRESS_REPORT_INTERVAL: Duration = Duration::from_millis(200);

/// 跨公网 holepunch 收敛时,iroh-blobs 内部 `ConnectionPool` 的 1s `connect_timeout`
/// 经常来不及——即便 `endpoint.connect` 已预热 quinn 层,pool 自己仍按 EndpointId 重新
/// 走 connect_with_alpn,首次 attempt 抓不到热连接就直接 abort。
///
/// 这两个常量定义"在我们这一层包重试":总尝试次数(含首次)和每次失败后的退避。
/// 退避采用阶梯式:200ms 让 endpoint 状态机消化一轮 pong,800ms 给 call-me-maybe
/// 完整往返,2s 兜底大 RTT。最坏情况 ≈ 3s + 4 次 1s 内部 timeout = 7s 才放弃。
const BLOB_FETCH_MAX_ATTEMPTS: u32 = 4;
const BLOB_FETCH_BACKOFFS: [Duration; 3] = [
    Duration::from_millis(200),
    Duration::from_millis(800),
    Duration::from_secs(2),
];

pub const BLOBS_ALPN: &[u8] = iroh_blobs::ALPN;

pub struct IrohBlobTransferAdapter {
    endpoint: Arc<Endpoint>,
    store: FsStore,
    /// Long-lived downloader. `iroh_blobs::Store::downloader(&endpoint)` spawns
    /// a DownloaderActor and its internal `ConnectionPool` on every call — if
    /// we rebuild it per `fetch()`, the pool (idle_timeout=5s, connect_timeout=1s)
    /// can never accumulate a reusable QUIC connection, so every fetch pays the
    /// full hole-punch cost. Cache it once per adapter instance.
    downloader: OnceLock<Downloader>,
}

impl IrohBlobTransferAdapter {
    pub fn new(endpoint: Arc<Endpoint>, store: FsStore) -> Self {
        Self {
            endpoint,
            store,
            downloader: OnceLock::new(),
        }
    }

    /// Lazy-init and cache the iroh-blobs `Downloader`. First call spawns the
    /// DownloaderActor; subsequent calls hand back the same instance so the
    /// internal ConnectionPool can reuse live QUIC connections across fetches.
    fn downloader(&self) -> &Downloader {
        self.downloader
            .get_or_init(|| self.store.downloader(&self.endpoint))
    }

    fn native_hash(digest: &BlobDigest) -> Hash {
        Hash::from_bytes(*digest.as_bytes())
    }

    fn core_digest(hash: Hash) -> BlobDigest {
        BlobDigest::from_bytes(*hash.as_bytes())
    }

    fn parse_ticket(ticket: &BlobTicket) -> Result<NativeBlobTicket, BlobError> {
        NativeBlobTicket::from_bytes(ticket.as_bytes()).map_err(|_| BlobError::InvalidTicket)
    }

    fn tag_name(reason: &TagReason) -> String {
        match reason {
            TagReason::ClipboardEntry(entry_id) => {
                format!("uc-clipboard-entry:{}", entry_id.as_ref())
            }
        }
    }

    /// Snapshot the current connection path to `endpoint_id`,renders as
    /// `direct(...)` / `relay(...)` / `mixed(udp:..., relay:...)` / `none` /
    /// `unknown`。仅用于日志,排查"慢同步走 relay 还是 direct"时一眼可见。
    fn conn_label(&self, endpoint_id: EndpointId) -> String {
        match self.endpoint.conn_type(endpoint_id) {
            Some(mut watcher) => watcher.get().to_string(),
            None => "unknown".to_string(),
        }
    }
}

/// Render the first 10 hex chars of a blob hash for log correlation.
/// Never log full hashes — combined with a tag reason, they can become a
/// weak content identifier.
fn hex_prefix(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(10);
    for b in bytes.iter().take(5) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Pick the `ImportMode` for `Blobs::add_path` based on the host platform.
///
/// See `publish_path` for the full rationale; in short: only Windows benefits
/// from `TryReference` (NTFS has no reflink, ReFS prefers external handle
/// over reflink anyway). All other platforms keep `Copy` so APFS / Btrfs /
/// XFS reflink fast paths fire.
fn preferred_import_mode() -> ImportMode {
    if cfg!(target_os = "windows") {
        ImportMode::TryReference
    } else {
        ImportMode::Copy
    }
}

#[async_trait]
impl BlobTransferPort for IrohBlobTransferAdapter {
    #[instrument(skip_all, fields(bytes = ciphertext.len()))]
    async fn publish(&self, ciphertext: Bytes) -> Result<BlobDigest, BlobError> {
        // GH#487: 大 blob 的 add_bytes 包含 BLAKE3 + BAO outboard 编码 +
        // 写盘,冷启动 / 慢盘场景下整段都可能阻塞十几秒。这里独立计时,让
        // 上游 publish_blob 的 publish_ms 能与本层 add_bytes_ms 对齐核对。
        let bytes = ciphertext.len() as u64;
        let started = Instant::now();
        let tag = self
            .store
            .blobs()
            .add_bytes(ciphertext)
            .await
            .map_err(|e| BlobError::Internal(e.to_string()))?;
        info!(
            bytes,
            add_bytes_ms = started.elapsed().as_millis() as u64,
            blob_hash = %hex_prefix(tag.hash.as_bytes()),
            "iroh blob publish: add_bytes completed"
        );
        Ok(Self::core_digest(tag.hash))
    }

    #[instrument(skip_all, fields(path = %path.display()))]
    async fn publish_path(&self, path: &std::path::Path) -> Result<BlobDigest, BlobError> {
        // GH#487 P1 streaming publish:走 iroh-blobs 的 add_path 入口,增量算
        // BAO outboard,整段过程内存峰值受 iroh chunk size 主导,与文件大小
        // 无关。旧路径(`tokio::fs::read → Bytes → add_bytes`)在 1GB 文件上
        // 需要 ~2GB 临时内存,且阻塞 outbound dispatch 主流程 ~11s。
        //
        // ImportMode 选择(平台条件):
        //   - 非 Windows(macOS APFS / Linux Btrfs / XFS-with-reflink):用
        //     `ImportMode::Copy` —— 内部 `reflink_or_copy_with_progress` 在
        //     CoW FS 上是 zero-copy reflink,免费;ext4 / 其他 fallback 真
        //     拷贝,这部分用户量少,先不优化。
        //   - Windows(NTFS 大头 / ReFS 少数):用 `ImportMode::TryReference`
        //     (iroh-blobs 0.97 `store/fs/import.rs:485-490`)—— 不进 store
        //     数据目录,直接 `OpenOptions::read(true).open(path)` 拿外部句柄
        //     算 outboard,store entry 状态为 External。NTFS 1GB 实测 ~21s
        //     真拷贝直接消失,只剩 read + BAO 的成本。
        //
        // 正确性窗口:TryReference 模式下,如果用户在 dispatch 后、所有对端
        // fetch 完成前**修改 / 移动**源文件 → outboard 与内容失配,接收端
        // BAO 校验失败。Windows 上 store 持有的 `OpenOptions::read` 句柄会
        // 在 share=full-access 下不阻止改动,但天然阻止文件被删除(file in
        // use)—— 覆盖了"不小心拖去回收站"这一最常见误操作。其他场景接受
        // 失败、由用户重新复制(对端报错而 sender 这一侧不感知,fallback
        // 到 Copy 重 import 的反向通知链路超出本次 step 范围)。
        let started = Instant::now();
        let mode = preferred_import_mode();
        let tag_info = self
            .store
            .blobs()
            .add_path_with_opts(AddPathOptions {
                path: path.to_owned(),
                format: BlobFormat::Raw,
                mode,
            })
            .await
            .map_err(|e| BlobError::Internal(e.to_string()))?;
        info!(
            add_path_ms = started.elapsed().as_millis() as u64,
            mode = ?mode,
            blob_hash = %hex_prefix(tag_info.hash.as_bytes()),
            "iroh blob publish: add_path completed (streaming)"
        );
        Ok(Self::core_digest(tag_info.hash))
    }

    #[instrument(skip_all)]
    async fn issue_ticket(&self, digest: &BlobDigest) -> Result<BlobTicket, BlobError> {
        if !self.has(digest).await? {
            return Err(BlobError::NotFound);
        }

        let ticket = NativeBlobTicket::new(
            self.endpoint.addr(),
            Self::native_hash(digest),
            BlobFormat::Raw,
        );
        Ok(BlobTicket::from_bytes(ticket.to_bytes()))
    }

    #[instrument(skip_all)]
    async fn fetch(
        &self,
        ticket: &BlobTicket,
        progress: Option<&dyn BlobProgressSink>,
    ) -> Result<Bytes, BlobError> {
        let native = Self::parse_ticket(ticket)?;
        self.ensure_blob_in_store(&native, progress).await?;
        self.store
            .blobs()
            .get_bytes(native.hash())
            .await
            .map_err(|e| BlobError::Unavailable(e.to_string()))
    }

    #[instrument(skip_all, fields(target = %target_path.display()))]
    async fn fetch_to_path(
        &self,
        ticket: &BlobTicket,
        target_path: &std::path::Path,
        progress: Option<&dyn BlobProgressSink>,
    ) -> Result<BlobDigest, BlobError> {
        // GH#487 receive-side stream-out:走 `ExportMode::TryReference`,而不是
        // `Blobs::export()` 默认的 `ExportMode::Copy`。
        //
        // `Copy` 在无 reflink 的 FS(NTFS / ext4)上 fallback 成 stream copy ——
        // 把 store 里 owned data file 再写一遍到 target_path,800 MB 实测 ~19.5s,
        // 与文件大小线性。
        //
        // `TryReference` 行为(iroh-blobs 0.97 `store/fs.rs:1281-1313`):
        //   - target 与 store_dir **同卷**(典型场景:都装在 AppData / $HOME):
        //     `std::fs::rename(store_owned_data, target)` —— 元数据操作 ~0ms,
        //     与文件大小无关
        //   - **跨卷**(rename 返回 ERR_CROSS):fallback 到
        //     `reflink_or_copy_with_progress`,行为与 `Copy` 等同,不会更差
        //   - 完成后 store entry 状态转为 External(指向 target),不再持有
        //     owned 副本。`has(digest)` 仍报告 complete(由
        //     `fetch_to_path_keeps_blob_observable_after_export` 测试覆盖契约),
        //     `issue_ticket` 仍可签发,tag/untag 仍工作。
        //
        // 副作用:本端 store 不再持有 blob 的本地副本,无法再向其他对端转发
        // 该 blob。但 clipboard 同步是单跳(sender → receiver),没人会再向
        // receiver 拉同一个 blob,这条能力实际无人使用,可接受。
        let native = Self::parse_ticket(ticket)?;
        let digest = Self::core_digest(native.hash());
        let hash_prefix = hex_prefix(native.hash().as_bytes());

        self.ensure_blob_in_store(&native, progress).await?;

        let export_start = Instant::now();
        let bytes_written = self
            .store
            .blobs()
            .export_with_opts(ExportOptions {
                hash: native.hash(),
                mode: ExportMode::TryReference,
                target: target_path.to_owned(),
            })
            .await
            .map_err(|e| BlobError::Internal(e.to_string()))?;
        info!(
            hash = %hash_prefix,
            bytes = bytes_written,
            export_ms = export_start.elapsed().as_millis() as u64,
            "blob fetch_to_path: export completed (TryReference)"
        );
        Ok(digest)
    }

    #[instrument(skip_all)]
    async fn has(&self, digest: &BlobDigest) -> Result<bool, BlobError> {
        let hash = Self::native_hash(digest);
        let observed = self
            .store
            .blobs()
            .observe(hash)
            .await
            .map_err(|e| BlobError::Internal(e.to_string()))?;
        Ok(observed.is_complete())
    }

    #[instrument(skip_all)]
    async fn tag(&self, digest: &BlobDigest, reason: TagReason) -> Result<(), BlobError> {
        let name = Self::tag_name(&reason);
        self.store
            .tags()
            .set(
                name.as_bytes(),
                HashAndFormat::raw(Self::native_hash(digest)),
            )
            .await
            .map_err(|e| BlobError::Internal(e.to_string()))
    }

    #[instrument(skip_all)]
    async fn untag(&self, _digest: &BlobDigest, reason: TagReason) -> Result<(), BlobError> {
        let name = Self::tag_name(&reason);
        let removed = self
            .store
            .tags()
            .delete(name.as_bytes())
            .await
            .map_err(|e| BlobError::Internal(e.to_string()))?;
        debug!(removed, "blob tag removed");
        Ok(())
    }

    fn digest_of(&self, ticket: &BlobTicket) -> Result<BlobDigest, BlobError> {
        let native = Self::parse_ticket(ticket)?;
        Ok(Self::core_digest(native.hash()))
    }
}

impl IrohBlobTransferAdapter {
    /// Make sure the local iroh store holds the blob the ticket points at,
    /// either by serving the existing copy or by running the full
    /// `pre-connect → downloader → retry` loop. Shared between
    /// [`fetch`](BlobTransferPort::fetch) (which then `get_bytes`s the
    /// store back into memory) and
    /// [`fetch_to_path`](BlobTransferPort::fetch_to_path) (which then
    /// `export`s the store entry into a target file). Pulling the loop
    /// up here keeps the two trait methods in lockstep on retries,
    /// progress sink semantics, and connection-pool warm-up.
    async fn ensure_blob_in_store(
        &self,
        native: &NativeBlobTicket,
        progress: Option<&dyn BlobProgressSink>,
    ) -> Result<(), BlobError> {
        let digest = Self::core_digest(native.hash());
        let hash_prefix = hex_prefix(native.hash().as_bytes());
        let provider_id = native.addr().id;

        if self.has(&digest).await? {
            info!(hash = %hash_prefix, "blob fetch: local hit, skipping network");
            return Ok(());
        }

        // Pre-connect to seed the iroh endpoint's address lookup with the
        // ticket's full `EndpointAddr` (relay + direct addrs). The downloader's
        // ConnectionPool only takes `EndpointId`, so without this step it'd
        // have to rediscover addrs via mDNS / pkarr.
        //
        // CRITICAL: We keep `_connection` in scope for the whole fetch. The
        // previous implementation did `drop(connection)` immediately, which
        // let the QUIC connection close before the downloader's ConnectionPool
        // had a chance to reuse it — forcing a second hole-punch on every
        // fetch (see phase notes: observed 33s blob-unavailable failures on
        // cold paths). Holding the connection until the download completes
        // gives the pool a warm reference to grab.
        let connect_start = Instant::now();
        let _connection = self
            .endpoint
            .connect(native.addr().clone(), BLOBS_ALPN)
            .await
            .map_err(|e| {
                warn!(
                    hash = %hash_prefix,
                    elapsed_ms = connect_start.elapsed().as_millis() as u64,
                    conn = %self.conn_label(provider_id),
                    error = %e,
                    "blob fetch: endpoint.connect failed"
                );
                BlobError::Unavailable(e.to_string())
            })?;
        info!(
            hash = %hash_prefix,
            elapsed_ms = connect_start.elapsed().as_millis() as u64,
            conn = %self.conn_label(provider_id),
            "blob fetch: endpoint.connect ready, launching download"
        );

        // Throttle Progress(n) logs: 65MB blobs emit one event per chunk.
        // Log a checkpoint every PROGRESS_LOG_BYTES so Seq can show shape of
        // the transfer (continuous vs stalled) without flooding.
        const PROGRESS_LOG_BYTES: u64 = 4 * 1024 * 1024;

        // Progress sink 状态需要跨 attempt 持有,避免重试时进度条回退。
        let mut last_reported_bytes: u64 = 0;
        let mut last_reported_at: Option<Instant> = None;
        let mut final_bytes: u64 = 0;
        let mut total_tried_providers: u32 = 0;
        let mut last_attempt_ms: u64 = 0;

        let fetch_start = Instant::now();
        for attempt in 1..=BLOB_FETCH_MAX_ATTEMPTS {
            // Subscribe to the Downloader's progress stream instead of `.await`ing
            // the IntoFuture form. iroh-blobs 0.97 `execute_get` (downloader.rs:472)
            // discards the underlying QUIC error with `Err(_cause) => continue`,
            // surfacing only the high-level `bail!("Unable to download {}", hash)`
            // when all providers are exhausted. The progress channel is the only
            // place where we can observe per-provider failures, byte-level
            // progress, and the eventual `Error(anyhow::Error)` whose chain
            // contains the real quinn cause (ConnectionLost, Read(Reset), etc).
            let download_start = Instant::now();
            let mut progress_stream = self
                .downloader()
                .download(native.hash_and_format(), [native.addr().id])
                .stream()
                .await
                .map_err(|e| {
                    warn!(
                        hash = %hash_prefix,
                        elapsed_ms = download_start.elapsed().as_millis() as u64,
                        attempt,
                        conn = %self.conn_label(provider_id),
                        error = %e,
                        "blob fetch: downloader.stream() open failed"
                    );
                    BlobError::Unavailable(e.to_string())
                })?;

            let mut bytes_so_far: u64 = 0;
            let mut last_logged_bytes: u64 = 0;
            let mut provider_failures: u32 = 0;
            let mut tried_providers: u32 = 0;

            let download_result: Result<(), BlobError> = loop {
                let Some(item) = progress_stream.next().await else {
                    break Ok(());
                };
                match item {
                    DownloadProgressItem::TryProvider { id, .. } => {
                        tried_providers += 1;
                        info!(
                            hash = %hash_prefix,
                            provider = %id.fmt_short(),
                            elapsed_ms = download_start.elapsed().as_millis() as u64,
                            attempt,
                            conn = %self.conn_label(provider_id),
                            "blob fetch: trying provider"
                        );
                    }
                    DownloadProgressItem::ProviderFailed { id, .. } => {
                        provider_failures += 1;
                        // execute_get drops the underlying error here — we only
                        // get to know which provider failed and how far we got.
                        warn!(
                            hash = %hash_prefix,
                            provider = %id.fmt_short(),
                            elapsed_ms = download_start.elapsed().as_millis() as u64,
                            bytes_downloaded = bytes_so_far,
                            attempt,
                            conn = %self.conn_label(provider_id),
                            "blob fetch: provider failed (cause discarded by iroh-blobs::execute_get)"
                        );
                    }
                    DownloadProgressItem::Progress(total) => {
                        bytes_so_far = total;
                        if total >= last_logged_bytes + PROGRESS_LOG_BYTES {
                            info!(
                                hash = %hash_prefix,
                                bytes = total,
                                elapsed_ms = download_start.elapsed().as_millis() as u64,
                                attempt,
                                conn = %self.conn_label(provider_id),
                                "blob fetch: progress checkpoint"
                            );
                            last_logged_bytes = total;
                        }
                        if let Some(sink) = progress {
                            let due_by_bytes = total >= last_reported_bytes + PROGRESS_REPORT_BYTES;
                            let due_by_time = last_reported_at
                                .map(|t| t.elapsed() >= PROGRESS_REPORT_INTERVAL)
                                .unwrap_or(true);
                            if (due_by_bytes || due_by_time) && total > last_reported_bytes {
                                sink.report(total, None).await;
                                last_reported_bytes = total;
                                last_reported_at = Some(Instant::now());
                            }
                        }
                    }
                    DownloadProgressItem::PartComplete { .. } => {
                        info!(
                            hash = %hash_prefix,
                            bytes = bytes_so_far,
                            elapsed_ms = download_start.elapsed().as_millis() as u64,
                            attempt,
                            conn = %self.conn_label(provider_id),
                            "blob fetch: part complete"
                        );
                        if let Some(sink) = progress {
                            if bytes_so_far > last_reported_bytes {
                                sink.report(bytes_so_far, Some(bytes_so_far)).await;
                                last_reported_bytes = bytes_so_far;
                                last_reported_at = Some(Instant::now());
                            }
                        }
                    }
                    DownloadProgressItem::DownloadError => {
                        warn!(
                            hash = %hash_prefix,
                            elapsed_ms = download_start.elapsed().as_millis() as u64,
                            bytes_downloaded = bytes_so_far,
                            provider_failures,
                            tried_providers,
                            attempt,
                            conn = %self.conn_label(provider_id),
                            "blob fetch: DownloadError signalled (split-strategy aggregate failure)"
                        );
                        break Err(BlobError::Unavailable("Download error".into()));
                    }
                    DownloadProgressItem::Error(e) => {
                        // The single most useful event: the anyhow chain here
                        // typically wraps the quinn::ConnectionError or
                        // ReadError that `execute_get` swallowed earlier.
                        warn!(
                            hash = %hash_prefix,
                            elapsed_ms = download_start.elapsed().as_millis() as u64,
                            bytes_downloaded = bytes_so_far,
                            provider_failures,
                            tried_providers,
                            attempt,
                            conn = %self.conn_label(provider_id),
                            error = ?e,
                            "blob fetch: downloader Error event (root cause from anyhow chain)"
                        );
                        break Err(BlobError::Unavailable(e.to_string()));
                    }
                }
            };

            last_attempt_ms = download_start.elapsed().as_millis() as u64;
            total_tried_providers = total_tried_providers.saturating_add(tried_providers);

            match download_result {
                Ok(()) => {
                    final_bytes = bytes_so_far;
                    break;
                }
                Err(BlobError::Unavailable(msg)) if attempt < BLOB_FETCH_MAX_ATTEMPTS => {
                    let backoff = BLOB_FETCH_BACKOFFS[(attempt - 1) as usize];
                    warn!(
                        hash = %hash_prefix,
                        attempt,
                        max_attempts = BLOB_FETCH_MAX_ATTEMPTS,
                        backoff_ms = backoff.as_millis() as u64,
                        cause = %msg,
                        "blob fetch: retrying after Unavailable"
                    );
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        info!(
            hash = %hash_prefix,
            bytes = final_bytes,
            last_attempt_ms,
            download_ms = fetch_start.elapsed().as_millis() as u64,
            connect_ms = (connect_start.elapsed() - fetch_start.elapsed()).as_millis() as u64,
            tried_providers = total_tried_providers,
            conn = %self.conn_label(provider_id),
            "blob fetch: download complete"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::time::Duration;

    use iroh::{protocol::Router, RelayMode};
    use tempfile::{tempdir, TempDir};
    use uc_core::ids::EntryId;

    struct Fixture {
        adapter: IrohBlobTransferAdapter,
        router: Router,
        store: FsStore,
        _tempdir: TempDir,
    }

    impl Fixture {
        async fn bind() -> anyhow::Result<Self> {
            let tempdir = tempdir()?;
            let store = FsStore::load(store_path(&tempdir)).await?;
            let endpoint = Endpoint::builder()
                .relay_mode(RelayMode::Disabled)
                .bind()
                .await?;
            let protocol = iroh_blobs::BlobsProtocol::new(&store, None);
            let router = Router::builder(endpoint.clone())
                .accept(BLOBS_ALPN, protocol)
                .spawn();
            let endpoint = Arc::new(endpoint);
            let adapter = IrohBlobTransferAdapter::new(endpoint, store.clone());

            Ok(Self {
                adapter,
                router,
                store,
                _tempdir: tempdir,
            })
        }

        async fn wait_for_direct_addr(&self) -> anyhow::Result<()> {
            for _ in 0..100 {
                if self.router.endpoint().addr().ip_addrs().next().is_some() {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            anyhow::bail!("iroh endpoint never published a loopback direct address")
        }

        async fn shutdown(self) -> anyhow::Result<()> {
            self.router.shutdown().await?;
            Ok(())
        }
    }

    fn store_path(tempdir: &TempDir) -> PathBuf {
        tempdir.path().join("iroh-blobs")
    }

    fn unknown_digest() -> BlobDigest {
        BlobDigest::from_bytes([0x7f; 32])
    }

    #[tokio::test]
    async fn publish_same_bytes_returns_stable_digest() -> anyhow::Result<()> {
        let fixture = Fixture::bind().await?;
        let payload = Bytes::from_static(b"slice3-t4-stable");

        let first = fixture.adapter.publish(payload.clone()).await?;
        let second = fixture.adapter.publish(payload).await?;

        assert_eq!(first, second);
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_to_path_writes_local_hit_to_target() -> anyhow::Result<()> {
        // GH#487 Phase 2: when the blob is already in the local store
        // (e.g. publisher fetching its own ticket), fetch_to_path must
        // export it directly to the target path without going through
        // the network or materialising the bytes in memory.
        let fixture = Fixture::bind().await?;
        fixture.wait_for_direct_addr().await?;
        let payload = b"gh-487-fetch-to-path-local-hit".to_vec();
        let digest = fixture
            .adapter
            .publish(Bytes::from(payload.clone()))
            .await?;
        let ticket = fixture.adapter.issue_ticket(&digest).await?;

        let dir = tempdir()?;
        let target = dir.path().join("out.bin");
        let returned = fixture
            .adapter
            .fetch_to_path(&ticket, &target, None)
            .await?;

        assert_eq!(returned, digest);
        let written = std::fs::read(&target)?;
        assert_eq!(written, payload);
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_to_path_try_reference_writes_correct_bytes_above_inline_limit(
    ) -> anyhow::Result<()> {
        // GH#487 receive-side TryReference: payload must exceed iroh-blobs'
        // default 16 KiB inline threshold so the store actually holds an
        // owned data file (the only case TryReference's `fs::rename` branch
        // can fire). With a small payload the store keeps the bytes inline
        // and `export` falls through to a write-from-memory path, which
        // would mask any rename-vs-copy regression.
        let fixture = Fixture::bind().await?;
        fixture.wait_for_direct_addr().await?;
        let payload = vec![0x5au8; 64 * 1024];
        let digest = fixture
            .adapter
            .publish(Bytes::from(payload.clone()))
            .await?;
        let ticket = fixture.adapter.issue_ticket(&digest).await?;

        let dir = tempdir()?;
        let target = dir.path().join("big.bin");
        let returned = fixture
            .adapter
            .fetch_to_path(&ticket, &target, None)
            .await?;

        assert_eq!(returned, digest);
        let written = std::fs::read(&target)?;
        assert_eq!(written.len(), payload.len());
        assert_eq!(written, payload);
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_to_path_keeps_blob_observable_after_export() -> anyhow::Result<()> {
        // GH#487 receive-side TryReference contract regression: after export
        // the iroh store transitions the entry to External(target_path) and
        // drops its owned data file. `has(digest)` must still report
        // complete and `issue_ticket` must still succeed — otherwise tag
        // bookkeeping (TagReason::ClipboardEntry) and any "show me the
        // blobs we have" diagnostics would silently break.
        let fixture = Fixture::bind().await?;
        fixture.wait_for_direct_addr().await?;
        let payload = vec![0xa5u8; 64 * 1024];
        let digest = fixture
            .adapter
            .publish(Bytes::from(payload.clone()))
            .await?;
        let ticket = fixture.adapter.issue_ticket(&digest).await?;

        let dir = tempdir()?;
        let target = dir.path().join("exported.bin");
        fixture
            .adapter
            .fetch_to_path(&ticket, &target, None)
            .await?;

        assert!(
            fixture.adapter.has(&digest).await?,
            "has() must still report complete for an externally referenced entry"
        );
        let _reissued = fixture
            .adapter
            .issue_ticket(&digest)
            .await
            .expect("issue_ticket must succeed for an externally referenced entry");

        // Tag/untag must still work — clipboard_sync uses these to pin
        // blobs to a ClipboardEntry and clean up on entry deletion.
        let reason = TagReason::ClipboardEntry(EntryId::from_str("entry-after-export"));
        fixture.adapter.tag(&digest, reason.clone()).await?;
        fixture.adapter.untag(&digest, reason).await?;

        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn publish_path_returns_same_digest_as_publish_bytes() -> anyhow::Result<()> {
        // GH#487 P1: streaming path 必须与全内存 publish 产出一致的
        // content-addressed digest,否则发送端 / 接收端协议上的 ticket /
        // dedup 会全部断裂。
        let fixture = Fixture::bind().await?;
        let payload = b"gh-487-streaming-publish-path-payload-hello".to_vec();

        let dir = tempdir()?;
        let path = dir.path().join("payload.bin");
        std::fs::write(&path, &payload)?;

        let bytes_digest = fixture
            .adapter
            .publish(Bytes::from(payload.clone()))
            .await?;
        let path_digest = fixture.adapter.publish_path(&path).await?;

        assert_eq!(bytes_digest, path_digest);
        fixture.shutdown().await?;
        Ok(())
    }

    #[test]
    fn preferred_import_mode_is_try_reference_on_windows_copy_elsewhere() {
        // GH#487 Step 2 platform contract: TryReference is the default on
        // Windows (NTFS / ReFS) to skip the ~21s NTFS stream-copy fallback;
        // everywhere else stays on Copy so the existing reflink fast paths
        // (APFS / Btrfs / XFS reflink) keep firing untouched.
        let mode = preferred_import_mode();
        if cfg!(target_os = "windows") {
            assert!(matches!(mode, ImportMode::TryReference));
        } else {
            assert!(matches!(mode, ImportMode::Copy));
        }
    }

    #[tokio::test]
    async fn publish_path_try_reference_yields_same_digest_as_copy() -> anyhow::Result<()> {
        // GH#487 Step 2: switching ImportMode must not change the
        // content-addressed digest. The store's own dedup invariant
        // promises this (BAO is computed off the file content, not the
        // import strategy), but a regression here would silently break
        // every ticket the sender mints once the platform-conditional
        // mode select kicks in. Payload is 64 KiB — above iroh-blobs'
        // default 16 KiB inline threshold — so both branches actually
        // hit the file-import path that differs between modes.
        let fixture = Fixture::bind().await?;
        let payload = vec![0xc3u8; 64 * 1024];

        let dir = tempdir()?;
        let path_copy = dir.path().join("copy.bin");
        let path_ref = dir.path().join("ref.bin");
        std::fs::write(&path_copy, &payload)?;
        std::fs::write(&path_ref, &payload)?;

        let copy_tag = fixture
            .store
            .blobs()
            .add_path_with_opts(AddPathOptions {
                path: path_copy.clone(),
                format: BlobFormat::Raw,
                mode: ImportMode::Copy,
            })
            .await?;
        let ref_tag = fixture
            .store
            .blobs()
            .add_path_with_opts(AddPathOptions {
                path: path_ref.clone(),
                format: BlobFormat::Raw,
                mode: ImportMode::TryReference,
            })
            .await?;

        assert_eq!(copy_tag.hash, ref_tag.hash);
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn publish_path_try_reference_serves_correct_bytes_to_local_fetch() -> anyhow::Result<()>
    {
        // GH#487 Step 2 end-to-end contract: when the store entry is an
        // External(path) reference rather than an owned data file, fetching
        // the blob (here through the local-hit fast path) must still hand
        // back the exact original bytes. This is what catches "we kept a
        // reference but the BAO outboard ended up wrong" regressions.
        let fixture = Fixture::bind().await?;
        fixture.wait_for_direct_addr().await?;
        let payload = vec![0x91u8; 64 * 1024];

        let dir = tempdir()?;
        let source = dir.path().join("source.bin");
        std::fs::write(&source, &payload)?;

        let tag_info = fixture
            .store
            .blobs()
            .add_path_with_opts(AddPathOptions {
                path: source.clone(),
                format: BlobFormat::Raw,
                mode: ImportMode::TryReference,
            })
            .await?;
        let digest = IrohBlobTransferAdapter::core_digest(tag_info.hash);
        let ticket = fixture.adapter.issue_ticket(&digest).await?;

        let target = dir.path().join("out.bin");
        let returned = fixture
            .adapter
            .fetch_to_path(&ticket, &target, None)
            .await?;

        assert_eq!(returned, digest);
        let fetched = std::fs::read(&target)?;
        assert_eq!(fetched, payload);
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn has_reports_present_and_missing_blobs() -> anyhow::Result<()> {
        let fixture = Fixture::bind().await?;

        let digest = fixture
            .adapter
            .publish(Bytes::from_static(b"slice3-t4-has"))
            .await?;

        assert!(fixture.adapter.has(&digest).await?);
        assert!(!fixture.adapter.has(&unknown_digest()).await?);
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn issue_ticket_and_digest_of_round_trip() -> anyhow::Result<()> {
        let fixture = Fixture::bind().await?;
        fixture.wait_for_direct_addr().await?;
        let digest = fixture
            .adapter
            .publish(Bytes::from_static(b"slice3-t4-ticket"))
            .await?;

        let ticket = fixture.adapter.issue_ticket(&digest).await?;

        assert_eq!(fixture.adapter.digest_of(&ticket)?, digest);
        assert_eq!(
            BlobTicket::from_bytes(ticket.as_bytes().to_vec()).as_bytes(),
            ticket.as_bytes()
        );
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn digest_of_invalid_ticket_returns_invalid_ticket() -> anyhow::Result<()> {
        let fixture = Fixture::bind().await?;
        let ticket = BlobTicket::from_bytes(vec![1, 2, 3, 4, 5]);

        let err = fixture
            .adapter
            .digest_of(&ticket)
            .expect_err("corrupt ticket must fail");

        assert!(matches!(err, BlobError::InvalidTicket));
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn issue_ticket_for_missing_digest_returns_not_found() -> anyhow::Result<()> {
        let fixture = Fixture::bind().await?;

        let err = fixture
            .adapter
            .issue_ticket(&unknown_digest())
            .await
            .expect_err("missing digest must not mint a ticket");

        assert!(matches!(err, BlobError::NotFound));
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_self_ticket_returns_original_bytes() -> anyhow::Result<()> {
        let fixture = Fixture::bind().await?;
        fixture.wait_for_direct_addr().await?;
        let payload = Bytes::from_static(b"slice3-t5-self-fetch");
        let digest = fixture.adapter.publish(payload.clone()).await?;
        let ticket = fixture.adapter.issue_ticket(&digest).await?;

        let fetched = fixture.adapter.fetch(&ticket, None).await?;

        assert_eq!(fetched, payload);
        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn fetch_remote_ticket_returns_original_bytes() -> anyhow::Result<()> {
        let provider = Fixture::bind().await?;
        let receiver = Fixture::bind().await?;
        provider.wait_for_direct_addr().await?;
        receiver.wait_for_direct_addr().await?;
        let payload = Bytes::from_static(b"slice3-t5-remote-fetch");
        let digest = provider.adapter.publish(payload.clone()).await?;
        let ticket = provider.adapter.issue_ticket(&digest).await?;

        let fetched = receiver.adapter.fetch(&ticket, None).await?;

        assert_eq!(fetched, payload);
        receiver.shutdown().await?;
        provider.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn tag_then_untag_is_idempotent() -> anyhow::Result<()> {
        let fixture = Fixture::bind().await?;
        let digest = fixture
            .adapter
            .publish(Bytes::from_static(b"slice3-t6-tag"))
            .await?;
        let reason = TagReason::ClipboardEntry(EntryId::from_str("entry-a"));

        fixture.adapter.tag(&digest, reason.clone()).await?;
        fixture.adapter.untag(&digest, reason.clone()).await?;
        fixture.adapter.untag(&digest, reason).await?;

        fixture.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn multiple_tag_reasons_are_independent() -> anyhow::Result<()> {
        let fixture = Fixture::bind().await?;
        let digest = fixture
            .adapter
            .publish(Bytes::from_static(b"slice3-t6-multi-tag"))
            .await?;
        let first = TagReason::ClipboardEntry(EntryId::from_str("entry-a"));
        let second = TagReason::ClipboardEntry(EntryId::from_str("entry-b"));

        fixture.adapter.tag(&digest, first.clone()).await?;
        fixture.adapter.tag(&digest, second.clone()).await?;
        fixture.adapter.untag(&digest, first.clone()).await?;

        let second_tag = IrohBlobTransferAdapter::tag_name(&second);
        assert!(fixture
            .store
            .tags()
            .get(second_tag.as_bytes())
            .await?
            .is_some());

        fixture.adapter.untag(&digest, second).await?;
        fixture.shutdown().await?;
        Ok(())
    }
}
