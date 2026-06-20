//! Wire codec for the active-clipboard state protocol.
//!
//! This is an independent sibling of the bulk clipboard codec
//! ([`super::clipboard_wire`]). The active-clipboard protocol carries a
//! small last-writer-wins (LWW) register observation — "this content is now
//! the active clipboard" — not the content bytes themselves. It is a clean
//! sibling on purpose: the bulk codec's [`read_frame`](super::clipboard_wire::read_frame)
//! hard-rejects any magic other than its own, so the two frame formats never
//! share a parser.
//!
//! ## Frame layout
//!
//! ```text
//! sender -> receiver (one uni- or bi-stream, one direction):
//!   [magic(1) | body_len_be(4) | body_bytes | FIN]
//! ```
//!
//! * `magic` = [`ACTIVE_CLIPBOARD_MAGIC`] — a fixed sentinel so bytes
//!   arriving on a mis-routed ALPN are rejected before postcard runs.
//! * `body` is a postcard-encoded [`WireActiveStateV1`]. Unlike the bulk
//!   codec there is no separate ciphertext payload — the whole message is
//!   the register observation.
//! * `body_len` is a big-endian `u32`, capped at [`MAX_BODY_SIZE`] before
//!   allocation to prevent an unbounded allocation from a hostile peer.
//!
//! ## Versioning
//!
//! postcard is positional/non-tagged, so a new field cannot be added in
//! place without breaking the wire. The body's leading `version` byte lets a
//! future revision add a `WireActiveStateV2` and dispatch decode on the
//! version (postcard encodes `u8 < 128` as a single byte, so peeking
//! `bytes[0]` is sufficient). v1 is the only schema today; anything else is
//! rejected with [`ActiveWireDecodeError::UnsupportedVersion`].

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// ============================================================================
// Constants
// ============================================================================

/// Sentinel byte identifying an active-clipboard state frame at the head of
/// the stream. Distinct from the bulk clipboard codec
/// ([`CLIPBOARD_MAGIC`](super::clipboard_wire::CLIPBOARD_MAGIC) = `0xC1`) so a
/// mis-routed connection fails fast instead of drifting into postcard.
pub const ACTIVE_CLIPBOARD_MAGIC: u8 = 0xC3;

/// Current wire schema version, encoded as the body's leading byte.
pub const ACTIVE_STATE_WIRE_VERSION: u8 = 1;

/// Hard ceiling on the postcard-encoded body size. The body is a handful of
/// short strings (a content hash, two ids, a timestamp); 4 KiB leaves ample
/// headroom for future optional fields without inviting oversized
/// allocations from a hostile peer.
pub const MAX_BODY_SIZE: u32 = 4 * 1024;

// ============================================================================
// Wire types
// ============================================================================

/// The decoded active-clipboard state observation handed to / from the
/// codec. Infra-local mirror of the cross-device register fields; kept
/// separate from any core type so `uc-core` stays free of `serde` derives on
/// its domain structs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveClipboardWireMessage {
    /// Stable, cross-device content identity string (`"blake3v1:<hex>"`).
    pub snapshot_hash: String,
    /// The sending device's local entry handle. Per-device only.
    pub entry_id: String,
    /// Wall-clock milliseconds of the activation event. Primary LWW key.
    pub activated_at_ms: i64,
    /// The device that performed the activation. LWW tiebreaker / attribution.
    pub activated_by: String,
}

/// postcard-serialized v1 body. The leading `version` byte is decoded first
/// so a future `WireActiveStateV2` can be dispatched without a flag day.
#[derive(Serialize, Deserialize, Debug)]
struct WireActiveStateV1 {
    version: u8,
    snapshot_hash: String,
    entry_id: String,
    activated_at_ms: i64,
    activated_by: String,
}

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum ActiveWireEncodeError {
    #[error("postcard encode failed: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("body too large: {size} bytes (max {max})")]
    BodyTooLarge { size: usize, max: u32 },
    #[error("stream io: {0}")]
    Io(std::io::Error),
}

#[derive(Debug, Error)]
pub enum ActiveWireDecodeError {
    #[error("postcard decode failed: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("unsupported active-clipboard wire version {got} (this build understands {expected})")]
    UnsupportedVersion { got: u8, expected: u8 },
    #[error("bad magic byte: got 0x{got:02X} (expected 0x{expected:02X})")]
    BadMagic { got: u8, expected: u8 },
    #[error("body size {size} exceeds maximum {max}")]
    BodyTooLarge { size: u32, max: u32 },
    #[error("stream io: {0}")]
    Io(std::io::Error),
}

// ============================================================================
// Public API — pure byte codec
// ============================================================================

