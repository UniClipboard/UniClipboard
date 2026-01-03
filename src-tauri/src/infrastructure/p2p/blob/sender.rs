//! BlobStream 发送器
//!
//! 负责将大数据分割成帧并发送。

use crate::infrastructure::p2p::blob::frame::{Frame, FrameHeader, FrameType, CHUNK_SIZE};
use anyhow::{anyhow, Result};
use log::{debug, info, trace};
use uuid::Uuid;

/// Blob 元数据
#[derive(Debug, Clone)]
pub struct BlobMetadata {
    /// 唯一标识符（UUID v4）
    pub id: String,
    /// 数据总大小（字节）
    pub size: u64,
    /// 分片大小（字节）
    pub chunk_size: u32,
    /// 分片总数
    pub chunk_count: u32,
    /// BLAKE3 哈希值（十六进制字符串）
    pub hash: String,
}

impl BlobMetadata {
    /// 创建新的元数据
    pub fn new(size: u64, chunk_size: u32, chunk_count: u32, hash: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            size,
            chunk_size,
            chunk_count,
            hash,
        }
    }
}

/// Blob 发送器
pub struct BlobSender {
    /// 元数据
    metadata: BlobMetadata,
    /// 当前分片索引
    current_chunk: u32,
    /// 原始数据
    data: Vec<u8>,
    /// 会话 ID
    session_id: u32,
    /// 总帧数
    total_frames: u32,
}

impl BlobSender {
    /// 创建新的发送器
    ///
    /// # Arguments
    ///
    /// * `data` - 要发送的数据
    /// * `session_id` - 会话 ID
    pub fn new(data: Vec<u8>, session_id: u32) -> Self {
        let size = data.len() as u64;
        let chunk_size = CHUNK_SIZE as u32;
        let chunk_count = ((size as f64) / (chunk_size as f64)).ceil() as u32;

        // 计算 BLAKE3 哈希
        let hash = blake3::hash(&data);
        let hash_hex = hex::encode(hash.as_bytes());

        // 总帧数 = 元数据帧(1) + 数据帧(N) + 完成帧(1)
        let total_frames = 1 + chunk_count + 1;

        let metadata = BlobMetadata::new(size, chunk_size, chunk_count, hash_hex);

        info!(
            "Created BlobSender: session_id={}, size={}, chunks={}, total_frames={}, hash={}",
            session_id, size, chunk_count, total_frames, metadata.hash
        );

        Self {
            metadata,
            current_chunk: 0,
            data,
            session_id,
            total_frames,
        }
    }

    /// 获取元数据
    pub fn metadata(&self) -> &BlobMetadata {
        &self.metadata
    }

    /// 获取会话 ID
    pub fn session_id(&self) -> u32 {
        self.session_id
    }

    /// 获取总帧数
    pub fn total_frames(&self) -> u32 {
        self.total_frames
    }

    /// 创建元数据帧
    pub fn make_metadata_frame(&self) -> Result<Frame> {
        debug!(
            "Creating metadata frame: session_id={}, total_frames={}, data_size={}",
            self.session_id, self.total_frames, self.metadata.size
        );

        // 从完整哈希中提取前 16 字节
        let hash_bytes = hex::decode(&self.metadata.hash)
            .map_err(|e| anyhow!("Failed to decode hash hex: {}", e))?;

        let mut hash_prefix = [0u8; 16];
        hash_prefix.copy_from_slice(&hash_bytes[..16]);

        let header = FrameHeader::metadata(
            self.session_id,
            self.total_frames,
            self.metadata.size,
            hash_prefix,
        );

        Ok(Frame::new(header, vec![]))
    }

    /// 获取下一帧（如果有）
    ///
    /// 返回 `None` 表示所有数据帧已发送完成
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        // 检查是否还有数据要发送
        if self.current_chunk >= self.metadata.chunk_count {
            trace!("All data chunks sent for session_id={}", self.session_id);
            return Ok(None);
        }

        let chunk_size = self.metadata.chunk_size as usize;
        let start = (self.current_chunk as usize) * chunk_size;
        let end = std::cmp::min(start + chunk_size, self.data.len());

