//! 文件传输 lifecycle 应用层入口。
//!
//! 按 `uc-application/AGENTS.md` §11.4，外部 crate（bootstrap / daemon /
//! tauri / cli / webserver）只能通过本目录下的 [`FileTransferFacade`]
//! 访问 file-transfer 用例；内部 `*UseCase` 类型保持 `pub(crate)`，
//! 不向外暴露。

mod facade;

pub use crate::file_transfer::{
    CancelTransfer, CompleteTransfer, FailTransfer, FileTransferApplicationError,
    ReportTransferProgress, StartTransfer,
};
pub use facade::{FileTransferFacade, FileTransferFacadeDeps, LinkTransferToEntry};
