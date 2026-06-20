//! V3 envelope codec for clipboard sync (Slice 2 Phase 3 · T2).
//!
//! Wraps the raw `uc-core::network::protocol::ClipboardBinaryPayload` V3
//! codec with snapshot-level convenience:
//!
//! * [`encode_snapshot_to_v3_bytes`] — take a `SystemClipboardSnapshot`
//!   (the daemon's capture output), serialise every representation into
//!   the V3 envelope, return the plaintext `Bytes` to feed
//!   `ClipboardSyncFacade::dispatch_entry` + the dedup hash string.
//! * [`decode_v3_bytes_to_snapshot`] — inverse of the above: consume the
//!   plaintext from `IngestInboundClipboardUseCase`'s `InboundNotice` and
//!   produce a fresh `SystemClipboardSnapshot` with new
//!   `RepresentationId`s (receiver-local identity, since representations
//!   from a peer have no meaningful ID continuity).
//!
//! Why not put these inside the facade? Because
//! `ApplyInboundClipboardUseCase` (T4) decodes in the use case layer —
//! daemon's worker just hands over the opaque bytes it received, and
//! the use case owns the codec + persistence + dedup decisions.
//!
//! Phase 2 consumers (CLI `send` / `watch`) that still deal in raw text
//! bytes can keep using the `payload_version=3` marker + caller-computed
//! `snapshot_hash` via `dispatch_entry`; Phase 3 CLI upgrades (T9/T10) go
//! through these helpers to match the daemon's wire format.

use std::io::{Read, Write};

use anyhow::{anyhow, Result};
use bytes::Bytes;

use uc_core::clipboard::normalize_wire_mime;
use uc_core::ids::{EntryId, FormatId, RepresentationId};
use uc_core::network::protocol::{BinaryRepresentation, ClipboardBinaryPayload};
use uc_core::ports::blob::BlobTicket;
#[cfg(test)]
use uc_core::MimeType;
use uc_core::{ObservedClipboardRepresentation, SystemClipboardSnapshot};

/// V3 blob refs trailer magic. Each ref carries 6 fields:
/// `ticket / entry_id / filename / mime / size_bytes / representation_index`.
const BLOB_REFS_MAGIC: &[u8; 4] = b"UCBS";
const NONE_STRING_LEN: u16 = u16::MAX;
const NONE_U32_MARKER: u8 = 0;
const SOME_U32_MARKER: u8 = 1;
const MAX_BLOB_REFS: usize = 1_024;
const MAX_TICKET_LEN: usize = 64 * 1024;
const MAX_BLOB_REF_STRING_LEN: usize = 8 * 1024;

/// V3 尾部扩展里的 blob 引用。
///
/// `ticket` 负责定位内容；`entry_id` 负责在接收端登记本次剪贴板归属；
/// `representation_index` 当 `Some(i)` 时表示这条 blob 的 bytes 应当被
/// 灌回到 envelope 主体 `representations[i]`（image/binary 等无法 inline
/// 的 rep 走这条路），而不是当成独立 file 写入接收端 cache 目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V3BlobRef {
    pub ticket: BlobTicket,
    pub entry_id: EntryId,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub size_bytes: u64,
    pub representation_index: Option<u32>,
}

/// Encode a snapshot into the V3 wire envelope and return
/// `(envelope_bytes, snapshot_hash)`.
///
/// `snapshot_hash` is the canonical `"blake3v1:<hex>"` string produced by
/// `snapshot.snapshot_hash()` — the same value the daemon will store in
/// `clipboard_event.snapshot_hash` after a local capture, so receiver
/// dedup via `find_entry_id_by_snapshot_hash` matches.
///
/// The envelope bytes are the application-layer plaintext; the transfer
/// cipher (`TransferCipherPort`) wraps them before they hit the wire.
pub(crate) fn encode_snapshot_to_v3_bytes(
    snapshot: &SystemClipboardSnapshot,
) -> Result<(Bytes, String)> {
    // V3 envelope BinaryRepresentation 仅承载 inline 字节;LocalFile source 必须在
    // dispatch 之前由 capture pipeline 物化到 blob 仓库,outbound 通过 V3BlobRef
    // 通道引用,因此 envelope 编码阶段 LocalFile 不应出现。这里用 expect_inline_bytes
    // 让契约违反时立即 panic,而不是默默写空字节。
    let reps = snapshot
        .representations
        .iter()
        .map(|rep| BinaryRepresentation {
            format_id: rep.format_id.as_ref().to_string(),
            mime: rep.mime.as_ref().map(|m| m.as_str().to_string()),
            data: rep.expect_inline_bytes().to_vec(),
        })
        .collect();

    let payload = ClipboardBinaryPayload {
        ts_ms: snapshot.ts_ms,
        representations: reps,
    };

    let bytes = payload
        .encode_to_vec()
        .map_err(|e| anyhow!("encode V3 envelope: {e}"))?;
    let snapshot_hash = snapshot.snapshot_hash().to_string();

    Ok((Bytes::from(bytes), snapshot_hash))
}

