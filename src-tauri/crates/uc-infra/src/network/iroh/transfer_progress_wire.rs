//! Wire codec for the **outbound transfer progress** protocol.
//!
//! ## 业务用途
//!
//! 接收端 fetch blob 时,沿反向 P2P 通道把字节级进度推送回数据来源端
//! (sender)。`uc-application` 的 fetch sink 旁路调用
//! [`OutboundProgressReporterPort`](uc_core::file_transfer::OutboundProgressReporterPort),
//! 适配器把每次调用打成一帧,通过专用 ALPN 单向流写到 sender。
//!
//! ## Frame layout
//!
//! ```text
//! receiver -> sender (one uni-stream, one direction):
//!   [magic(1) | transfer_id(16) | bytes(8) | total(8) | status(1) | FIN]
//! ```
//!
//! * `magic` = [`PROGRESS_MAGIC`] — 与 `clipboard_wire` 的 0xC1 区分,
//!   一旦在错配 ALPN 上收到错误字节,handler 立刻拒绝,不让脏数据进入
//!   后续解析。
//! * `transfer_id` = sender 的 `EntryId` 的 UUID 字节(16 bytes,big-
//!   endian 一致编码)。`EntryId` 内部是 v4 UUID,转字符串就是 sender
//!   端的 entry_id,sender 收到后从 16 bytes 重建 UUID 字符串去查本地
//!   entry。这避免在 wire 上塞变长字符串。
//! * `bytes_transferred` / `total_bytes` 都是 `u64` 大端。`total_bytes
//!   == 0` 约定为"未知"(对应 [`OutboundProgressStatus::InProgress`] 时
//!   adapter 未拿到总大小的情况),sender 端 UI 渲染成 indeterminate。
//! * `status`:
//!   - `0x01` InProgress
//!   - `0x02` Completed
//!   - `0x03` Failed
//!   - `0x04` Cancelled(LocalUser)
//!   - `0x05` Cancelled(RemotePeer)
//!   - `0x06` Cancelled(Replaced)
//!   - `0x07` Cancelled(Timeout)
//!   - `0x08` Cancelled(Unknown)
//!
//! 取消子原因占用独立 status 字节,而不是单独引入 reason 字段,以保持
//! 帧定长。老 sender 收到 `0x04..0x08` 会返回 `UnknownStatus` 拒绝单帧,
//! 但 accept loop 会接受下一帧,不影响连续 progress。
//!
//! ## 为什么用裸字节而不是 postcard
//!
//! 帧固定 34 字节,比 postcard 头本身还要短。`u128`/`u64` 的大端编码
//! 是最便宜也最容易在 receiver 重建的形式;serde_with 提供 `As<Bytes>`
//! 也行,但会把简单代码间接化。这层不做版本协商(ALPN 已经表达版
//! 本),将来要扩字段就跳 ALPN 到 `/1`。

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use uc_core::file_transfer::{FileTransferCancellationReason, OutboundProgressStatus};

/// Sentinel byte distinguishing transfer-progress frames from any other
/// stream the receiver might mis-route here. Must NOT collide with
/// `clipboard_wire::CLIPBOARD_MAGIC` (0xC1).
pub const PROGRESS_MAGIC: u8 = 0xC2;

/// Hard-coded fixed frame size (after the magic byte).
const PAYLOAD_LEN: usize = 16 /* transfer_id */ + 8 /* bytes */ + 8 /* total */ + 1 /* status */;

/// Total wire bytes including magic — used to size accept-side read buffers.
pub const FRAME_LEN: usize = 1 + PAYLOAD_LEN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressFrame {
    /// 16-byte UUID:sender's `EntryId` 的字节形式。
    pub transfer_id_bytes: [u8; 16],
    pub bytes_transferred: u64,
    /// `None` 时 wire 上写 0,decode 还原为 `None`。
    pub total_bytes: Option<u64>,
    pub status: OutboundProgressStatus,
}

impl ProgressFrame {
    fn status_byte(&self) -> u8 {
        match self.status {
            OutboundProgressStatus::InProgress => 0x01,
            OutboundProgressStatus::Completed => 0x02,
            OutboundProgressStatus::Failed => 0x03,
            OutboundProgressStatus::Cancelled { reason } => match reason {
                FileTransferCancellationReason::LocalUser => 0x04,
                FileTransferCancellationReason::RemotePeer => 0x05,
                FileTransferCancellationReason::Replaced => 0x06,
                FileTransferCancellationReason::Timeout => 0x07,
                FileTransferCancellationReason::Unknown => 0x08,
            },
        }
    }
}

#[derive(Debug, Error)]
pub enum ProgressWireError {
    #[error("stream io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad magic: got 0x{got:02X} (expected 0x{expected:02X})")]
    BadMagic { got: u8, expected: u8 },
    #[error("unknown status byte: 0x{0:02X}")]
    UnknownStatus(u8),
}

