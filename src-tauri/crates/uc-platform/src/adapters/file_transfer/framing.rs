//! Binary framing for file transfer messages.
//!
//! Each frame consists of:
//! - 1 byte: message type tag
//! - 4 bytes: payload length (big-endian u32)
//! - N bytes: payload data

use anyhow::{anyhow, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum frame size: 1MB data + 64KB metadata overhead.
pub const MAX_FILE_FRAME_BYTES: usize = 1024 * 1024 + 64 * 1024;

/// Message type tags for file transfer protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FileMessageType {
    Announce = 1,
    Accept = 2,
    Reject = 3,
    Chunk = 4,
    Complete = 5,
}

impl FileMessageType {
    pub fn from_byte(b: u8) -> Result<Self> {
        match b {
            1 => Ok(Self::Announce),
            2 => Ok(Self::Accept),
            3 => Ok(Self::Reject),
            4 => Ok(Self::Chunk),
            5 => Ok(Self::Complete),
            other => Err(anyhow!("unknown file message type: {}", other)),
        }
    }
}

/// Write a typed, length-prefixed frame to the writer.
///
/// Format: [1-byte type tag][4-byte big-endian length][payload]
pub async fn write_file_frame<W>(
    writer: &mut W,
    msg_type: FileMessageType,
    payload: &[u8],
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    if payload.len() > MAX_FILE_FRAME_BYTES {
        return Err(anyhow!(
            "file frame payload too large: {} > {}",
            payload.len(),
            MAX_FILE_FRAME_BYTES
        ));
    }

    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| anyhow!("frame too large for u32: {} bytes", payload.len()))?;

    writer.write_all(&[msg_type as u8]).await?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(payload).await?;
    Ok(())
}

/// Read a typed, length-prefixed frame from the reader.
///
/// Returns `Ok(None)` if the stream ends cleanly before the type byte.
pub async fn read_file_frame<R>(reader: &mut R) -> Result<Option<(FileMessageType, Vec<u8>)>>
where
    R: AsyncRead + Unpin,
{
    // Read type tag
    let mut type_buf = [0u8; 1];
    let n = reader.read(&mut type_buf).await?;
    if n == 0 {
        return Ok(None);
    }
    let msg_type = FileMessageType::from_byte(type_buf[0])?;

    // Read length
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_FILE_FRAME_BYTES {
        return Err(anyhow!(
            "file frame exceeds max: {} > {}",
            len,
            MAX_FILE_FRAME_BYTES
        ));
    }

    // Read payload
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;

    Ok(Some((msg_type, buf)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frame_roundtrip() {
        let payload = b"hello world";
        let mut buf = Vec::new();
        write_file_frame(&mut buf, FileMessageType::Announce, payload)
            .await
            .unwrap();

        let mut cursor = &buf[..];
        let result = read_file_frame(&mut cursor).await.unwrap().unwrap();
        assert_eq!(result.0, FileMessageType::Announce);
        assert_eq!(result.1, payload);
    }

    #[tokio::test]
    async fn all_message_types_roundtrip() {
        let types = [
            FileMessageType::Announce,
            FileMessageType::Accept,
            FileMessageType::Reject,
            FileMessageType::Chunk,
            FileMessageType::Complete,
        ];
        for msg_type in types {
            let payload = b"test";
            let mut buf = Vec::new();
            write_file_frame(&mut buf, msg_type, payload).await.unwrap();

            let mut cursor = &buf[..];
            let (decoded_type, decoded_payload) =
                read_file_frame(&mut cursor).await.unwrap().unwrap();
            assert_eq!(decoded_type, msg_type);
            assert_eq!(decoded_payload, payload);
        }
    }

    #[tokio::test]
    async fn empty_stream_returns_none() {
        let buf: &[u8] = &[];
        let mut cursor = buf;
        let result = read_file_frame(&mut cursor).await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn invalid_message_type_rejected() {
        assert!(FileMessageType::from_byte(0).is_err());
        assert!(FileMessageType::from_byte(6).is_err());
        assert!(FileMessageType::from_byte(255).is_err());
    }
}
