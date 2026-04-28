//! `BlobTransferPort` — publish / fetch ciphertext + local reference
//! management.
//!
//! Business model:
//!
//! * **Producer side** ([`BlobTransferPort::publish`] +
//!   [`BlobTransferPort::issue_ticket`]) — place a ciphertext in the local
//!   shareable store, receive a stable local identity ([`BlobDigest`]);
//!   mint an out-of-band retrieval credential ([`BlobTicket`]) when a peer
//!   needs to pull it.
//! * **Consumer side** ([`BlobTransferPort::digest_of`] +
//!   [`BlobTransferPort::fetch`]) — resolve a credential's content identity
//!   locally (skip the fetch when already held), otherwise retrieve the
//!   ciphertext via the credential's embedded sources.
//! * **Reference management** ([`BlobTransferPort::tag`] /
//!   [`BlobTransferPort::untag`] / [`BlobTransferPort::has`]) — declare /
//!   release that some business object holds a ciphertext, query whether
//!   the ciphertext is present locally. Reference *scanning* (GC) lives
//!   outside this port (see `task_plan.md §T-02`).
//!
//! Ciphertext in, ciphertext out — this port does **not** encrypt or
//! decrypt. That responsibility belongs to
//! [`BlobCipherPort`](crate::ports::security::BlobCipherPort), driven by
//! the callers above this port.

use async_trait::async_trait;
use bytes::Bytes;

use crate::ids::EntryId;

/// A ciphertext's **stable local identity**.
///
/// Once a ciphertext is placed into the local shareable store, it gets a
/// 32-byte identifier: identical ciphertexts map to identical identifiers,
/// different ciphertexts map to different identifiers. Upper layers use
/// this to answer "do I already have this ciphertext locally?" / "is this
/// ciphertext equivalent to that one?" without loading the full ciphertext
/// into memory.
///
/// The concrete derivation is the storage adapter's responsibility; core
/// treats it as opaque bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobDigest([u8; 32]);

impl BlobDigest {
    pub const fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A **retrieval credential** for a shared ciphertext.
///
/// After a device publishes a large-payload ciphertext (a file, a big
/// image) into its local shareable store, it receives a credential. The
/// credential travels alongside the clipboard-sync notice; peers in the
/// same space consume it in two steps:
///
/// * First ask "which ciphertext does this credential point to?"
///   ([`BlobTransferPort::digest_of`]) — used to skip redundant pulls when
///   the local store already holds the referenced ciphertext.
/// * Then retrieve the ciphertext itself ([`BlobTransferPort::fetch`]) —
///   the storage adapter opens whatever connection the credential's
///   embedded source(s) describe.
///
/// The credential contains **neither the ciphertext itself nor any
/// decryption key**; decryption goes through
/// [`BlobCipherPort`](crate::ports::security::BlobCipherPort) separately.
/// The credential also provides no authenticity guarantee on its own:
/// protection against forgery / substitution comes from space-member
/// relationships and the AEAD on the ciphertext, not from the credential.
///
/// At minimum the credential carries "content identity + at least one
/// reachable source"; concrete encoding is the storage adapter's concern
/// and core treats it as opaque bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobTicket(Vec<u8>);

impl BlobTicket {
    pub fn from_bytes(b: Vec<u8>) -> Self {
        Self(b)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Why a given ciphertext is **currently held**.
///
/// Storage is a shared resource: one ciphertext may be referenced by
/// several clipboard entries (the same file copied repeatedly, or received
/// from multiple devices, all land on the same ciphertext). Callers
/// declare "this ciphertext is referenced by X" via
/// [`BlobTransferPort::tag`] and release the declaration via
/// [`BlobTransferPort::untag`]; the storage adapter uses these records to
/// decide which ciphertexts may be reclaimed and which must stay.
///
/// Phase 1 supports only "referenced by some clipboard entry" — the lone
/// reference source in Slice 3. Future reasons (for example, a
/// user-pinned item) can be added as new variants without breaking
/// existing adapters.
///
/// Reclaim *scanning* itself is out of scope for Phase 1 (see
/// `task_plan.md §T-02`) — Phase 1 only guarantees that declarations and
/// releases are recorded correctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagReason {
    ClipboardEntry(EntryId),
}

/// Business-level errors for [`BlobTransferPort`] operations.
///
/// Intentionally coarse-grained — storage backends vary, but callers only
/// need to distinguish "is it here", "can I pull it", and "is the
/// credential readable". Finer detail belongs in adapter logs.
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    /// Local store does not hold the requested ciphertext. Callers may
    /// choose to pull via a credential or give up.
    #[error("blob not found")]
    NotFound,