/// Encode + write one progress frame. Caller is responsible for closing
/// the send half (`finish()` on iroh `SendStream`) so the peer's read sees
/// EOF after the frame.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    send: &mut W,
    frame: &ProgressFrame,
) -> Result<(), ProgressWireError> {
    let mut buf = [0u8; FRAME_LEN];
    buf[0] = PROGRESS_MAGIC;
    buf[1..17].copy_from_slice(&frame.transfer_id_bytes);
    buf[17..25].copy_from_slice(&frame.bytes_transferred.to_be_bytes());
    let total_wire = frame.total_bytes.unwrap_or(0);
    buf[25..33].copy_from_slice(&total_wire.to_be_bytes());
    buf[33] = frame.status_byte();
    send.write_all(&buf).await?;
    Ok(())
}

/// Read exactly one frame.
pub async fn read_frame<R: AsyncRead + Unpin>(
    recv: &mut R,
) -> Result<ProgressFrame, ProgressWireError> {
    let mut buf = [0u8; FRAME_LEN];
    recv.read_exact(&mut buf).await?;
    decode(&buf)
}

fn decode(buf: &[u8; FRAME_LEN]) -> Result<ProgressFrame, ProgressWireError> {
    if buf[0] != PROGRESS_MAGIC {
        return Err(ProgressWireError::BadMagic {
            got: buf[0],
            expected: PROGRESS_MAGIC,
        });
    }
    let mut transfer_id_bytes = [0u8; 16];
    transfer_id_bytes.copy_from_slice(&buf[1..17]);
    let mut b8 = [0u8; 8];
    b8.copy_from_slice(&buf[17..25]);
    let bytes_transferred = u64::from_be_bytes(b8);
    b8.copy_from_slice(&buf[25..33]);
    let total_wire = u64::from_be_bytes(b8);
    let total_bytes = if total_wire == 0 {
        None
    } else {
        Some(total_wire)
    };
    let status = match buf[33] {
        0x01 => OutboundProgressStatus::InProgress,
        0x02 => OutboundProgressStatus::Completed,
        0x03 => OutboundProgressStatus::Failed,
        0x04 => OutboundProgressStatus::Cancelled {
            reason: FileTransferCancellationReason::LocalUser,
        },
        0x05 => OutboundProgressStatus::Cancelled {
            reason: FileTransferCancellationReason::RemotePeer,
        },
        0x06 => OutboundProgressStatus::Cancelled {
            reason: FileTransferCancellationReason::Replaced,
        },
        0x07 => OutboundProgressStatus::Cancelled {
            reason: FileTransferCancellationReason::Timeout,
        },
        0x08 => OutboundProgressStatus::Cancelled {
            reason: FileTransferCancellationReason::Unknown,
        },
        other => return Err(ProgressWireError::UnknownStatus(other)),
    };
    Ok(ProgressFrame {
        transfer_id_bytes,
        bytes_transferred,
        total_bytes,
        status,
    })
}

/// 从 `EntryId.as_str()`(UUID v4 形式)解析出 16 bytes。失败说明上层
/// 给了一个非 UUID 字符串,通常是构造错误。
pub fn transfer_id_to_bytes(transfer_id: &str) -> Option<[u8; 16]> {
    uuid::Uuid::parse_str(transfer_id)
        .ok()
        .map(|u| *u.as_bytes())
}

