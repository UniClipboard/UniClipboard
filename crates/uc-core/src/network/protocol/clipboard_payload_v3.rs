//! V3 binary payload codec for clipboard multi-representation transfer.
//!
//! Replaces V2's JSON+base64 encoding with a pure binary format using
//! `std::io::Read/Write` and manual `to_le_bytes/from_le_bytes`.
//!
//! # Binary Layout (before compression)
//! ```text
//! [8B]  ts_ms (i64 LE)
//! [2B]  rep_count (u16 LE)
//! For each representation:
//!   [2B]  format_id_len (u16 LE)
//!   [NB]  format_id (UTF-8)
//!   [1B]  has_mime (0 or 1)
//!   if has_mime == 1:
//!     [2B]  mime_len (u16 LE)
//!     [NB]  mime (UTF-8)
//!   [4B]  data_len (u32 LE)
//!   [NB]  data (raw bytes)
//! ```
//!
//! No serde dependency — pure `std::io` for zero-overhead encoding.

use std::io::{Read, Write};

// Decode-side safety limits to prevent OOM from malformed/malicious input.
/// Maximum number of representations in a single payload.
const MAX_REPRESENTATIONS: usize = 1_024;
/// Maximum length of a format_id string in bytes.
const MAX_FORMAT_ID_LEN: usize = 1_024;
/// Maximum length of a MIME type string in bytes.
const MAX_MIME_LEN: usize = 1_024;
/// Maximum length of a single representation's data in bytes (256 MiB).
const MAX_DATA_LEN: usize = 256 * 1024 * 1024;

/// A single clipboard representation in binary wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryRepresentation {
    /// Platform format identifier (e.g., "public.png", "text/html").
    pub format_id: String,
    /// MIME type string, if known.
    pub mime: Option<String>,
    /// Raw bytes of this representation.
    pub data: Vec<u8>,
}

/// V3 binary clipboard payload containing all representations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardBinaryPayload {
    /// Timestamp in milliseconds since Unix epoch.
    pub ts_ms: i64,
    /// All clipboard representations bundled together.
    pub representations: Vec<BinaryRepresentation>,
}

impl ClipboardBinaryPayload {
    /// Encode this payload into binary format, writing to `writer`.
    pub fn encode_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // [8B] ts_ms
        writer.write_all(&self.ts_ms.to_le_bytes())?;

