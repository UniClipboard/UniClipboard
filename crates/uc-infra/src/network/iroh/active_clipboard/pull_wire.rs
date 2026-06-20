//! Wire codec for the active-clipboard pull protocol.
//!
//! This is an independent sibling of the bulk clipboard codec
//! ([`super::clipboard_wire`]) and the active-clipboard state codec
//! ([`super::wire`]). A pull is a single request → response
//! exchange on one bi-stream:
//!
//! ```text
//! requester -> holder (request):
//!   [magic(1) | snapshot_hash_len_be(2) | snapshot_hash_bytes | FIN]
//!
//! holder -> requester (response):
//!   [status(1) | (status==Ok ? envelope_len_be(4) | envelope_bytes) | FIN]
//! ```
//!
//! * `magic` = [`ACTIVE_PULL_MAGIC`] — a fixed sentinel so bytes arriving on a
//!   mis-routed ALPN are rejected before anything is allocated.
//! * `snapshot_hash` is the cross-device `"blake3v1:<hex>"` identity string,
//!   length-capped at [`MAX_SNAPSHOT_HASH_LEN`] before allocation.
//! * `status` discriminates the response (see [`PullResponseStatus`]). Only
//!   `Ok` carries an envelope; the error statuses are a single byte.
//! * `envelope_bytes` is the transfer-encrypted clipboard payload the holder
//!   produced — the same opaque wire format the bulk clipboard sync path uses.
//!   It is **not** decoded here; the codec only frames it, length-capped at
//!   [`MAX_ENVELOPE_LEN`].
//!
//! This codec is independent of the 0xC1 bulk codec on purpose: that codec's
//! `read_frame` hard-rejects any magic other than its own, so the two frame
//! formats never share a parser, and the pull path never touches it.

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// ============================================================================
// Constants
// ============================================================================

/// Sentinel byte identifying an active-clipboard pull request frame at the
/// head of the stream. Distinct from the bulk clipboard codec
/// ([`CLIPBOARD_MAGIC`](super::clipboard_wire::CLIPBOARD_MAGIC) = `0xC1`) and
/// the active-clipboard state codec
/// ([`ACTIVE_CLIPBOARD_MAGIC`](super::wire::ACTIVE_CLIPBOARD_MAGIC) = `0xC3`)
/// so a mis-routed connection fails fast at the magic check.
pub const ACTIVE_PULL_MAGIC: u8 = 0xC2;

/// Hard ceiling on the request's snapshot-hash length. A `"blake3v1:<hex>"`
/// string is ~71 bytes; 1 KiB leaves ample headroom while bounding the
/// allocation a hostile peer can request.
pub const MAX_SNAPSHOT_HASH_LEN: u16 = 1024;

/// Hard ceiling on the response envelope length (16 MiB). Inline V3 envelopes
/// carry small/text content directly; large content travels as blob refs
/// (small envelope + out-of-band blob fetch), so this only needs to cover the
/// inline ceiling plus headroom — not the full blob payload.
pub const MAX_ENVELOPE_LEN: u32 = 16 * 1024 * 1024;

// ============================================================================
// Response status
// ============================================================================

/// Discriminator byte at the head of a pull response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullResponseStatus {
    /// The holder produced a transfer envelope; the body follows.
    Ok,
    /// The holder does not hold the requested content.
    NotAvailable,
    /// The holder's session is locked, so it could not produce the envelope.
    Locked,
    /// The holder hit an internal failure while building the envelope.
    Internal,
}

impl PullResponseStatus {
    const OK: u8 = 0;
    const NOT_AVAILABLE: u8 = 1;
    const LOCKED: u8 = 2;
    const INTERNAL: u8 = 3;