    /// Could not pull the ciphertext: source unreachable, source has
    /// already reclaimed it, transfer interrupted, etc. Distinct from
    /// [`BlobError::NotFound`], which means "not local"; this means
    /// "remote side could not deliver either".
    #[error("blob unavailable: {0}")]
    Unavailable(String),

    /// The credential cannot be understood by the current adapter
    /// (version drift, corruption, credential issued by a different
    /// storage backend). Normally signals a deployment / configuration
    /// mismatch between sender and receiver, not a data error.
    #[error("ticket could not be interpreted")]
    InvalidTicket,

    /// Adapter-internal failure (IO, upstream library error, etc.).
    /// Callers usually just record and surface.
    #[error("internal: {0}")]
    Internal(String),
}

/// 字节级进度上报通道。
///
/// adapter 在 fetch 过程中可以调用 sink 通知调用方"已经传输了多少字节"。
/// adapter 自己负责做合理节流(字节阈值 + 时间窗),sink 实现端不应假设
/// 调用频率,但也不应该在实现里再做长时间的同步阻塞操作 ——
/// adapter 通常在网络循环里调用 sink。
///
/// `total_bytes` 由 adapter 透传:
/// - 如果 adapter 知道总大小(例如 iroh-blobs 在 PartComplete 时知道 size),
///   就传 `Some`;
/// - 否则传 `None`,由 sink 实现层自己结合外部已知的大小处理。
#[async_trait]
pub trait BlobProgressSink: Send + Sync {
    /// 上报当前累计已传输字节数。
    ///
    /// `bytes_transferred` 是单调递增的累计值(adapter 保证不回退);
    /// `total_bytes` 在已知时透传,未知时为 `None`。
    async fn report(&self, bytes_transferred: u64, total_bytes: Option<u64>);
}

/// Blob transfer capability: publish / retrieve / lifecycle management.
///
/// See module-level docs for the producer / consumer / reference split.
/// This port does **not** perform encryption or decryption — the bytes on
/// every method boundary are ciphertext, and cryptographic operations go
/// through [`BlobCipherPort`](crate::ports::security::BlobCipherPort).
#[async_trait]
pub trait BlobTransferPort: Send + Sync {
    // ── Producer side ──

    /// Place a ciphertext into the local shareable store, return its
    /// stable local identity. Idempotent: publishing the same bytes again
    /// returns the same [`BlobDigest`].
    async fn publish(&self, ciphertext: Bytes) -> Result<BlobDigest, BlobError>;

    /// Mint an out-of-band retrieval credential for a ciphertext the
    /// local store already holds, so other devices can pull from this
    /// one. The credential carries at minimum "content identity + at
    /// least one reachable source".
    async fn issue_ticket(&self, digest: &BlobDigest) -> Result<BlobTicket, BlobError>;

    // ── Consumer side ──

    /// Retrieve a ciphertext given its credential. If the local store
    /// already holds the referenced ciphertext the adapter may serve
    /// it directly; otherwise it pulls via the credential's embedded
    /// sources. Resume-on-interrupt and integrity verification are the
    /// adapter's concern — callers only see "got it" or "didn't".
    ///
    /// `progress` 可选:传入时 adapter 会在拉取过程中按字节阈值/时间窗
    /// 节流上报已传输字节数;不需要进度时传 `None`。本地命中时不会上报
    /// 任何进度(直接返回缓存)。
    async fn fetch(
        &self,
        ticket: &BlobTicket,
        progress: Option<&dyn BlobProgressSink>,
    ) -> Result<Bytes, BlobError>;

    // ── Lifecycle ──

    /// Whether the local store currently holds the ciphertext.
    async fn has(&self, digest: &BlobDigest) -> Result<bool, BlobError>;

    /// Declare that some business object references the given
    /// ciphertext, deferring reclaim.
    async fn tag(&self, digest: &BlobDigest, reason: TagReason) -> Result<(), BlobError>;

    /// Release a declaration previously made via [`tag`](Self::tag).
    /// Idempotent: releasing a declaration that does not exist returns
    /// `Ok(())`.
    async fn untag(&self, digest: &BlobDigest, reason: TagReason) -> Result<(), BlobError>;

    // ── Metadata ──

    /// Resolve the content identity a credential points to, purely
    /// locally — no network, no local-store IO. Typical usage: after
    /// receiving a clipboard notice, ask this first and skip the fetch
    /// when [`has`](Self::has) confirms the ciphertext is already held.
    ///
    /// Returns [`BlobError::InvalidTicket`] if the credential cannot be
    /// interpreted by the current adapter.
    fn digest_of(&self, ticket: &BlobTicket) -> Result<BlobDigest, BlobError>;
}
