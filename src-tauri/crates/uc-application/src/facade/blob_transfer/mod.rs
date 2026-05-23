//! Blob 传输门面。
//!
//! CLI / daemon / future UI 只从这里进入,不直接导入 use case。

mod facade;

pub use facade::{
    BatchPosition, BlobTransferDeps, BlobTransferError, BlobTransferFacade, FetchBlobCommand,
    FetchBlobResult, FetchBlobToPathCommand, FetchBlobToPathResult, FetchTransferContext,
    InboundCancelOutcome, PublishBlobCommand, PublishBlobPathCommand, PublishBlobResult,
    SharedHostEventEmitter,
};