    fn as_byte(self) -> u8 {
        match self {
            PullResponseStatus::Ok => Self::OK,
            PullResponseStatus::NotAvailable => Self::NOT_AVAILABLE,
            PullResponseStatus::Locked => Self::LOCKED,
            PullResponseStatus::Internal => Self::INTERNAL,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        match b {
            Self::OK => Some(PullResponseStatus::Ok),
            Self::NOT_AVAILABLE => Some(PullResponseStatus::NotAvailable),
            Self::LOCKED => Some(PullResponseStatus::Locked),
            Self::INTERNAL => Some(PullResponseStatus::Internal),
            _ => None,
        }
    }
}

/// A decoded pull response: either the transfer envelope or a typed
/// no-content reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullResponse {
    /// The holder served the content; carries the transfer envelope bytes.
    Envelope(Vec<u8>),
    /// The holder does not hold the requested content.
    NotAvailable,
    /// The holder's session was locked and could not serve.
    Locked,
    /// The holder failed internally while building the envelope.
    Internal,
}

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum PullWireError {
    #[error("bad magic byte: got 0x{got:02X} (expected 0x{expected:02X})")]
    BadMagic { got: u8, expected: u8 },
    #[error("snapshot hash length {len} exceeds maximum {max}")]
    SnapshotHashTooLong { len: u16, max: u16 },
    #[error("snapshot hash bytes are not valid UTF-8")]
    SnapshotHashNotUtf8,
    #[error("envelope length {len} exceeds maximum {max}")]
    EnvelopeTooLong { len: u32, max: u32 },
    #[error("unknown response status byte 0x{got:02X}")]
    UnknownStatus { got: u8 },
    #[error("stream io: {0}")]
    Io(std::io::Error),
}

// ============================================================================
// Request framing
// ============================================================================

/// Serialize + send the pull request frame: `magic | hash_len | hash_bytes`.
/// The caller closes the send half after this returns.
pub async fn write_request<W: AsyncWrite + Unpin>(
    send: &mut W,
    snapshot_hash: &str,
) -> Result<(), PullWireError> {
    let bytes = snapshot_hash.as_bytes();
    if bytes.len() > MAX_SNAPSHOT_HASH_LEN as usize {
        return Err(PullWireError::SnapshotHashTooLong {
            len: bytes.len().min(u16::MAX as usize) as u16,
            max: MAX_SNAPSHOT_HASH_LEN,
        });
    }
    let len = bytes.len() as u16;

    send.write_all(&[ACTIVE_PULL_MAGIC])
        .await
        .map_err(PullWireError::Io)?;
    send.write_all(&len.to_be_bytes())
        .await
        .map_err(PullWireError::Io)?;
    send.write_all(bytes).await.map_err(PullWireError::Io)?;
    Ok(())
}

/// Read one pull request frame, validating magic + length cap **before**
/// allocating the snapshot-hash buffer. Returns the requested snapshot hash.
pub async fn read_request<R: AsyncRead + Unpin>(recv: &mut R) -> Result<String, PullWireError> {
    let mut magic = [0u8; 1];
    recv.read_exact(&mut magic)
        .await
        .map_err(PullWireError::Io)?;
    if magic[0] != ACTIVE_PULL_MAGIC {
        return Err(PullWireError::BadMagic {
            got: magic[0],
            expected: ACTIVE_PULL_MAGIC,
        });
    }

    let mut len_buf = [0u8; 2];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(PullWireError::Io)?;
    let len = u16::from_be_bytes(len_buf);
    if len > MAX_SNAPSHOT_HASH_LEN {
        return Err(PullWireError::SnapshotHashTooLong {
            len,
            max: MAX_SNAPSHOT_HASH_LEN,
        });
    }

    let mut body = vec![0u8; len as usize];
    recv.read_exact(&mut body)
        .await
        .map_err(PullWireError::Io)?;
    String::from_utf8(body).map_err(|_| PullWireError::SnapshotHashNotUtf8)
}

// ============================================================================
// Response framing
// ============================================================================