/// Serialize an [`ActiveClipboardWireMessage`] body for the wire. Does not
/// include the magic byte or length prefix — callers typically run this once
/// and hand the bytes to [`write_frame`].
pub fn encode_body(msg: &ActiveClipboardWireMessage) -> Result<Vec<u8>, ActiveWireEncodeError> {
    let wire = WireActiveStateV1 {
        version: ACTIVE_STATE_WIRE_VERSION,
        snapshot_hash: msg.snapshot_hash.clone(),
        entry_id: msg.entry_id.clone(),
        activated_at_ms: msg.activated_at_ms,
        activated_by: msg.activated_by.clone(),
    };
    let bytes = postcard::to_allocvec(&wire)?;
    if bytes.len() > MAX_BODY_SIZE as usize {
        return Err(ActiveWireEncodeError::BodyTooLarge {
            size: bytes.len(),
            max: MAX_BODY_SIZE,
        });
    }
    Ok(bytes)
}

/// Deserialize a body from its postcard byte form. Dispatches on the leading
/// version byte; anything outside the supported set (`{1}`) is rejected with
/// [`ActiveWireDecodeError::UnsupportedVersion`].
pub fn decode_body(bytes: &[u8]) -> Result<ActiveClipboardWireMessage, ActiveWireDecodeError> {
    // postcard encodes a `u8 < 128` as a single byte, so peeking the first
    // byte recovers the version. An empty slice maps to postcard's short-read
    // error so we reuse the existing branch instead of adding a variant.
    let version = bytes
        .first()
        .copied()
        .ok_or(ActiveWireDecodeError::Postcard(
            postcard::Error::DeserializeUnexpectedEnd,
        ))?;
    match version {
        1 => {
            let wire: WireActiveStateV1 = postcard::from_bytes(bytes)?;
            Ok(ActiveClipboardWireMessage {
                snapshot_hash: wire.snapshot_hash,
                entry_id: wire.entry_id,
                activated_at_ms: wire.activated_at_ms,
                activated_by: wire.activated_by,
            })
        }
        other => Err(ActiveWireDecodeError::UnsupportedVersion {
            got: other,
            expected: ACTIVE_STATE_WIRE_VERSION,
        }),
    }
}

// ============================================================================
// Public API — stream I/O
// ============================================================================

/// Serialize + send one active-clipboard state frame: magic | body_len |
/// body. The caller closes the send half after this returns so the peer's
/// final read hits EOF cleanly.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    send: &mut W,
    msg: &ActiveClipboardWireMessage,
) -> Result<(), ActiveWireEncodeError> {
    let body = encode_body(msg)?;
    let body_len = body.len() as u32; // bounded by MAX_BODY_SIZE

    send.write_all(&[ACTIVE_CLIPBOARD_MAGIC])
        .await
        .map_err(ActiveWireEncodeError::Io)?;
    send.write_all(&body_len.to_be_bytes())
        .await
        .map_err(ActiveWireEncodeError::Io)?;
    send.write_all(&body)
        .await
        .map_err(ActiveWireEncodeError::Io)?;
    Ok(())
}

