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
        let rep_count = u16::try_from(self.representations.len()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "representation count {} exceeds u16::MAX",
                    self.representations.len()
                ),
            )
        })?;
        writer.write_all(&rep_count.to_le_bytes())?;

        for rep in &self.representations {
            // [2B] format_id_len + [NB] format_id
            let format_id_bytes = rep.format_id.as_bytes();
            let format_id_len = u16::try_from(format_id_bytes.len()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "format_id length {} exceeds u16::MAX",
                        format_id_bytes.len()
                    ),
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
                    let mime_len = u16::try_from(mime_bytes.len()).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!("mime length {} exceeds u16::MAX", mime_bytes.len()),
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
            let data_len = u32::try_from(rep.data.len()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("data length {} exceeds u32::MAX", rep.data.len()),
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

        let mut representations = Vec::with_capacity(rep_count);

        for _ in 0..rep_count {
            // [2B] format_id_len + [NB] format_id
            let mut fid_len_buf = [0u8; 2];
            reader.read_exact(&mut fid_len_buf)?;
            let format_id_len = u16::from_le_bytes(fid_len_buf) as usize;
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
            let mime = if has_mime_buf[0] == 1 {
                // [2B] mime_len + [NB] mime
                let mut mime_len_buf = [0u8; 2];
                reader.read_exact(&mut mime_len_buf)?;
                let mime_len = u16::from_le_bytes(mime_len_buf) as usize;
                let mut mime_bytes = vec![0u8; mime_len];
                reader.read_exact(&mut mime_bytes)?;
                let mime_str = String::from_utf8(mime_bytes).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid UTF-8 in mime: {e}"),
                    )
                })?;
                Some(mime_str)
            } else {
                None
            };

            // [4B] data_len + [NB] data
            let mut data_len_buf = [0u8; 4];
            reader.read_exact(&mut data_len_buf)?;
            let data_len = u32::from_le_bytes(data_len_buf) as usize;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_payload(reps: Vec<BinaryRepresentation>) -> ClipboardBinaryPayload {
        ClipboardBinaryPayload {
            ts_ms: 1_713_000_000_000,
            representations: reps,
        }
    }

    fn round_trip(payload: &ClipboardBinaryPayload) -> ClipboardBinaryPayload {
        let encoded = payload.encode_to_vec().expect("encode failed");
        ClipboardBinaryPayload::decode_from(&mut Cursor::new(encoded)).expect("decode failed")
    }

    #[test]
    fn round_trip_single_rep() {
        let payload = make_payload(vec![BinaryRepresentation {
            format_id: "public.utf8-plain-text".to_string(),
            mime: Some("text/plain".to_string()),
            data: b"hello world".to_vec(),
        }]);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn round_trip_multi_rep() {
        let payload = make_payload(vec![
            BinaryRepresentation {
                format_id: "public.utf8-plain-text".to_string(),
                mime: Some("text/plain".to_string()),
                data: b"hello world".to_vec(),
            },
            BinaryRepresentation {
                format_id: "public.png".to_string(),
                mime: Some("image/png".to_string()),
                data: vec![0x89, 0x50, 0x4E, 0x47],
            },
            BinaryRepresentation {
                format_id: "public.html".to_string(),
                mime: Some("text/html".to_string()),
                data: b"<b>bold</b>".to_vec(),
            },
        ]);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn round_trip_empty_reps() {
        let payload = make_payload(vec![]);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn round_trip_large_data_10mb() {
        let large_data = vec![0xABu8; 10 * 1024 * 1024]; // 10MB
        let payload = make_payload(vec![BinaryRepresentation {
            format_id: "public.data".to_string(),
            mime: Some("application/octet-stream".to_string()),
            data: large_data,
        }]);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn round_trip_optional_mime_present() {
        let payload = make_payload(vec![BinaryRepresentation {
            format_id: "text".to_string(),
            mime: Some("text/plain".to_string()),
            data: b"data".to_vec(),
        }]);
        let encoded = payload.encode_to_vec().unwrap();
        // has_mime byte should be 1 at offset: 8 (ts) + 2 (rep_count) + 2 (fid_len) + 4 ("text") = 16
        assert_eq!(encoded[16], 1);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn round_trip_optional_mime_absent() {
        let payload = make_payload(vec![BinaryRepresentation {
            format_id: "text".to_string(),
            mime: None,
            data: b"data".to_vec(),
        }]);
        let encoded = payload.encode_to_vec().unwrap();
        // has_mime byte should be 0 at offset 16
        assert_eq!(encoded[16], 0);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn round_trip_utf8_format_id() {
        let payload = make_payload(vec![BinaryRepresentation {
            format_id: "com.example.日本語テスト".to_string(),
            mime: Some("text/plain; charset=utf-8".to_string()),
            data: b"unicode format_id".to_vec(),
        }]);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn round_trip_empty_data() {
        let payload = make_payload(vec![BinaryRepresentation {
            format_id: "empty".to_string(),
            mime: None,
            data: vec![],
        }]);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn round_trip_many_reps() {
        let reps: Vec<BinaryRepresentation> = (0..150)
            .map(|i| BinaryRepresentation {
                format_id: format!("format_{i}"),
                mime: if i % 2 == 0 {
                    Some(format!("type/{i}"))
                } else {
                    None
                },
                data: vec![i as u8; (i % 256) as usize],
            })
            .collect();
        let payload = make_payload(reps);
        assert_eq!(round_trip(&payload), payload);
    }

    #[test]
    fn encode_deterministic() {
        let payload = make_payload(vec![BinaryRepresentation {
            format_id: "test".to_string(),
            mime: Some("text/plain".to_string()),
            data: b"deterministic".to_vec(),
        }]);
        let encoded1 = payload.encode_to_vec().unwrap();
        let encoded2 = payload.encode_to_vec().unwrap();
        assert_eq!(encoded1, encoded2, "encoding must be deterministic");
    }

    #[test]
    fn ts_ms_preserved() {
        let payload = ClipboardBinaryPayload {
            ts_ms: -12345,
            representations: vec![],
        };
        let decoded = round_trip(&payload);
        assert_eq!(decoded.ts_ms, -12345);
    }
}
