//! Blob publish and fetch use cases.
//!
//! In-memory non-file content is encrypted before entering the persistent
//! transfer store and decrypted after fetch. File payloads use the explicit
//! raw-byte exception; their names, paths, and other metadata remain encrypted
//! in the clipboard event.
//!
//! External callers enter through the facade.

mod fetch_blob;
mod publish_blob;

pub(crate) use fetch_blob::{FetchBlobInput, FetchBlobPathInput, FetchBlobUseCase};
pub(crate) use publish_blob::{PublishBlobInput, PublishBlobUseCase};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use bytes::Bytes;
    use uc_core::ids::EntryId;
    use uc_core::ports::blob::{
        BlobDigest, BlobError, BlobProgressSink, BlobReferenceError, BlobReferenceRepositoryPort,
        BlobTicket, BlobTransferPort, PlaintextHash, TagReason,
    };
    use uc_core::ports::security::{TransferCipherError, TransferCipherPort};
    use uc_core::ports::ContentHashPort;
    use uc_core::{ContentHash, HashAlgorithm};

    use super::{FetchBlobInput, FetchBlobUseCase, PublishBlobInput, PublishBlobUseCase};

    #[derive(Clone, Default)]
    struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

    impl CapturedWriter {
        fn dump(&self) -> String {
            String::from_utf8(self.0.lock().expect("lock captured logs").clone())
                .expect("captured logs should be UTF-8")
        }
    }

    impl Write for CapturedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("lock captured logs")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedWriter {
        type Writer = CapturedWriter;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[tokio::test]
    async fn publish_plaintext_does_not_store_plaintext_transport_bytes() {
        let hash = Arc::new(FakeHash);
        let transfer = Arc::new(FakeBlobTransfer::default());
        let reference = Arc::new(FakeBlobReferenceRepository::default());
        let usecase =
            PublishBlobUseCase::new(hash, transfer.clone(), reference, Arc::new(FakeCipher));

        let plaintext = Bytes::from_static(b"private image bytes");
        let outcome = usecase
            .execute(PublishBlobInput::Plaintext {
                plaintext: plaintext.clone(),
                entry_id: EntryId::from("entry-image"),
            })
            .await
            .expect("publish should succeed");

        assert_ne!(
            transfer.stored_bytes(&outcome.digest),
            Some(plaintext),
            "non-file payload reached the transfer store as plaintext"
        );
    }

    #[tokio::test]
    async fn publish_plaintext_does_not_reuse_raw_file_digest() {
        let hash = Arc::new(FakeHash);
        let transfer = Arc::new(FakeBlobTransfer::default());
        let reference = Arc::new(FakeBlobReferenceRepository::default());
        let usecase = PublishBlobUseCase::new(
            hash.clone(),
            transfer.clone(),
            reference.clone(),
            Arc::new(FakeCipher),
        );

        let plaintext = Bytes::from_static(b"same bytes in a file and image");
        let raw_digest = transfer
            .publish(
                plaintext.clone(),
                TagReason::ClipboardEntry(EntryId::from("entry-file")),
            )
            .await
            .expect("raw file fixture should publish");
        let plaintext_hash =
            PlaintextHash::from_bytes(hash.hash_bytes(&plaintext).expect("fixture hash").bytes);
        reference
            .save(plaintext_hash, raw_digest)
            .await
            .expect("fixture reference should save");

        let outcome = usecase
            .execute(PublishBlobInput::Plaintext {
                plaintext,
                entry_id: EntryId::from("entry-image"),
            })
            .await
            .expect("image publish should succeed");

        assert_ne!(outcome.digest, raw_digest);
        assert!(!outcome.reused_existing);
        assert_eq!(transfer.publish_count(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_path_logs_do_not_include_source_path() {
        let hash = Arc::new(FakeHash);
        let transfer = Arc::new(FakeBlobTransfer::default());
        let reference = Arc::new(FakeBlobReferenceRepository::default());
        let usecase = PublishBlobUseCase::new(hash, transfer, reference, Arc::new(FakeCipher));
        let dir = tempfile::tempdir().expect("tempdir");
        let secret_name = "customer-payroll-private.txt";
        let path = dir.path().join(secret_name);
        std::fs::write(&path, b"file payload").expect("write fixture");

        let writer = CapturedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer.clone())
            .with_ansi(false)
            .finish();
        let dispatch = tracing::Dispatch::new(subscriber);
        let _guard = tracing::dispatcher::set_default(&dispatch);

        usecase
            .execute(PublishBlobInput::Path {
                path,
                entry_id: EntryId::from("entry-file-log-redaction"),
            })
            .await
            .expect("path publish should succeed");

        let logs = writer.dump();
        assert!(
            !logs.contains(secret_name),
            "source filename leaked into publish logs: {logs}"
        );
    }

    #[tokio::test]
    async fn publish_plaintext_encrypts_each_publication_without_reference_reuse() {
        let hash = Arc::new(FakeHash);
        let transfer = Arc::new(FakeBlobTransfer::default());
        let reference = Arc::new(FakeBlobReferenceRepository::default());
        let usecase = PublishBlobUseCase::new(
            hash,
            transfer.clone(),
            reference.clone(),
            Arc::new(FakeCipher),
        );

        let plaintext = Bytes::from_static(b"same file bytes");
        let mut first_digest = None;
        for _ in 0..10 {
            let outcome = usecase
                .execute(PublishBlobInput::Plaintext {
                    plaintext: plaintext.clone(),
                    entry_id: EntryId::new(),
                })
                .await
                .expect("publish should succeed");
            if let Some(first_digest) = first_digest {
                assert_eq!(outcome.digest, first_digest);
                assert!(!outcome.reused_existing);
            } else {
                first_digest = Some(outcome.digest);
                assert!(!outcome.reused_existing);
            }
        }

        assert_eq!(transfer.publish_count(), 10);
        assert_eq!(transfer.tag_count(), 10);
        assert_eq!(reference.save_count(), 10);
    }

    #[tokio::test]
    async fn fetch_second_plaintext_publication_returns_plaintext() {
        let hash = Arc::new(FakeHash);
        let transfer = Arc::new(FakeBlobTransfer::default());
        let reference = Arc::new(FakeBlobReferenceRepository::default());
        let cipher = Arc::new(FakeCipher);
        let publish = PublishBlobUseCase::new(
            hash.clone(),
            transfer.clone(),
            reference.clone(),
            cipher.clone(),
        );
        let fetch = FetchBlobUseCase::new(hash, transfer, reference, cipher);

        let plaintext = Bytes::from_static(b"same file bytes");
        let first = publish
            .execute(PublishBlobInput::Plaintext {
                plaintext: plaintext.clone(),
                entry_id: EntryId::from("entry-one"),
            })
            .await
            .expect("first publish should succeed");
        let second = publish
            .execute(PublishBlobInput::Plaintext {
                plaintext: plaintext.clone(),
                entry_id: EntryId::from("entry-two"),
            })
            .await
            .expect("second publish should succeed");

        assert_eq!(first.digest, second.digest);
        assert!(!second.reused_existing);

        let outcome = fetch
            .execute(FetchBlobInput {
                ticket: second.ticket,
                entry_id: EntryId::from("entry-two"),
                progress: None,
            })
            .await
            .expect("second digest should fetch for the new entry tag");

        assert_eq!(outcome.plaintext, plaintext);
        assert_eq!(outcome.entry_id, EntryId::from("entry-two"));
    }

    #[tokio::test]
    async fn fetch_saves_reference_and_tags_entry() {
        let hash = Arc::new(FakeHash);
        let transfer = Arc::new(FakeBlobTransfer::default());
        let reference = Arc::new(FakeBlobReferenceRepository::default());
        let cipher = Arc::new(FakeCipher);
        let usecase = FetchBlobUseCase::new(
            hash.clone(),
            transfer.clone(),
            reference.clone(),
            cipher.clone(),
        );

        let entry_id = EntryId::new();
        let plaintext = Bytes::from_static(b"remote blob bytes");
        // Phase F: publish 已携带原子 tag,seed 这里随意挂一个非冲突的 reason
        // 即可 —— 真正被测试的是 use case 走 fetch 流程后再补一份业务 tag。
        let encrypted = cipher
            .encrypt(&plaintext)
            .await
            .expect("fixture encryption should succeed");
        let digest = transfer
            .publish(
                Bytes::from(encrypted),
                TagReason::ClipboardEntry(EntryId::from("seed-fixture")),
            )
            .await
            .expect("seed publish should succeed");
        let ticket = transfer
            .issue_ticket(&digest)
            .await
            .expect("seed ticket should succeed");

        let outcome = usecase
            .execute(FetchBlobInput {
                ticket,
                entry_id: entry_id.clone(),
                progress: None,
            })
            .await
            .expect("fetch should succeed");

        assert_eq!(outcome.plaintext, plaintext);
        assert_eq!(outcome.entry_id, entry_id);
        assert_eq!(outcome.digest, digest);
        assert_eq!(
            reference
                .find_by_plaintext_hash(&outcome.plaintext_hash)
                .await
                .expect("reference lookup should succeed"),
            Some(digest)
        );
        // Phase F 之后 publish 自身也算一次 tag(seed publish 时的
        // `seed-fixture` reason),fetch use case 完成后再补一次 entry_id
        // 的 tag,所以 fixture 里总共记录两条。这个断言锁定 fetch 路径
        // 一定**有**业务 tag 调用,与 publish-时的 seed tag 区分开。
        assert_eq!(transfer.tag_count(), 2);
    }

    #[derive(Default)]
    struct FakeHash;

    impl ContentHashPort for FakeHash {
        fn hash_bytes(&self, bytes: &[u8]) -> anyhow::Result<ContentHash> {
            Ok(ContentHash {
                alg: HashAlgorithm::Blake3V1,
                bytes: *blake3::hash(bytes).as_bytes(),
            })
        }
    }

    struct FakeCipher;

    #[async_trait]
    impl TransferCipherPort for FakeCipher {
        async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, TransferCipherError> {
            let mut encrypted = b"encrypted:".to_vec();
            encrypted.extend_from_slice(plaintext);
            Ok(encrypted)
        }

        async fn decrypt(&self, encrypted: &[u8]) -> Result<Vec<u8>, TransferCipherError> {
            encrypted
                .strip_prefix(b"encrypted:")
                .map(ToOwned::to_owned)
                .ok_or(TransferCipherError::InvalidFormat)
        }
    }

    #[derive(Default)]
    struct FakeBlobTransfer {
        store: Mutex<HashMap<BlobDigest, Bytes>>,
        tags: Mutex<Vec<(BlobDigest, TagReason)>>,
        publish_count: Mutex<usize>,
    }

    impl FakeBlobTransfer {
        fn publish_count(&self) -> usize {
            *self.publish_count.lock().expect("lock publish count")
        }

        fn tag_count(&self) -> usize {
            self.tags.lock().expect("lock tags").len()
        }

        fn stored_bytes(&self, digest: &BlobDigest) -> Option<Bytes> {
            self.store.lock().expect("lock store").get(digest).cloned()
        }
    }

    #[async_trait]
    impl BlobTransferPort for FakeBlobTransfer {
        async fn publish(
            &self,
            plaintext: Bytes,
            reason: TagReason,
        ) -> Result<BlobDigest, BlobError> {
            *self.publish_count.lock().expect("lock publish count") += 1;
            let digest = digest_for(&plaintext);
            self.store
                .lock()
                .expect("lock store")
                .insert(digest, plaintext);
            // Phase F: publish atomically attaches the caller-supplied tag
            // (mirrors `IrohBlobTransferAdapter::publish` going through
            // `with_named_tag`). Tests asserting on `tag_count` see the
            // publish-time tag plus any subsequent `tag(...)` calls.
            self.tags.lock().expect("lock tags").push((digest, reason));
            Ok(digest)
        }

        async fn publish_path(
            &self,
            path: &std::path::Path,
            reason: TagReason,
        ) -> Result<BlobDigest, BlobError> {
            // Fake-only: read into memory and delegate to publish() so
            // tests stay simple (small files) while still exercising the
            // path code-path through the use case + facade layers.
            let bytes = tokio::fs::read(path)
                .await
                .map_err(|e| BlobError::Internal(e.to_string()))?;
            self.publish(Bytes::from(bytes), reason).await
        }

        async fn issue_ticket(&self, digest: &BlobDigest) -> Result<BlobTicket, BlobError> {
            Ok(BlobTicket::from_bytes(digest.as_bytes().to_vec()))
        }

        async fn fetch(
            &self,
            ticket: &BlobTicket,
            _progress: Option<&dyn BlobProgressSink>,
        ) -> Result<Bytes, BlobError> {
            let digest = self.digest_of(ticket)?;
            self.store
                .lock()
                .expect("lock store")
                .get(&digest)
                .cloned()
                .ok_or(BlobError::NotFound)
        }

        async fn fetch_to_path(
            &self,
            ticket: &BlobTicket,
            target_path: &std::path::Path,
            progress: Option<&dyn BlobProgressSink>,
        ) -> Result<BlobDigest, BlobError> {
            // Fake-only: read store entry into memory and write the file
            // out — tests work with small payloads, so the in-memory step
            // is fine. Real adapter (`IrohBlobTransferAdapter`) goes
            // through `Blobs::export` for true streaming export.
            let bytes = self.fetch(ticket, progress).await?;
            tokio::fs::write(target_path, &bytes)
                .await
                .map_err(|e| BlobError::Internal(e.to_string()))?;
            self.digest_of(ticket)
        }

        async fn shutdown_inflight_fetch(&self, _ticket: &BlobTicket) -> Result<(), BlobError> {
            // Fake: 测试场景下 fetch 是同步内存读,没有"正在进行中"的 fetch
            // 可中止。返回 Ok 满足幂等契约;真实 adapter
            // (`IrohBlobTransferAdapter`) 走 QUIC connection 拆除路径。
            Ok(())
        }

        async fn has(&self, digest: &BlobDigest) -> Result<bool, BlobError> {
            Ok(self.store.lock().expect("lock store").contains_key(digest))
        }

        async fn tag(&self, digest: &BlobDigest, reason: TagReason) -> Result<(), BlobError> {
            self.tags.lock().expect("lock tags").push((*digest, reason));
            Ok(())
        }

        async fn untag(&self, reason: TagReason) -> Result<(), BlobError> {
            self.tags
                .lock()
                .expect("lock tags")
                .retain(|(_, r)| r != &reason);
            Ok(())
        }

        fn digest_of(&self, ticket: &BlobTicket) -> Result<BlobDigest, BlobError> {
            let bytes: [u8; 32] = ticket
                .as_bytes()
                .try_into()
                .map_err(|_| BlobError::InvalidTicket)?;
            Ok(BlobDigest::from_bytes(bytes))
        }
    }

    #[derive(Default)]
    struct FakeBlobReferenceRepository {
        rows: Mutex<HashMap<PlaintextHash, BlobDigest>>,
        save_count: Mutex<usize>,
    }

    impl FakeBlobReferenceRepository {
        fn save_count(&self) -> usize {
            *self.save_count.lock().expect("lock save count")
        }
    }

    #[async_trait]
    impl BlobReferenceRepositoryPort for FakeBlobReferenceRepository {
        async fn find_by_plaintext_hash(
            &self,
            hash: &PlaintextHash,
        ) -> Result<Option<BlobDigest>, BlobReferenceError> {
            Ok(self.rows.lock().expect("lock rows").get(hash).copied())
        }

        async fn save(
            &self,
            hash: PlaintextHash,
            digest: BlobDigest,
        ) -> Result<(), BlobReferenceError> {
            *self.save_count.lock().expect("lock save count") += 1;
            self.rows.lock().expect("lock rows").insert(hash, digest);
            Ok(())
        }

        async fn forget(&self, hash: &PlaintextHash) -> Result<(), BlobReferenceError> {
            self.rows.lock().expect("lock rows").remove(hash);
            Ok(())
        }
    }

    fn digest_for(bytes: &[u8]) -> BlobDigest {
        BlobDigest::from_bytes(*blake3::hash(bytes).as_bytes())
    }
}
