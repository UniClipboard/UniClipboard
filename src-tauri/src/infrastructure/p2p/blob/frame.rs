//! BlobStream 帧格式定义
//!
//! 定义了分片流协议的帧类型、帧头和完整帧结构。

use serde::{Deserialize, Serialize};

/// 分片流协议版本
/// v2: 添加 data_size 字段到元数据帧
pub const PROTOCOL_VERSION: u8 = 2;

/// 分片大小：32KB
pub const CHUNK_SIZE: usize = 32768;

/// 帧头固定大小（36 字节）
pub const FRAME_HEADER_SIZE: usize = 36;

/// 帧类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum FrameType {
    /// 元数据帧：包含传输的元信息（文件大小、哈希等）
    Metadata = 0,
    /// 数据帧：包含实际的数据分片
    Data = 1,
    /// 完成帧：表示传输完成
    Complete = 2,
    /// 错误帧：表示传输过程中发生错误
    Error = 3,
}

impl FrameType {
    /// 从 u8 转换为 FrameType
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(FrameType::Metadata),
            1 => Some(FrameType::Data),
            2 => Some(FrameType::Complete),
            3 => Some(FrameType::Error),
            _ => None,
        }
    }
}

/// 分片帧头（36 字节）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeader {
    /// 协议版本
    pub version: u8,
    /// 帧类型
    pub frame_type: u8,
    /// 保留字段（2 字节）
    pub reserved: [u8; 2],
    /// 会话 ID（用于区分不同的传输会话）
    pub session_id: u32,
    /// 序列号（当前帧的序号）
    /// 对于元数据帧，此字段编码 data_size 的低 32 位
    pub sequence: u32,
    /// 总帧数（元数据帧中设置）
    pub total_frames: u32,
    /// 数据长度（当前帧的数据部分长度）
    /// 对于元数据帧，此字段编码 data_size 的高 32 位
    pub data_length: u32,
    /// BLAKE3 哈希前 16 字节（用于完整性校验）
    pub hash_prefix: [u8; 16],
    /// 数据总大小（仅用于元数据帧，v2 协议新增）
    /// 在序列化时编码到 sequence 和 data_length 字段
    #[serde(skip)]
    pub data_size: u64,
}

impl FrameHeader {
    /// 创建新的帧头
    pub fn new(
        frame_type: FrameType,
        session_id: u32,
        sequence: u32,
        total_frames: u32,
        data_length: u32,
        hash_prefix: [u8; 16],
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            frame_type: frame_type as u8,
            reserved: [0; 2],
            session_id,
            sequence,
            total_frames,
            data_length,
            hash_prefix,
            data_size: 0,
        }
    }

    /// 创建元数据帧头（v2 协议）
    pub fn metadata(session_id: u32, total_frames: u32, data_size: u64, hash_prefix: [u8; 16]) -> Self {
        let size_bytes = data_size.to_be_bytes();
        let sequence = u32::from_be_bytes([size_bytes[4], size_bytes[5], size_bytes[6], size_bytes[7]]);
        let data_length = u32::from_be_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]]);

        Self {
            version: PROTOCOL_VERSION,
            frame_type: FrameType::Metadata as u8,
            reserved: [0; 2],
            session_id,
            sequence,
            total_frames,
            data_length,
            hash_prefix,
            data_size,
        }
    }

    /// 创建数据帧头
    pub fn data(
        session_id: u32,
        sequence: u32,
        total_frames: u32,
        data_length: u32,
        hash_prefix: [u8; 16],
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            frame_type: FrameType::Data as u8,
            reserved: [0; 2],
            session_id,
            sequence,
            total_frames,
            data_length,
            hash_prefix,
            data_size: 0,
        }
    }

    /// 创建完成帧头
    pub fn complete(session_id: u32) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            frame_type: FrameType::Complete as u8,
            reserved: [0; 2],
            session_id,
            sequence: 0,
            total_frames: 0,
            data_length: 0,
            hash_prefix: [0; 16],
            data_size: 0,
        }
    }

    /// 创建错误帧头
    pub fn error(session_id: u32) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            frame_type: FrameType::Error as u8,
            reserved: [0; 2],
            session_id,
            sequence: 0,
            total_frames: 0,
            data_length: 0,
            hash_prefix: [0; 16],
            data_size: 0,
        }
    }

    /// 获取帧类型
    pub fn get_frame_type(&self) -> Option<FrameType> {
        FrameType::from_u8(self.frame_type)
    }

    /// 序列化为固定的 36 字节
    pub fn to_fixed_bytes(&self) -> [u8; 36] {
        let mut buf = [0u8; 36];
        buf[0] = self.version;
        buf[1] = self.frame_type;
        buf[2] = self.reserved[0];
        buf[3] = self.reserved[1];
        buf[4..8].copy_from_slice(&self.session_id.to_be_bytes());
        buf[8..12].copy_from_slice(&self.sequence.to_be_bytes());
        buf[12..16].copy_from_slice(&self.total_frames.to_be_bytes());
        buf[16..20].copy_from_slice(&self.data_length.to_be_bytes());
        buf[20..36].copy_from_slice(&self.hash_prefix);
        buf
    }

    /// 从 36 字节反序列化
    pub fn from_fixed_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 36 {
            return Err("Insufficient bytes for FrameHeader".to_string());
        }

        let frame_type = bytes[1];
        let sequence = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
        let data_length = u32::from_be_bytes(bytes[16..20].try_into().unwrap());

        // 对于元数据帧，从 sequence 和 data_length 字段解码 data_size
        let data_size = if frame_type == FrameType::Metadata as u8 {
            let high = data_length as u64;
            let low = sequence as u64;
            (high << 32) | low
        } else {
            0
        };

        Ok(Self {
            version: bytes[0],
            frame_type,
            reserved: [bytes[2], bytes[3]],
            session_id: u32::from_be_bytes(bytes[4..8].try_into().unwrap()),
            sequence,
            total_frames: u32::from_be_bytes(bytes[12..16].try_into().unwrap()),
            data_length,
            hash_prefix: bytes[20..36].try_into().unwrap(),
            data_size,
        })
    }
}