        // [2B] rep_count
        if self.representations.len() > MAX_REPRESENTATIONS {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "representation count {} exceeds maximum {}",
                    self.representations.len(),
                    MAX_REPRESENTATIONS
                ),
            ));
        }
        let rep_count = u16::try_from(self.representations.len()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "representation count {} cannot fit u16",
                    self.representations.len()
                ),
            )
        })?;
        writer.write_all(&rep_count.to_le_bytes())?;

        for rep in &self.representations {
            // [2B] format_id_len + [NB] format_id
            let format_id_bytes = rep.format_id.as_bytes();
            if format_id_bytes.len() > MAX_FORMAT_ID_LEN {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "format_id length {} exceeds maximum {}",
                        format_id_bytes.len(),
                        MAX_FORMAT_ID_LEN
                    ),
                ));
            }
            let format_id_len = u16::try_from(format_id_bytes.len()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("format_id length {} cannot fit u16", format_id_bytes.len()),
                )
            })?;
            writer.write_all(&format_id_len.to_le_bytes())?;
            writer.write_all(format_id_bytes)?;

            // [1B] has_mime
            match &rep.mime {
                Some(mime) => {
                    writer.write_all(&[1u8])?;
                    // [2B] mime_len + [NB] mime
                    let mime_bytes = mime.as_bytes();
                    if mime_bytes.len() > MAX_MIME_LEN {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!(
                                "mime length {} exceeds maximum {}",
                                mime_bytes.len(),
                                MAX_MIME_LEN
                            ),
                        ));
                    }
                    let mime_len = u16::try_from(mime_bytes.len()).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!("mime length {} cannot fit u16", mime_bytes.len()),
                        )
                    })?;
                    writer.write_all(&mime_len.to_le_bytes())?;
                    writer.write_all(mime_bytes)?;
                }
                None => {
                    writer.write_all(&[0u8])?;
                }
            }

            // [4B] data_len + [NB] data
            if rep.data.len() > MAX_DATA_LEN {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "data length {} exceeds maximum {}",
                        rep.data.len(),
                        MAX_DATA_LEN
                    ),
                ));
            }
            let data_len = u32::try_from(rep.data.len()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("data length {} cannot fit u32", rep.data.len()),
                )
            })?;
            writer.write_all(&data_len.to_le_bytes())?;
            writer.write_all(&rep.data)?;
        }

        Ok(())
    }

    /// Convenience method: encode to a new `Vec<u8>`.
    pub fn encode_to_vec(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.encode_to(&mut buf)?;
        Ok(buf)
    }

    /// Decode a binary payload from `reader`.
    pub fn decode_from<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        // [8B] ts_ms
        let mut ts_buf = [0u8; 8];
        reader.read_exact(&mut ts_buf)?;
        let ts_ms = i64::from_le_bytes(ts_buf);

        // [2B] rep_count
        let mut rep_count_buf = [0u8; 2];
        reader.read_exact(&mut rep_count_buf)?;
        let rep_count = u16::from_le_bytes(rep_count_buf) as usize;

        if rep_count > MAX_REPRESENTATIONS {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("rep_count {rep_count} exceeds maximum {MAX_REPRESENTATIONS}"),
            ));
        }

        let mut representations = Vec::with_capacity(rep_count);

        for _ in 0..rep_count {
            // [2B] format_id_len + [NB] format_id
            let mut fid_len_buf = [0u8; 2];
            reader.read_exact(&mut fid_len_buf)?;
            let format_id_len = u16::from_le_bytes(fid_len_buf) as usize;
            if format_id_len > MAX_FORMAT_ID_LEN {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("format_id_len {format_id_len} exceeds maximum {MAX_FORMAT_ID_LEN}"),
                ));
            }
            let mut format_id_bytes = vec![0u8; format_id_len];
            reader.read_exact(&mut format_id_bytes)?;
            let format_id = String::from_utf8(format_id_bytes).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid UTF-8 in format_id: {e}"),
                )
            })?;

            // [1B] has_mime
            let mut has_mime_buf = [0u8; 1];
            reader.read_exact(&mut has_mime_buf)?;
            let mime = match has_mime_buf[0] {
                1 => {
                    // [2B] mime_len + [NB] mime
                    let mut mime_len_buf = [0u8; 2];
                    reader.read_exact(&mut mime_len_buf)?;
                    let mime_len = u16::from_le_bytes(mime_len_buf) as usize;
                    if mime_len > MAX_MIME_LEN {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("mime_len {mime_len} exceeds maximum {MAX_MIME_LEN}"),
                        ));
                    }
                    let mut mime_bytes = vec![0u8; mime_len];
                    reader.read_exact(&mut mime_bytes)?;
                    let mime_str = String::from_utf8(mime_bytes).map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("invalid UTF-8 in mime: {e}"),
                        )
                    })?;
                    Some(mime_str)
                }
                0 => None,
                other => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid has_mime flag: expected 0 or 1, got {other}"),
                    ));
                }
            };

            // [4B] data_len + [NB] data
            let mut data_len_buf = [0u8; 4];
            reader.read_exact(&mut data_len_buf)?;
            let data_len = u32::from_le_bytes(data_len_buf) as usize;
            if data_len > MAX_DATA_LEN {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("data_len {data_len} exceeds maximum {MAX_DATA_LEN}"),
                ));
            }
            let mut data = vec![0u8; data_len];
            reader.read_exact(&mut data)?;

            representations.push(BinaryRepresentation {
                format_id,
                mime,
                data,
            });
        }

        Ok(Self {
            ts_ms,
            representations,
        })
    }
}
