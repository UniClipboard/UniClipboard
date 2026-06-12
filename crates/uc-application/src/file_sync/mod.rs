//! Holds residual file-sync helpers. The expired-cache cleanup use case
//! moved to `crate::usecases::clipboard_history::cleanup` (it now routes
//! every expired file through entry-aware deletion instead of bypassing
//! iroh-blobs metadata).

pub mod cleanup;
