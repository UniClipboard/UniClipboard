//! Streaming blake3 helpers for path-backed blob ingest.
//!
//! Both helpers run on a single read of the source and keep resident memory
//! independent of file size (64 KiB buffer), so they are safe for arbitrarily
//! large files. `copy_and_hash` derives the content hash from the exact bytes it
//! writes to the destination, which is what lets the ingest path record a hash
//! that can never diverge from the stored blob.

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::path::Path;
use uc_core::ContentHash;

/// Buffer size for streaming reads/writes. Resident memory stays constant
/// regardless of file size.
const STREAM_BUF_LEN: usize = 64 * 1024;

/// Stream `path` through blake3, returning `(ContentHash, byte_size)` without
/// loading the file into memory.
pub(crate) fn stream_hash_file(path: &Path) -> Result<(ContentHash, u64)> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {} for hashing", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; STREAM_BUF_LEN];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buf).context("read failed during hashing")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total = total.saturating_add(n as u64);
    }
    let hash = hasher.finalize();
    Ok((ContentHash::from(hash.as_bytes()), total))
}

/// Copy `source` to `dest` while hashing the bytes in the same pass, returning
/// `(ContentHash, byte_size)`.
///
/// The source is read exactly once and the returned hash is of the exact bytes
/// written to `dest`, so a caller can record an identity that matches the stored
/// blob even if the source is rewritten right after this returns. The bytes are
/// flushed and fsync'd before returning.
pub(crate) fn copy_and_hash(source: &Path, dest: &Path) -> Result<(ContentHash, u64)> {
    let mut src = std::fs::File::open(source)
        .with_context(|| format!("failed to open {} for copy+hash", source.display()))?;
    let mut out = std::fs::File::create(dest)
        .with_context(|| format!("failed to create {} for copy+hash", dest.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; STREAM_BUF_LEN];
    let mut total: u64 = 0;
    loop {
        let n = src.read(&mut buf).context("read failed during copy+hash")?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .context("write failed during copy+hash")?;
        hasher.update(&buf[..n]);
        total = total.saturating_add(n as u64);
    }
    out.flush().context("flush failed during copy+hash")?;
    out.sync_all().context("sync failed during copy+hash")?;
    let hash = hasher.finalize();
    Ok((ContentHash::from(hash.as_bytes()), total))
}
