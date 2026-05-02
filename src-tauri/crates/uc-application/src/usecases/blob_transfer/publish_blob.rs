use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use tracing::info;

use uc_core::ids::EntryId;
use uc_core::ports::blob::{
    BlobDigest, BlobReferenceRepositoryPort, BlobTicket, BlobTransferPort, PlaintextHash, TagReason,
};
use uc_core::ports::ContentHashPort;

/// publish_blob 的输入 —— 内存 plaintext 与磁盘文件路径两条入口的语义对称。
///
/// `Plaintext` 路径用于已经在内存里的 inline payload(小图、文本扩展 rep);
/// `Path` 路径用于磁盘文件 outbound,走流式入库避免把整文件加载到内存
/// (GH#487)。两条路径产出的 `PublishBlobOutcome` 在协议层等价。
#[derive(Debug, Clone)]
pub(crate) enum PublishBlobInput {
    Plaintext { plaintext: Bytes, entry_id: EntryId },
    Path { path: PathBuf, entry_id: EntryId },
}

#[derive(Debug, Clone)]
pub(crate) struct PublishBlobOutcome {
    pub ticket: BlobTicket,
    pub entry_id: EntryId,
    pub plaintext_hash: PlaintextHash,
    pub digest: BlobDigest,
    pub reused_existing: bool,
}

pub(crate) struct PublishBlobUseCase {
    hash: Arc<dyn ContentHashPort>,
    blob_transfer: Arc<dyn BlobTransferPort>,
    blob_reference: Arc<dyn BlobReferenceRepositoryPort>,
}

impl PublishBlobUseCase {
    pub fn new(
        hash: Arc<dyn ContentHashPort>,
        blob_transfer: Arc<dyn BlobTransferPort>,
        blob_reference: Arc<dyn BlobReferenceRepositoryPort>,
    ) -> Self {
        Self {
            hash,
            blob_transfer,
            blob_reference,
        }
    }

    pub async fn execute(
        &self,
        input: PublishBlobInput,
    ) -> Result<PublishBlobOutcome, PublishBlobError> {
        match input {
            PublishBlobInput::Plaintext {
                plaintext,
                entry_id,
            } => self.execute_plaintext(plaintext, entry_id).await,
            PublishBlobInput::Path { path, entry_id } => self.execute_path(path, entry_id).await,
        }
    }

    async fn execute_plaintext(
        &self,
        plaintext: Bytes,
        entry_id: EntryId,
    ) -> Result<PublishBlobOutcome, PublishBlobError> {
        if plaintext.is_empty() {
            return Err(PublishBlobError::EmptyPlaintext);
        }

        // Phase timing for outbound blob publish.
        // hash 与 add_bytes 都会对 plaintext 做一次 BLAKE3,大文件场景下两次
        // 加起来不可忽略;tag/ticket/save_ref 涉及 store + sqlite 写入,冷启动
        // 时也可能慢。GH#487 诊断需要这些阶段拆分。
        let bytes = plaintext.len() as u64;

        let hash_start = Instant::now();
        let plaintext_hash = PlaintextHash::from_bytes(
            self.hash
                .hash_bytes(&plaintext)
                .map_err(|e| PublishBlobError::Hash(e.to_string()))?
                .bytes,
        );
        let hash_ms = hash_start.elapsed().as_millis() as u64;

        let lookup_start = Instant::now();
        if let Some(digest) = self.find_reusable_digest(&plaintext_hash).await? {
            let lookup_ms = lookup_start.elapsed().as_millis() as u64;

            let tag_start = Instant::now();
            self.blob_transfer
                .tag(&digest, TagReason::ClipboardEntry(entry_id.clone()))
                .await
                .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
            let tag_ms = tag_start.elapsed().as_millis() as u64;

            let ticket_start = Instant::now();
            let ticket = self
                .blob_transfer
                .issue_ticket(&digest)
                .await
                .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
            let ticket_ms = ticket_start.elapsed().as_millis() as u64;

            info!(
                entry_id = %entry_id.as_str(),
                bytes,
                reused_existing = true,
                hash_ms,
                lookup_ms,
                tag_ms,
                ticket_ms,
                "publish_blob: reused existing digest"
            );

            return Ok(PublishBlobOutcome {
                ticket,
                entry_id,
                plaintext_hash,
                digest,
                reused_existing: true,
            });
        }
        let lookup_ms = lookup_start.elapsed().as_millis() as u64;

        // File blobs go through iroh-blobs as raw bytes — content-addressed by
        // blake3 of the plaintext, which equals `plaintext_hash`. Application-
        // layer encryption is intentionally absent: file payloads are opaque
        // user-chosen content (the user already consented by copying), and any
        // sensitive *metadata* (filenames, paths, mime, thumbnails) lives on
        // the clipboard event side and is encrypted there by
        // `EncryptingClipboardEventWriter`.
        //
        // Phase F: publish 携带业务 reason 一起原子入库。adapter 内部走
        // `with_named_tag`,完成后 blob 已经直接挂在 ClipboardEntry tag 上,
        // 不再产生 iroh-blobs 自动的 `auto-<ts>` 孤儿 tag。我们因此**也不再
        // 单独调一次 `tag()`** —— 之前那次 tag() 是为了在 auto-tag 已经存在
        // 的情况下额外打一层业务声明,Phase F 后语义等同于"重新 set 同一个
        // 已存在的 tag",冗余。
        let publish_start = Instant::now();
        let digest = self
            .blob_transfer
            .publish(plaintext, TagReason::ClipboardEntry(entry_id.clone()))
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        let publish_ms = publish_start.elapsed().as_millis() as u64;

        let save_ref_start = Instant::now();
        self.blob_reference
            .save(plaintext_hash, digest)
            .await
            .map_err(|e| PublishBlobError::Reference(e.to_string()))?;
        let save_ref_ms = save_ref_start.elapsed().as_millis() as u64;

        let ticket_start = Instant::now();
        let ticket = self
            .blob_transfer
            .issue_ticket(&digest)
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        let ticket_ms = ticket_start.elapsed().as_millis() as u64;

        info!(
            entry_id = %entry_id.as_str(),
            bytes,
            reused_existing = false,
            hash_ms,
            lookup_ms,
            publish_ms,
            save_ref_ms,
            ticket_ms,
            "publish_blob: new blob added"
        );

        Ok(PublishBlobOutcome {
            ticket,
            entry_id,
            plaintext_hash,
            digest,
            reused_existing: false,
        })
    }