/// Read one active-clipboard state frame from a stream, validating magic +
/// size cap **before** allocating the body buffer.
pub async fn read_frame<R: AsyncRead + Unpin>(
    recv: &mut R,
) -> Result<ActiveClipboardWireMessage, ActiveWireDecodeError> {
    let mut magic_buf = [0u8; 1];
    recv.read_exact(&mut magic_buf)
        .await
        .map_err(ActiveWireDecodeError::Io)?;
    if magic_buf[0] != ACTIVE_CLIPBOARD_MAGIC {
        return Err(ActiveWireDecodeError::BadMagic {
            got: magic_buf[0],
            expected: ACTIVE_CLIPBOARD_MAGIC,
        });
    }

    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(ActiveWireDecodeError::Io)?;
    let body_len = u32::from_be_bytes(len_buf);
    if body_len > MAX_BODY_SIZE {
        return Err(ActiveWireDecodeError::BodyTooLarge {
            size: body_len,
            max: MAX_BODY_SIZE,
        });
    }
    let mut body = vec![0u8; body_len as usize];
    recv.read_exact(&mut body)
        .await
        .map_err(ActiveWireDecodeError::Io)?;

    decode_body(&body)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn sample_message() -> ActiveClipboardWireMessage {
        ActiveClipboardWireMessage {
            snapshot_hash: format!("blake3v1:{}", "a".repeat(64)),
            entry_id: "01941b00-0000-7000-8000-000000000001".to_string(),
            activated_at_ms: 1_700_000_000_000,
            activated_by: "dev-alpha".to_string(),
        }
    }

    async fn round_trip(
        msg: &ActiveClipboardWireMessage,
    ) -> Result<ActiveClipboardWireMessage, ActiveWireDecodeError> {
        let (mut client, mut server) = duplex(64 * 1024);
        let m = msg.clone();
        let send_task = tokio::spawn(async move {
            write_frame(&mut client, &m).await.expect("write frame");
            client.shutdown().await.expect("shutdown client");
        });
        let recovered = read_frame(&mut server).await?;
        send_task.await.unwrap();
        Ok(recovered)
    }

    /// 1. Normal round-trip — the message recovers field-for-field.
    #[tokio::test]
    async fn write_then_read_round_trips_message() {
        let msg = sample_message();
        let recovered = round_trip(&msg).await.expect("round trip");
        assert_eq!(recovered, msg);
    }

    /// 2. Pure byte codec round-trip without the stream layer.
    #[test]
    fn encode_then_decode_body_round_trips() {
        let msg = sample_message();
        let bytes = encode_body(&msg).unwrap();
        let decoded = decode_body(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    /// 3. Bad magic byte — receiver rejects before decoding the postcard
    /// body or allocating buffers.
    #[tokio::test]
    async fn read_frame_rejects_bad_magic_byte() {
        let (mut client, mut server) = duplex(64);
        let send_task = tokio::spawn(async move {
            // Wrong magic (0xC1 is the bulk codec's), then plausible garbage.
            client.write_all(&[0xC1, 0x00, 0x00, 0x00, 0x01]).await.ok();
            client.shutdown().await.ok();
        });

        let err = read_frame(&mut server)
            .await
            .expect_err("bad magic must be rejected");
        match err {
            ActiveWireDecodeError::BadMagic { got, expected } => {
                assert_eq!(got, 0xC1);
                assert_eq!(expected, ACTIVE_CLIPBOARD_MAGIC);
            }
            other => panic!("expected BadMagic, got {other:?}"),
        }
        send_task.await.unwrap();
    }

    /// 4. Body length exceeds MAX_BODY_SIZE — rejected at the length prefix,
    /// before the receiver allocates the body buffer.
    #[tokio::test]
    async fn read_frame_rejects_oversized_body_length() {
        let (mut client, mut server) = duplex(64);
        let oversized = MAX_BODY_SIZE + 1;
        let send_task = tokio::spawn(async move {
            client.write_all(&[ACTIVE_CLIPBOARD_MAGIC]).await.ok();
            client.write_all(&oversized.to_be_bytes()).await.ok();
            client.shutdown().await.ok();
        });

        let err = read_frame(&mut server)
            .await
            .expect_err("oversized body length must be rejected");
        match err {
            ActiveWireDecodeError::BodyTooLarge { size, max } => {
                assert_eq!(size, oversized);
                assert_eq!(max, MAX_BODY_SIZE);
            }
            other => panic!("expected BodyTooLarge, got {other:?}"),
        }
        send_task.await.unwrap();
    }

    /// 5. Truncated stream — peer drops after magic + partial length prefix.
    /// `read_exact` surfaces the EOF as an Io error, not a panic.
    #[tokio::test]
    async fn read_frame_surfaces_truncation_as_io_error() {
        let (mut client, mut server) = duplex(64);
        let send_task = tokio::spawn(async move {
            client
                .write_all(&[ACTIVE_CLIPBOARD_MAGIC, 0x00, 0x00])
                .await
                .ok();
            client.shutdown().await.ok();
        });

        let err = read_frame(&mut server)
            .await
            .expect_err("truncated prefix must surface as error");
        match err {
            ActiveWireDecodeError::Io(_) => {}
            other => panic!("expected Io error for truncation, got {other:?}"),
        }
        send_task.await.unwrap();
    }

    /// 6. Unsupported version — forge a body at version+1 and confirm the
    /// decoder rejects explicitly instead of interpreting it as v1.
    #[test]
    fn decode_rejects_future_body_version() {
        let future = WireActiveStateV1 {
            version: ACTIVE_STATE_WIRE_VERSION + 1,
            snapshot_hash: "blake3v1:stub".to_string(),
            entry_id: "e".to_string(),
            activated_at_ms: 0,
            activated_by: "d".to_string(),
        };
        let bytes = postcard::to_allocvec(&future).unwrap();

        match decode_body(&bytes) {
            Err(ActiveWireDecodeError::UnsupportedVersion { got, expected }) => {
                assert_eq!(got, ACTIVE_STATE_WIRE_VERSION + 1);
                assert_eq!(expected, ACTIVE_STATE_WIRE_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    /// 7. Empty body — decode maps an empty slice to a short-read error
    /// rather than panicking on the version peek.
    #[test]
    fn decode_empty_body_is_a_postcard_error() {
        match decode_body(&[]) {
            Err(ActiveWireDecodeError::Postcard(_)) => {}
            other => panic!("expected Postcard short-read error, got {other:?}"),
        }
    }
}