/// 编码 V3 envelope,并在尾部追加可选 blob 引用扩展。
///
/// 旧 decoder 只读取 `ClipboardBinaryPayload` 本体,不会触碰尾部字节,因此
/// 这个扩展不需要 bump payload version。新 decoder 通过固定 magic 识别尾部。
pub fn encode_snapshot_with_blob_refs_to_v3_bytes(
    snapshot: &SystemClipboardSnapshot,
    blob_refs: &[V3BlobRef],
) -> Result<(Bytes, String)> {
    let (bytes, snapshot_hash) = encode_snapshot_to_v3_bytes(snapshot)?;
    if blob_refs.is_empty() {
        return Ok((bytes, snapshot_hash));
    }

    let mut out = bytes.to_vec();
    write_blob_refs_extension(&mut out, blob_refs)
        .map_err(|e| anyhow!("encode V3 blob refs extension: {e}"))?;
    Ok((Bytes::from(out), snapshot_hash))
}

/// Decode V3 envelope bytes back into a `SystemClipboardSnapshot`.
///
/// Each `BinaryRepresentation` becomes an
/// `ObservedClipboardRepresentation` with a **freshly minted**
/// `RepresentationId`. Representation identity is local per-device —
/// the sender's ID has no meaning on the receiver, and a fresh ID keeps
/// the receiver's `ClipboardEvent` + `clipboard_snapshot_representation`
/// rows unique across re-syncs.
///
/// Caller should pass `notice.plaintext.as_ref()` from the inbound
/// notice; the decoder does not claim ownership of the buffer.
pub fn decode_v3_bytes_to_snapshot(bytes: &[u8]) -> Result<SystemClipboardSnapshot> {
    decode_v3_bytes_to_snapshot_and_blob_refs(bytes).map(|(snapshot, _)| snapshot)
}

/// 解码 V3 envelope,同时读取可选 blob 引用尾部扩展。
pub fn decode_v3_bytes_to_snapshot_and_blob_refs(
    bytes: &[u8],
) -> Result<(SystemClipboardSnapshot, Vec<V3BlobRef>)> {
    let mut cursor = bytes;
    let payload = ClipboardBinaryPayload::decode_from(&mut cursor)
        .map_err(|e| anyhow!("decode V3 envelope: {e}"))?;

    let representations = payload
        .representations
        .into_iter()
        .map(|rep| {
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from(rep.format_id),
                // Normalize wire mime: drop UTI / platform-native identifiers
                // shipped by older peers. Downstream classification falls
                // back to `format_id` when mime is `None`.
                normalize_wire_mime(rep.mime),
                rep.data,
            )
        })
        .collect();

    let blob_refs = read_blob_refs_extension(cursor)?;
    Ok((
        SystemClipboardSnapshot {
            ts_ms: payload.ts_ms,
            representations,
        },
        blob_refs,
    ))
}

fn write_blob_refs_extension<W: Write>(
    writer: &mut W,
    blob_refs: &[V3BlobRef],
) -> std::io::Result<()> {
    if blob_refs.len() > MAX_BLOB_REFS {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "blob ref count {} exceeds maximum {}",
                blob_refs.len(),
                MAX_BLOB_REFS
            ),
        ));
    }

    writer.write_all(BLOB_REFS_MAGIC)?;
    writer.write_all(&(blob_refs.len() as u16).to_le_bytes())?;
    for blob_ref in blob_refs {
        write_bytes_u32(writer, blob_ref.ticket.as_bytes(), "ticket")?;
        write_string_u16(writer, blob_ref.entry_id.as_ref(), "entry_id")?;
        write_optional_string_u16(writer, blob_ref.filename.as_deref(), "filename")?;
        write_optional_string_u16(writer, blob_ref.mime.as_deref(), "mime")?;
        writer.write_all(&blob_ref.size_bytes.to_le_bytes())?;
        write_optional_u32(writer, blob_ref.representation_index)?;
    }
    Ok(())
}