/// Serialize + send a pull response frame. `Ok` writes the status byte
/// followed by a length-prefixed envelope; the error statuses write a single
/// status byte. The caller closes the send half after this returns.
pub async fn write_response<W: AsyncWrite + Unpin>(
    send: &mut W,
    response: &PullResponse,
) -> Result<(), PullWireError> {
    match response {
        PullResponse::Envelope(bytes) => {
            if bytes.len() > MAX_ENVELOPE_LEN as usize {
                return Err(PullWireError::EnvelopeTooLong {
                    len: bytes.len().min(u32::MAX as usize) as u32,
                    max: MAX_ENVELOPE_LEN,
                });
            }
            send.write_all(&[PullResponseStatus::Ok.as_byte()])
                .await
                .map_err(PullWireError::Io)?;
            send.write_all(&(bytes.len() as u32).to_be_bytes())
                .await
                .map_err(PullWireError::Io)?;
            send.write_all(bytes).await.map_err(PullWireError::Io)?;
        }
        PullResponse::NotAvailable => {
            send.write_all(&[PullResponseStatus::NotAvailable.as_byte()])
                .await
                .map_err(PullWireError::Io)?;
        }
        PullResponse::Locked => {
            send.write_all(&[PullResponseStatus::Locked.as_byte()])
                .await
                .map_err(PullWireError::Io)?;
        }
        PullResponse::Internal => {
            send.write_all(&[PullResponseStatus::Internal.as_byte()])
                .await
                .map_err(PullWireError::Io)?;
        }
    }
    Ok(())
}