        if start >= self.data.len() {
            return Err(anyhow!(
                "Invalid chunk index: {} (data size: {})",
                self.current_chunk,
                self.data.len()
            ));
        }

        let chunk_data = &self.data[start..end];
        let data_length = chunk_data.len() as u32;

        trace!(
            "Sending chunk {}/{}: start={}, end={}, size={}",
            self.current_chunk + 1,
            self.metadata.chunk_count,
            start,
            end,
            data_length
        );

        // 计算当前分片的 BLAKE3 哈希（用于完整性校验）
        let chunk_hash = blake3::hash(chunk_data);
        let mut hash_prefix = [0u8; 16];
        hash_prefix.copy_from_slice(&chunk_hash.as_bytes()[..16]);

        let header = FrameHeader::data(
            self.session_id,
            self.current_chunk + 1, // 序列号从 1 开始
            self.total_frames,
            data_length,
            hash_prefix,
        );

        self.current_chunk += 1;

        Ok(Some(Frame::new(header, chunk_data.to_vec())))
    }

    /// 创建完成帧
    pub fn make_complete_frame(&self) -> Frame {
        debug!(
            "Creating complete frame: session_id={}",
            self.session_id
        );

        let header = FrameHeader::complete(self.session_id);
        Frame::new(header, vec![])
    }

    /// 检查是否所有帧都已发送
    pub fn is_complete(&self) -> bool {
        self.current_chunk >= self.metadata.chunk_count
    }

    /// 获取发送进度
    pub fn progress(&self) -> f64 {
        if self.metadata.chunk_count == 0 {
            return 1.0;
        }
        (self.current_chunk as f64) / (self.metadata.chunk_count as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blob_sender_creation() {
        let data = vec![1u8, 2, 3, 4, 5];
        let sender = BlobSender::new(data, 123);

        assert_eq!(sender.session_id(), 123);
        assert_eq!(sender.metadata().size, 5);
        assert_eq!(sender.metadata().chunk_count, 1);
        assert_eq!(sender.total_frames(), 3); // 1 metadata + 1 data + 1 complete
    }

    #[test]
    fn test_large_data_chunking() {
        // 创建 100KB 数据（应该分成 4 个分片，每个 32KB）
        let data = vec![42u8; 102400];
        let mut sender = BlobSender::new(data, 456);

        assert_eq!(sender.metadata().chunk_count, 4);
        assert_eq!(sender.total_frames(), 6); // 1 metadata + 4 data + 1 complete

        // 发送所有数据帧
        let mut frames_sent = 0;
        while let Ok(Some(frame)) = sender.next_frame() {
            frames_sent += 1;
            assert_eq!(frame.frame_type(), Some(FrameType::Data));
        }

        assert_eq!(frames_sent, 4);
        assert!(sender.is_complete());
    }

    #[test]
    fn test_metadata_frame() {
        let data = vec![1u8, 2, 3];
        let sender = BlobSender::new(data, 789);

        let frame = sender.make_metadata_frame().unwrap();
        assert_eq!(frame.frame_type(), Some(FrameType::Metadata));
        assert_eq!(frame.session_id(), 789);
        assert_eq!(frame.header.total_frames, 3);
    }

    #[test]
    fn test_complete_frame() {
        let data = vec![1u8, 2, 3];
        let sender = BlobSender::new(data, 999);

        let frame = sender.make_complete_frame();
        assert_eq!(frame.frame_type(), Some(FrameType::Complete));
        assert_eq!(frame.session_id(), 999);
    }

    #[test]
    fn test_progress() {
        let data = vec![42u8; 102400]; // 100KB，4 个分片
        let mut sender = BlobSender::new(data, 111);

        assert_eq!(sender.progress(), 0.0);

        sender.next_frame().unwrap();
        assert!((sender.progress() - 0.25).abs() < 0.01);

        sender.next_frame().unwrap();
        assert!((sender.progress() - 0.50).abs() < 0.01);

        sender.next_frame().unwrap();
        assert!((sender.progress() - 0.75).abs() < 0.01);

        sender.next_frame().unwrap();
        assert!((sender.progress() - 1.00).abs() < 0.01);
    }
}