fn read_blob_refs_extension(mut bytes: &[u8]) -> Result<Vec<V3BlobRef>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let mut magic = [0u8; 4];
    bytes
        .read_exact(&mut magic)
        .map_err(|e| anyhow!("read V3 blob refs magic: {e}"))?;
    if &magic != BLOB_REFS_MAGIC {
        return Err(anyhow!("unknown V3 trailing extension"));
    }

    let count = read_u16(&mut bytes, "blob_ref_count")? as usize;
    if count > MAX_BLOB_REFS {
        return Err(anyhow!(
            "blob_ref_count {count} exceeds maximum {MAX_BLOB_REFS}"
        ));
    }

    let mut refs = Vec::with_capacity(count);
    for _ in 0..count {
        let ticket = BlobTicket::from_bytes(read_bytes_u32(&mut bytes, "ticket")?);
        let entry_id = EntryId::from_string(read_string_u16(&mut bytes, "entry_id")?);
        let filename = read_optional_string_u16(&mut bytes, "filename")?;
        let mime = read_optional_string_u16(&mut bytes, "mime")?;
        let size_bytes = read_u64(&mut bytes, "size_bytes")?;
        let representation_index = read_optional_u32(&mut bytes, "representation_index")?;
        refs.push(V3BlobRef {
            ticket,
            entry_id,
            filename,
            mime,
            size_bytes,
            representation_index,
        });
    }

    if !bytes.is_empty() {
        return Err(anyhow!(
            "V3 blob refs extension has {} trailing byte(s)",
            bytes.len()
        ));
    }
    Ok(refs)
}

fn write_bytes_u32<W: Write>(writer: &mut W, value: &[u8], label: &str) -> std::io::Result<()> {
    if value.len() > MAX_TICKET_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{label} length {} exceeds maximum {MAX_TICKET_LEN}",
                value.len()
            ),
        ));
    }
    let len = u32::try_from(value.len()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{label} length {} cannot fit u32", value.len()),
        )
    })?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(value)
}

fn write_string_u16<W: Write>(writer: &mut W, value: &str, label: &str) -> std::io::Result<()> {
    let bytes = value.as_bytes();
    if bytes.len() >= NONE_STRING_LEN as usize || bytes.len() > MAX_BLOB_REF_STRING_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{label} length {} exceeds maximum {}",
                bytes.len(),
                MAX_BLOB_REF_STRING_LEN
            ),
        ));
    }
    let len = u16::try_from(bytes.len()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{label} length {} cannot fit u16", bytes.len()),
        )
    })?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(bytes)
}

fn write_optional_string_u16<W: Write>(
    writer: &mut W,
    value: Option<&str>,
    label: &str,
) -> std::io::Result<()> {
    match value {
        Some(value) => write_string_u16(writer, value, label),
        None => writer.write_all(&NONE_STRING_LEN.to_le_bytes()),
    }
}

fn write_optional_u32<W: Write>(writer: &mut W, value: Option<u32>) -> std::io::Result<()> {
    match value {
        Some(v) => {
            writer.write_all(&[SOME_U32_MARKER])?;
            writer.write_all(&v.to_le_bytes())
        }
        None => writer.write_all(&[NONE_U32_MARKER]),
    }
}

fn read_optional_u32<R: Read>(reader: &mut R, label: &str) -> Result<Option<u32>> {
    let mut marker = [0u8; 1];
    reader
        .read_exact(&mut marker)
        .map_err(|e| anyhow!("read {label} marker: {e}"))?;
    match marker[0] {
        NONE_U32_MARKER => Ok(None),
        SOME_U32_MARKER => Ok(Some(read_u32(reader, label)?)),
        other => Err(anyhow!("invalid {label} marker byte: {other}")),
    }
}

fn read_bytes_u32<R: Read>(reader: &mut R, label: &str) -> Result<Vec<u8>> {
    let len = read_u32(reader, label)? as usize;
    if len > MAX_TICKET_LEN {
        return Err(anyhow!(
            "{label} length {len} exceeds maximum {MAX_TICKET_LEN}"
        ));
    }
    let mut bytes = vec![0u8; len];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| anyhow!("read {label}: {e}"))?;
    Ok(bytes)
}

fn read_string_u16<R: Read>(reader: &mut R, label: &str) -> Result<String> {
    let len = read_u16(reader, label)? as usize;
    if len > MAX_BLOB_REF_STRING_LEN {
        return Err(anyhow!(
            "{label} length {len} exceeds maximum {MAX_BLOB_REF_STRING_LEN}"
        ));
    }
    let mut bytes = vec![0u8; len];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| anyhow!("read {label}: {e}"))?;
    String::from_utf8(bytes).map_err(|e| anyhow!("invalid UTF-8 in {label}: {e}"))
}