/// Read one pull response frame: a status byte, plus a length-prefixed
/// envelope when the status is `Ok`. The envelope length is validated against
/// [`MAX_ENVELOPE_LEN`] before allocation.
pub async fn read_response<R: AsyncRead + Unpin>(
    recv: &mut R,
) -> Result<PullResponse, PullWireError> {
    let mut status_buf = [0u8; 1];
    recv.read_exact(&mut status_buf)
        .await
        .map_err(PullWireError::Io)?;
    let status = PullResponseStatus::from_byte(status_buf[0])
        .ok_or(PullWireError::UnknownStatus { got: status_buf[0] })?;

    match status {
        PullResponseStatus::Ok => {
            let mut len_buf = [0u8; 4];
            recv.read_exact(&mut len_buf)
                .await
                .map_err(PullWireError::Io)?;
            let len = u32::from_be_bytes(len_buf);
            if len > MAX_ENVELOPE_LEN {
                return Err(PullWireError::EnvelopeTooLong {
                    len,
                    max: MAX_ENVELOPE_LEN,
                });
            }
            let mut body = vec![0u8; len as usize];
            recv.read_exact(&mut body)
                .await
                .map_err(PullWireError::Io)?;
            Ok(PullResponse::Envelope(body))
        }
        PullResponseStatus::NotAvailable => Ok(PullResponse::NotAvailable),
        PullResponseStatus::Locked => Ok(PullResponse::Locked),
        PullResponseStatus::Internal => Ok(PullResponse::Internal),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn request_round_trip(hash: &str) -> Result<String, PullWireError> {
        let (mut client, mut server) = duplex(64 * 1024);
        let h = hash.to_string();
        let send_task = tokio::spawn(async move {
            write_request(&mut client, &h).await.expect("write request");
            client.shutdown().await.expect("shutdown");
        });
        let got = read_request(&mut server).await?;
        send_task.await.unwrap();
        Ok(got)
    }

    async fn response_round_trip(resp: &PullResponse) -> Result<PullResponse, PullWireError> {
        let (mut client, mut server) = duplex(64 * 1024);
        let r = resp.clone();
        let send_task = tokio::spawn(async move {
            write_response(&mut client, &r)
                .await
                .expect("write response");
            client.shutdown().await.expect("shutdown");
        });
        let got = read_response(&mut server).await?;
        send_task.await.unwrap();
        Ok(got)
    }

    /// 1. Request round-trips the snapshot hash field-for-field.
    #[tokio::test]
    async fn request_round_trips_snapshot_hash() {
        let hash = format!("blake3v1:{}", "a".repeat(64));
        let got = request_round_trip(&hash).await.expect("round trip");
        assert_eq!(got, hash);
    }

    /// 2. The Ok envelope response round-trips the raw bytes verbatim.
    #[tokio::test]
    async fn ok_envelope_response_round_trips() {
        let envelope = vec![0x55, 0x43, 0x33, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
        let got = response_round_trip(&PullResponse::Envelope(envelope.clone()))
            .await
            .expect("round trip");
        assert_eq!(got, PullResponse::Envelope(envelope));
    }

    /// 3. Each error status round-trips as its typed variant (no body).
    #[tokio::test]
    async fn error_status_responses_round_trip() {
        for resp in [
            PullResponse::NotAvailable,
            PullResponse::Locked,
            PullResponse::Internal,
        ] {
            let got = response_round_trip(&resp).await.expect("round trip");
            assert_eq!(got, resp);
        }
    }

    /// 4. A request frame with the wrong magic is rejected at the magic check,
    /// before allocating the snapshot-hash buffer.
    #[tokio::test]
    async fn read_request_rejects_bad_magic() {
        let (mut client, mut server) = duplex(64);
        let send_task = tokio::spawn(async move {
            // 0xC1 is the bulk codec's magic.
            client.write_all(&[0xC1, 0x00, 0x01]).await.ok();
            client.shutdown().await.ok();
        });
        let err = read_request(&mut server)
            .await
            .expect_err("bad magic must be rejected");
        match err {
            PullWireError::BadMagic { got, expected } => {
                assert_eq!(got, 0xC1);
                assert_eq!(expected, ACTIVE_PULL_MAGIC);
            }
            other => panic!("expected BadMagic, got {other:?}"),
        }
        send_task.await.unwrap();
    }

    /// 5. A request claiming an over-long snapshot hash is rejected at the
    /// length prefix, before allocating the body buffer.
    #[tokio::test]
    async fn read_request_rejects_oversized_snapshot_hash() {
        let (mut client, mut server) = duplex(64);
        let oversized = MAX_SNAPSHOT_HASH_LEN + 1;
        let send_task = tokio::spawn(async move {
            client.write_all(&[ACTIVE_PULL_MAGIC]).await.ok();
            client.write_all(&oversized.to_be_bytes()).await.ok();
            client.shutdown().await.ok();
        });
        let err = read_request(&mut server)
            .await
            .expect_err("oversized snapshot hash must be rejected");
        match err {
            PullWireError::SnapshotHashTooLong { len, max } => {
                assert_eq!(len, oversized);
                assert_eq!(max, MAX_SNAPSHOT_HASH_LEN);
            }
            other => panic!("expected SnapshotHashTooLong, got {other:?}"),
        }
        send_task.await.unwrap();
    }

    /// 6. A response claiming an over-long envelope is rejected at the length
    /// prefix, before allocating the body buffer.
    #[tokio::test]
    async fn read_response_rejects_oversized_envelope() {
        let (mut client, mut server) = duplex(64);
        let oversized = MAX_ENVELOPE_LEN + 1;
        let send_task = tokio::spawn(async move {
            client
                .write_all(&[PullResponseStatus::Ok.as_byte()])
                .await
                .ok();
            client.write_all(&oversized.to_be_bytes()).await.ok();
            client.shutdown().await.ok();
        });
        let err = read_response(&mut server)
            .await
            .expect_err("oversized envelope must be rejected");
        match err {
            PullWireError::EnvelopeTooLong { len, max } => {
                assert_eq!(len, oversized);
                assert_eq!(max, MAX_ENVELOPE_LEN);
            }
            other => panic!("expected EnvelopeTooLong, got {other:?}"),
        }
        send_task.await.unwrap();
    }

    /// 7. An unknown response status byte is rejected explicitly rather than
    /// misinterpreted.
    #[tokio::test]
    async fn read_response_rejects_unknown_status() {
        let (mut client, mut server) = duplex(64);
        let send_task = tokio::spawn(async move {
            client.write_all(&[0xFF]).await.ok();
            client.shutdown().await.ok();
        });
        let err = read_response(&mut server)
            .await
            .expect_err("unknown status must be rejected");
        match err {
            PullWireError::UnknownStatus { got } => assert_eq!(got, 0xFF),
            other => panic!("expected UnknownStatus, got {other:?}"),
        }
        send_task.await.unwrap();
    }

    /// 8. A truncated request (magic + partial length prefix) surfaces as an
    /// Io error, not a panic.
    #[tokio::test]
    async fn read_request_surfaces_truncation_as_io() {
        let (mut client, mut server) = duplex(64);
        let send_task = tokio::spawn(async move {
            client.write_all(&[ACTIVE_PULL_MAGIC, 0x00]).await.ok();
            client.shutdown().await.ok();
        });
        let err = read_request(&mut server)
            .await
            .expect_err("truncation must surface as error");
        assert!(matches!(err, PullWireError::Io(_)));
        send_task.await.unwrap();
    }
}