/// 反向:从 16 bytes 重建 UUID 字符串(短横线形式),用于 sender 端
/// 查找本地 entry。
pub fn transfer_id_from_bytes(bytes: &[u8; 16]) -> String {
    uuid::Uuid::from_bytes(*bytes).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn sample_uuid_bytes() -> [u8; 16] {
        // Fixed UUID for round-trip predictability.
        *uuid::Uuid::parse_str("11111111-2222-4333-8444-555555555555")
            .unwrap()
            .as_bytes()
    }

    #[tokio::test]
    async fn frame_round_trip_in_progress() {
        let frame = ProgressFrame {
            transfer_id_bytes: sample_uuid_bytes(),
            bytes_transferred: 42 * 1024 * 1024,
            total_bytes: Some(100 * 1024 * 1024),
            status: OutboundProgressStatus::InProgress,
        };
        let (mut client, mut server) = duplex(64);
        let f = frame.clone();
        let task = tokio::spawn(async move {
            write_frame(&mut client, &f).await.expect("write");
            client.shutdown().await.expect("shutdown");
        });
        let recovered = read_frame(&mut server).await.expect("read");
        assert_eq!(recovered, frame);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn frame_round_trip_completed_with_unknown_total() {
        let frame = ProgressFrame {
            transfer_id_bytes: sample_uuid_bytes(),
            bytes_transferred: 1234,
            total_bytes: None,
            status: OutboundProgressStatus::Completed,
        };
        let (mut client, mut server) = duplex(64);
        let f = frame.clone();
        let task = tokio::spawn(async move {
            write_frame(&mut client, &f).await.expect("write");
            client.shutdown().await.expect("shutdown");
        });
        let recovered = read_frame(&mut server).await.expect("read");
        assert_eq!(recovered, frame);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn frame_round_trip_failed() {
        let frame = ProgressFrame {
            transfer_id_bytes: sample_uuid_bytes(),
            bytes_transferred: 0,
            total_bytes: None,
            status: OutboundProgressStatus::Failed,
        };
        let (mut client, mut server) = duplex(64);
        let f = frame.clone();
        let task = tokio::spawn(async move {
            write_frame(&mut client, &f).await.expect("write");
            client.shutdown().await.expect("shutdown");
        });
        let recovered = read_frame(&mut server).await.expect("read");
        assert_eq!(recovered, frame);
        task.await.unwrap();
    }

    /// 五个 cancel 子原因都走独立 status byte。这里一次性覆盖
    /// 0x04..0x08,避免 wire 字节与 [`FileTransferCancellationReason`]
    /// 的对齐发生漂移。
    #[tokio::test]
    async fn frame_round_trip_cancelled_all_reasons() {
        let reasons = [
            FileTransferCancellationReason::LocalUser,
            FileTransferCancellationReason::RemotePeer,
            FileTransferCancellationReason::Replaced,
            FileTransferCancellationReason::Timeout,
            FileTransferCancellationReason::Unknown,
        ];
        for reason in reasons {
            let frame = ProgressFrame {
                transfer_id_bytes: sample_uuid_bytes(),
                bytes_transferred: 123,
                total_bytes: Some(456),
                status: OutboundProgressStatus::Cancelled { reason },
            };
            let (mut client, mut server) = duplex(64);
            let f = frame.clone();
            let task = tokio::spawn(async move {
                write_frame(&mut client, &f).await.expect("write");
                client.shutdown().await.expect("shutdown");
            });
            let recovered = read_frame(&mut server).await.expect("read");
            assert_eq!(recovered, frame, "reason {reason:?} did not round-trip");
            task.await.unwrap();
        }
    }

    /// Wire 字节 0x04..0x08 必须与领域枚举 1:1 对齐。这个测试是显式
    /// 锁桩,防止有人改了 status_byte 映射但忘了改 decode(或反之)。
    #[test]
    fn cancel_status_bytes_pin() {
        let cases: &[(FileTransferCancellationReason, u8)] = &[
            (FileTransferCancellationReason::LocalUser, 0x04),
            (FileTransferCancellationReason::RemotePeer, 0x05),
            (FileTransferCancellationReason::Replaced, 0x06),
            (FileTransferCancellationReason::Timeout, 0x07),
            (FileTransferCancellationReason::Unknown, 0x08),
        ];
        for &(reason, expected) in cases {
            let frame = ProgressFrame {
                transfer_id_bytes: sample_uuid_bytes(),
                bytes_transferred: 0,
                total_bytes: None,
                status: OutboundProgressStatus::Cancelled { reason },
            };
            assert_eq!(
                frame.status_byte(),
                expected,
                "status byte for {reason:?} drifted"
            );
        }
    }

    #[tokio::test]
    async fn read_rejects_bad_magic() {
        let mut buf = [0u8; FRAME_LEN];
        buf[0] = 0x00; // wrong magic
        let (mut client, mut server) = duplex(64);
        let task = tokio::spawn(async move {
            client.write_all(&buf).await.expect("write");
            client.shutdown().await.expect("shutdown");
        });
        let err = read_frame(&mut server)
            .await
            .expect_err("bad magic must reject");
        match err {
            ProgressWireError::BadMagic { got, expected } => {
                assert_eq!(got, 0x00);
                assert_eq!(expected, PROGRESS_MAGIC);
            }
            other => panic!("expected BadMagic, got {other:?}"),
        }
        task.await.unwrap();
    }

    #[tokio::test]
    async fn read_rejects_unknown_status_byte() {
        let mut buf = [0u8; FRAME_LEN];
        buf[0] = PROGRESS_MAGIC;
        buf[1..17].copy_from_slice(&sample_uuid_bytes());
        // bytes / total all zero
        buf[33] = 0xAA; // unknown status
        let (mut client, mut server) = duplex(64);
        let task = tokio::spawn(async move {
            client.write_all(&buf).await.expect("write");
            client.shutdown().await.expect("shutdown");
        });
        let err = read_frame(&mut server)
            .await
            .expect_err("bad status must reject");
        assert!(matches!(err, ProgressWireError::UnknownStatus(0xAA)));
        task.await.unwrap();
    }

    #[test]
    fn transfer_id_round_trip() {
        let original = "11111111-2222-4333-8444-555555555555";
        let bytes = transfer_id_to_bytes(original).expect("valid uuid");
        let restored = transfer_id_from_bytes(&bytes);
        assert_eq!(restored, original);
    }

    #[test]
    fn transfer_id_rejects_non_uuid() {
        assert!(transfer_id_to_bytes("not-a-uuid").is_none());
    }
}