fn read_optional_string_u16<R: Read>(reader: &mut R, label: &str) -> Result<Option<String>> {
    let len = read_u16(reader, label)?;
    if len == NONE_STRING_LEN {
        return Ok(None);
    }
    let len = len as usize;
    if len > MAX_BLOB_REF_STRING_LEN {
        return Err(anyhow!(
            "{label} length {len} exceeds maximum {MAX_BLOB_REF_STRING_LEN}"
        ));
    }
    let mut bytes = vec![0u8; len];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| anyhow!("read {label}: {e}"))?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| anyhow!("invalid UTF-8 in {label}: {e}"))
}

fn read_u16<R: Read>(reader: &mut R, label: &str) -> Result<u16> {
    let mut bytes = [0u8; 2];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| anyhow!("read {label}: {e}"))?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32<R: Read>(reader: &mut R, label: &str) -> Result<u32> {
    let mut bytes = [0u8; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| anyhow!("read {label}: {e}"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R, label: &str) -> Result<u64> {
    let mut bytes = [0u8; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| anyhow!("read {label}: {e}"))?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_snapshot(text: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_700_000_000_000,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                text.as_bytes().to_vec(),
            )],
        }
    }

    /// Verdict 1 — roundtrip: encoded + decoded snapshot carries the
    /// same ts_ms, representation count, format_id/mime/bytes. The
    /// `RepresentationId` intentionally differs (receiver-local).
    #[test]
    fn roundtrip_single_text_representation() {
        let original = fixture_snapshot("hello phase3");
        let (bytes, hash) = encode_snapshot_to_v3_bytes(&original).expect("encode should succeed");
        assert!(bytes.len() > 0);
        assert!(
            hash.starts_with("blake3v1:"),
            "snapshot_hash should be blake3v1:<hex>, got {hash}"
        );

        let decoded = decode_v3_bytes_to_snapshot(&bytes).expect("decode should succeed");
        assert_eq!(decoded.ts_ms, original.ts_ms);
        assert_eq!(decoded.representations.len(), 1);
        let rep = &decoded.representations[0];
        assert_eq!(rep.format_id.as_ref(), "text");
        assert_eq!(rep.mime.as_ref().map(|m| m.as_str()), Some("text/plain"));
        assert_eq!(rep.expect_inline_bytes(), b"hello phase3");
    }

    /// Verdict 2 — roundtrip preserves byte-for-byte equality for
    /// multiple representations + None-mime + non-ASCII bytes.
    #[test]
    fn roundtrip_multi_rep_with_binary_and_no_mime() {
        let original = SystemClipboardSnapshot {
            ts_ms: 42,
            representations: vec![
                ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("public.png"),
                    Some(MimeType("image/png".to_string())),
                    vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0x00],
                ),
                ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("custom-binary"),
                    None, // no mime — exercises the [1B] has_mime=0 branch
                    b"\x00\x01\x02\xDE\xAD\xBE\xEF".to_vec(),
                ),
            ],
        };
        let (bytes, _) = encode_snapshot_to_v3_bytes(&original).expect("encode should succeed");
        let decoded = decode_v3_bytes_to_snapshot(&bytes).expect("decode should succeed");

        assert_eq!(decoded.ts_ms, 42);
        assert_eq!(decoded.representations.len(), 2);
        assert_eq!(decoded.representations[0].format_id.as_ref(), "public.png");
        assert_eq!(
            decoded.representations[0].expect_inline_bytes(),
            vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0x00]
        );
        assert_eq!(
            decoded.representations[1].format_id.as_ref(),
            "custom-binary"
        );
        assert!(decoded.representations[1].mime.is_none());
    }

    /// Verdict 3 — snapshot_hash is deterministic: same snapshot bytes →
    /// same hash. Guards against future refactors accidentally making
    /// it order-dependent or including non-content fields.
    #[test]
    fn snapshot_hash_deterministic_across_encodes() {
        let snap_a = fixture_snapshot("same text");
        let snap_b = fixture_snapshot("same text");
        let (_, hash_a) = encode_snapshot_to_v3_bytes(&snap_a).unwrap();
        let (_, hash_b) = encode_snapshot_to_v3_bytes(&snap_b).unwrap();
        assert_eq!(hash_a, hash_b, "same content → same snapshot_hash");

        let snap_c = fixture_snapshot("different text");
        let (_, hash_c) = encode_snapshot_to_v3_bytes(&snap_c).unwrap();
        assert_ne!(hash_a, hash_c, "different content → different hash");
    }

    /// Verdict 4 — decode rejects truncated / corrupt bytes cleanly
    /// instead of panicking. The use case maps this to `DecodeFailed`
    /// and drops the inbound frame.
    #[test]
    fn decode_fails_on_truncated_bytes() {
        let original = fixture_snapshot("valid payload");
        let (bytes, _) = encode_snapshot_to_v3_bytes(&original).unwrap();
        // Truncate mid-stream — should fail in `read_exact`.
        let truncated = &bytes[..5.min(bytes.len())];
        let err =
            decode_v3_bytes_to_snapshot(truncated).expect_err("truncated payload must not decode");
        let msg = err.to_string();
        assert!(
            msg.contains("decode V3 envelope"),
            "error should mention V3 envelope context, got: {msg}"
        );
    }

    /// Verdict 5 —— 带 blob 扩展的 V3 payload 仍能被旧 decoder 当普通
    /// snapshot 读出;新 decoder 能额外取回 ticket + entry_id + 文件元数据。
    #[test]
    fn blob_refs_extension_is_backward_compatible_trailer() {
        let original = fixture_snapshot("file placeholder");
        let entry_id = EntryId::from("entry-1");
        let blob_ref = V3BlobRef {
            ticket: BlobTicket::from_bytes(vec![1, 2, 3, 4, 5]),
            entry_id: entry_id.clone(),
            filename: Some("report.pdf".to_string()),
            mime: Some("application/pdf".to_string()),
            size_bytes: 12_345,
            representation_index: None,
        };

        let (bytes, hash) =
            encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref.clone()])
                .expect("encode with blob refs should succeed");
        assert!(hash.starts_with("blake3v1:"));

        let legacy_decoded =
            decode_v3_bytes_to_snapshot(&bytes).expect("legacy snapshot decode should succeed");
        assert_eq!(
            legacy_decoded.representations[0].expect_inline_bytes(),
            b"file placeholder"
        );

        let (decoded, refs) = decode_v3_bytes_to_snapshot_and_blob_refs(&bytes)
            .expect("decode with blob refs should succeed");
        assert_eq!(
            decoded.representations[0].expect_inline_bytes(),
            b"file placeholder"
        );
        assert_eq!(refs, vec![blob_ref]);
        assert_eq!(refs[0].entry_id, entry_id);
    }

    /// Verdict 5b —— representation_index 字段端到端 roundtrip。新 sender
    /// 把 image rep 的 bytes 替换为 blob ref 时携带 `representation_index`,
    /// 接收端必须无损读到。
    #[test]
    fn blob_refs_with_representation_index_roundtrip() {
        let original = fixture_snapshot("placeholder for image rep");
        let entry_id = EntryId::from("entry-img");
        let blob_ref = V3BlobRef {
            ticket: BlobTicket::from_bytes(vec![9, 8, 7]),
            entry_id: entry_id.clone(),
            filename: None,
            mime: Some("image/png".to_string()),
            size_bytes: 3_500_000,
            representation_index: Some(0),
        };

        let (bytes, _) = encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref.clone()])
            .expect("encode with representation_index should succeed");

        let (_, refs) = decode_v3_bytes_to_snapshot_and_blob_refs(&bytes).expect("decode ok");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], blob_ref);
        assert_eq!(refs[0].representation_index, Some(0));
    }

    /// Verdict 6 —— 不带扩展时新 decoder 返回空 blob 引用,保持普通文本
    /// 路径的行为不变。
    #[test]
    fn blob_refs_decoder_returns_empty_for_plain_v3_payload() {
        let original = fixture_snapshot("plain text");
        let (bytes, _) = encode_snapshot_to_v3_bytes(&original).unwrap();
        let (decoded, refs) = decode_v3_bytes_to_snapshot_and_blob_refs(&bytes).unwrap();

        assert_eq!(
            decoded.representations[0].expect_inline_bytes(),
            b"plain text"
        );
        assert!(refs.is_empty());
    }

    /// Verdict 7 —— 新 decoder 遇到未知尾部扩展要明确报错,避免悄悄丢失
    /// 后续版本的关键数据。
    #[test]
    fn blob_refs_decoder_rejects_unknown_trailer() {
        let original = fixture_snapshot("plain text");
        let (bytes, _) = encode_snapshot_to_v3_bytes(&original).unwrap();
        let mut bytes = bytes.to_vec();
        bytes.extend_from_slice(b"NOPE");

        let err = decode_v3_bytes_to_snapshot_and_blob_refs(&bytes)
            .expect_err("unknown trailer should fail");
        assert!(err.to_string().contains("unknown V3 trailing extension"));
    }
}