/// 完整帧（头部 + 数据）
#[derive(Debug, Clone)]
pub struct Frame {
    /// 帧头
    pub header: FrameHeader,
    /// 帧数据
    pub data: Vec<u8>,
}

impl Frame {
    /// 创建新帧
    pub fn new(header: FrameHeader, data: Vec<u8>) -> Self {
        Self { header, data }
    }

    /// 序列化为字节（使用固定二进制布局）
    pub fn to_bytes(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // 使用手动序列化，固定字节头部
        let header_bytes = self.header.to_fixed_bytes();

        let total_length = header_bytes.len() + self.data.len();
        let mut buffer = Vec::with_capacity(total_length);
        buffer.extend_from_slice(&header_bytes);
        buffer.extend_from_slice(&self.data);

        Ok(buffer)
    }

    /// 从字节反序列化（使用固定二进制布局）
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        // 检查最小长度
        if bytes.len() < FRAME_HEADER_SIZE {
            return Err(format!(
                "Frame too short: expected at least {} bytes, got {}",
                FRAME_HEADER_SIZE,
                bytes.len()
            )
            .into());
        }

        // 从固定字节反序列化头部
        let header = FrameHeader::from_fixed_bytes(&bytes[..FRAME_HEADER_SIZE])
            .map_err(|e| format!("Failed to deserialize frame header: {}", e))?;

        // 检查版本
        if header.version != PROTOCOL_VERSION {
            return Err(format!(
                "Invalid protocol version: expected {}, got {}",
                PROTOCOL_VERSION, header.version
            )
            .into());
        }

        // 检查数据长度
        let expected_data_len = header.data_length as usize;
        let actual_data_len = bytes.len() - FRAME_HEADER_SIZE;

        if expected_data_len != actual_data_len {
            return Err(format!(
                "Data length mismatch: expected {} bytes, got {}",
                expected_data_len, actual_data_len
            )
            .into());
        }

        // 提取数据
        let data = bytes[FRAME_HEADER_SIZE..].to_vec();

        Ok(Self { header, data })
    }

    /// 获取帧类型
    pub fn frame_type(&self) -> Option<FrameType> {
        self.header.get_frame_type()
    }

    /// 获取会话 ID
    pub fn session_id(&self) -> u32 {
        self.header.session_id
    }

    /// 获取序列号
    pub fn sequence(&self) -> u32 {
        self.header.sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_type_conversion() {
        assert_eq!(FrameType::from_u8(0), Some(FrameType::Metadata));
        assert_eq!(FrameType::from_u8(1), Some(FrameType::Data));
        assert_eq!(FrameType::from_u8(2), Some(FrameType::Complete));
        assert_eq!(FrameType::from_u8(3), Some(FrameType::Error));
        assert_eq!(FrameType::from_u8(99), None);
    }

    #[test]
    fn test_frame_serialization() {
        let header = FrameHeader::data(
            12345,
            1,
            100,
            5,
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );
        let data = vec![1u8, 2, 3, 4, 5];
        let frame = Frame::new(header, data.clone());

        let bytes = frame.to_bytes().unwrap();
        let decoded = Frame::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.header.version, PROTOCOL_VERSION);
        assert_eq!(decoded.header.frame_type, FrameType::Data as u8);
        assert_eq!(decoded.header.session_id, 12345);
        assert_eq!(decoded.header.total_frames, 100);
        assert_eq!(decoded.data, data);
    }

    #[test]
    fn test_data_frame() {
        let header = FrameHeader::data(100, 5, 10, 1024, [0; 16]);
        let data = vec![42u8; 1024];
        let frame = Frame::new(header, data);

        assert_eq!(frame.session_id(), 100);
        assert_eq!(frame.sequence(), 5);
    }

    #[test]
    fn test_complete_frame() {
        let header = FrameHeader::complete(999);
        let frame = Frame::new(header, vec![]);

        assert_eq!(frame.frame_type(), Some(FrameType::Complete));
        assert_eq!(frame.session_id(), 999);
    }
}
