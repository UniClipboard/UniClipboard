//! BlobStream 集成测试
//!
//! 测试发送器和接收器的端到端功能

use crate::infrastructure::p2p::blob::receiver::{BlobReceiver, FrameHandleResult};
use crate::infrastructure::p2p::blob::sender::{BlobMetadata, BlobSender};
use crate::infrastructure::p2p::blob::Frame;

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// 测试完整的发送和接收流程
    #[test]
    fn test_end_to_end_transfer() {
        let session_id = 12345;

        // 创建测试数据（100KB）
        let test_data = vec![0x42u8; 102400];

        // 创建发送器
        let mut sender = BlobSender::new(test_data.clone(), session_id);

        // 创建接收器
        let mut receiver = BlobReceiver::new(session_id);

        // 1. 发送并处理元数据帧
        let metadata_frame = sender.make_metadata_frame().unwrap();
        let result = receiver.handle_frame(metadata_frame).unwrap();
        assert_eq!(result, FrameHandleResult::MetadataReceived);

        // 设置接收器的元数据（模拟从元数据帧中解析）
        let metadata = sender.metadata().clone();
        receiver.set_metadata(metadata);

        // 2. 发送并处理所有数据帧
        let mut data_frame_count = 0;
        loop {
            match sender.next_frame() {
                Ok(Some(frame)) => {
                    data_frame_count += 1;
                    let result = receiver.handle_frame(frame).unwrap();
                    match result {
                        FrameHandleResult::DataReceived { complete } => {
                            if complete {
                                break;
                            }
                        }
                        _ => panic!("Expected DataReceived result"),
                    }
                }
                Ok(None) => break,
                Err(e) => panic!("Error getting next frame: {}", e),
            }
        }

        assert!(data_frame_count > 0, "Should have sent at least one data frame");
        assert!(sender.is_complete());

        // 3. 发送并处理完成帧
        let complete_frame = sender.make_complete_frame();
        let result = receiver.handle_frame(complete_frame).unwrap();
        assert_eq!(result, FrameHandleResult::TransferComplete);

        // 4. 组装数据
        let assembled_data = receiver.assemble().unwrap();

        // 5. 验证数据
        assert_eq!(assembled_data.len(), test_data.len());
        assert_eq!(assembled_data, test_data);
    }

    /// 测试小数据传输（小于一个分片）
    #[test]
    fn test_small_data_transfer() {
        let session_id = 67890;

        // 创建测试数据（1KB）
        let test_data = vec![0xABu8; 1024];

        // 创建发送器
        let mut sender = BlobSender::new(test_data.clone(), session_id);

        // 创建接收器
        let mut receiver = BlobReceiver::new(session_id);

        // 发送元数据帧
        let metadata_frame = sender.make_metadata_frame().unwrap();
        receiver.handle_frame(metadata_frame).unwrap();
        receiver.set_metadata(sender.metadata().clone());

        // 发送数据帧
        let data_frame = sender.next_frame().unwrap().unwrap();
        receiver.handle_frame(data_frame).unwrap();

        // 发送完成帧
        let complete_frame = sender.make_complete_frame();
        receiver.handle_frame(complete_frame).unwrap();

        // 组装并验证
        let assembled_data = receiver.assemble().unwrap();
        assert_eq!(assembled_data, test_data);
    }

    /// 测试帧序列化和反序列化
    #[test]
    fn test_frame_serialization() {
        let session_id = 99999;

        // 创建测试数据
        let test_data = vec![0xCCu8; 4096];

        // 创建发送器
        let sender = BlobSender::new(test_data, session_id);

        // 创建元数据帧并序列化
        let metadata_frame = sender.make_metadata_frame().unwrap();
        let metadata_bytes = metadata_frame.to_bytes().unwrap();

        // 反序列化元数据帧
        let decoded_metadata_frame = Frame::from_bytes(&metadata_bytes).unwrap();
        assert_eq!(
            decoded_metadata_frame.session_id(),
            metadata_frame.session_id()
        );
        assert_eq!(
            decoded_metadata_frame.frame_type(),
            metadata_frame.frame_type()
        );

        // 创建数据帧并序列化
        let mut sender_clone = BlobSender::new(vec![0xDDu8; 4096], session_id);
        let data_frame = sender_clone.next_frame().unwrap().unwrap();
        let data_bytes = data_frame.to_bytes().unwrap();

        // 反序列化数据帧
        let decoded_data_frame = Frame::from_bytes(&data_bytes).unwrap();
        assert_eq!(decoded_data_frame.session_id(), data_frame.session_id());
        assert_eq!(decoded_data_frame.sequence(), data_frame.sequence());
        assert_eq!(decoded_data_frame.data, data_frame.data);
    }

    /// 测试进度跟踪
    #[test]
    fn test_progress_tracking() {
        let session_id = 11111;

        // 创建测试数据（需要多个分片）
        let test_data = vec![0xEEu8; 100000];

        let mut sender = BlobSender::new(test_data, session_id);
        let mut receiver = BlobReceiver::new(session_id);

        // 初始进度
        assert_eq!(sender.progress(), 0.0);

        // 发送元数据
        let metadata_frame = sender.make_metadata_frame().unwrap();
        receiver.handle_frame(metadata_frame).unwrap();
        receiver.set_metadata(sender.metadata().clone());

        // 发送一半的数据帧
        let total_frames = sender.total_frames();
        let data_frames = total_frames - 2; // 减去元数据帧和完成帧

        for _ in 0..(data_frames / 2) {
            if let Some(frame) = sender.next_frame().unwrap() {
                receiver.handle_frame(frame).unwrap();
            }
        }

        // 检查进度大约在 50%
        let progress = sender.progress();
        assert!(progress > 0.4 && progress < 0.6, "Progress should be around 50%, got {}", progress);
    }
}