    /// Streaming publish from a local file path. GH#487 P1.
    ///
    /// 跟 `execute_plaintext` 不同,这里**不做 pre-publish dedup**:plaintext
    /// 不在内存里,算 blake3 之前必须先把文件读完——既然要读完,不如交给
    /// iroh-blobs 的 `add_path` 一次过算 BAO 树。iroh store 自身基于 hash
    /// 内容寻址,重复 import 同一文件不会真的占双份盘(只浪费一次 BAO CPU
    /// 与临时拷贝 IO),与"先全文件读到内存 + Bytes 拷贝 + add_bytes"的旧
    /// 路径相比,对 1GB 文件能把 RSS 峰值从 ~2GB 降到与 chunk 相关的常数。
    ///
    /// 文件 blob 不加密(同 `execute_plaintext` 注释),所以 `plaintext_hash`
    /// 与 iroh blob digest 在数值上相等。
    async fn execute_path(
        &self,
        path: PathBuf,
        entry_id: EntryId,
    ) -> Result<PublishBlobOutcome, PublishBlobError> {
        // Phase F: streaming publish 也走原子 tag 路径,理由与
        // `execute_plaintext` 一致 —— 见上面 publish 注释。
        let publish_start = Instant::now();
        let digest = self
            .blob_transfer
            .publish_path(&path, TagReason::ClipboardEntry(entry_id.clone()))
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        let publish_ms = publish_start.elapsed().as_millis() as u64;

        // 文件 blob 不加密 → plaintext_hash == iroh blob hash == digest。
        let plaintext_hash = PlaintextHash::from_bytes(*digest.as_bytes());

        // upsert(BlobReferenceRepositoryPort::save 注释保证 overwrite 安全)。
        let save_ref_start = Instant::now();
        self.blob_reference
            .save(plaintext_hash, digest)
            .await
            .map_err(|e| PublishBlobError::Reference(e.to_string()))?;
        let save_ref_ms = save_ref_start.elapsed().as_millis() as u64;

        let ticket_start = Instant::now();
        let ticket = self
            .blob_transfer
            .issue_ticket(&digest)
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        let ticket_ms = ticket_start.elapsed().as_millis() as u64;

        info!(
            entry_id = %entry_id.as_str(),
            path = %path.display(),
            publish_ms,
            save_ref_ms,
            ticket_ms,
            "publish_blob: streamed from path"
        );

        Ok(PublishBlobOutcome {
            ticket,
            entry_id,
            plaintext_hash,
            digest,
            reused_existing: false,
        })
    }

    async fn find_reusable_digest(
        &self,
        plaintext_hash: &PlaintextHash,
    ) -> Result<Option<BlobDigest>, PublishBlobError> {
        let Some(digest) = self
            .blob_reference
            .find_by_plaintext_hash(plaintext_hash)
            .await
            .map_err(|e| PublishBlobError::Reference(e.to_string()))?
        else {
            return Ok(None);
        };

        let exists = self
            .blob_transfer
            .has(&digest)
            .await
            .map_err(|e| PublishBlobError::Transfer(e.to_string()))?;
        Ok(exists.then_some(digest))
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PublishBlobError {
    #[error("blob plaintext is empty")]
    EmptyPlaintext,
    #[error("hash failed: {0}")]
    Hash(String),
    #[error("blob transfer failed: {0}")]
    Transfer(String),
    #[error("blob reference failed: {0}")]
    Reference(String),
}
