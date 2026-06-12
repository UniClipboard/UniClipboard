//! Blob domain module.
//!
//! Holds the read/write port abstractions for blob storage. The blob value
//! object itself (`Blob`, `BlobStorageLocator`) and storage-format details
//! live in `uc-infra` — only the cross-layer contracts are exposed here.

pub mod ports;
