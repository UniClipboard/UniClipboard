// BlobStream 分片流协议模块
// 用于大文件（剪贴板内容）的可靠传输

pub mod frame;
pub mod integration_test;
pub mod receiver;
pub mod sender;

pub use frame::{Frame, FrameHeader, FrameType, PROTOCOL_VERSION};
pub use receiver::{BlobReceiver, FrameHandleResult};
pub use sender::{BlobMetadata, BlobSender};

/// BlobStream 协议名称
pub const BLOBSTREAM_PROTOCOL: &str = "/uniclipboard/blob-stream/1.0.0";
