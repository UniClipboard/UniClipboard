//! Additional Authenticated Data (AAD) generation for encryption.
//!
//! This module provides centralized AAD generation functions to ensure
//! consistency across all encryption/decryption operations.
//!
//! # AAD Format
//!
//! AAD follows the pattern: `uc:<type>:v1|<identifiers>`
//!
//! - `uc:` - Application namespace prefix
//! - `<type>` - Data type (inline, blob)
//! - `:v1` - Format version
//! - `|<identifiers>` - Pipe-separated context identifiers
//!
//! # Example
//!
//! ```rust
//! use uc_core::crypto::aad;
//! use uc_core::ids::{EventId, RepresentationId, BlobId};
//!
//! // For inline clipboard data
//! let event_id = EventId::new();
//! let rep_id = RepresentationId::new();
//! let aad = aad::for_inline(&event_id, &rep_id);
//!
//! // For blob storage
//! let blob_id = BlobId::new();
//! let aad = aad::for_blob(&blob_id);
//! ```

use crate::ids::{BlobId, EntryId, EventId, RepresentationId};

/// Current AAD format version.
const AAD_VERSION: &str = "v1";

/// AAD namespace prefix for all application data.
const AAD_NAMESPACE: &str = "uc";

/// Generates AAD for inline clipboard data encryption/decryption.
///
/// # Format
///
/// `uc:inline:v1|{event_id}|{representation_id}`
///
/// # Arguments
///
/// * `event_id` - The clipboard event identifier
/// * `rep_id` - The representation identifier
///
/// # Returns
///
/// AAD as bytes for use with AEAD encryption.
///
/// # Examples
///
/// ```rust
/// use uc_core::crypto::aad::for_inline;
/// use uc_core::ids::{EventId, RepresentationId};
///
/// let event_id = EventId::from("test-event");
/// let rep_id = RepresentationId::from("test-rep");
/// let aad = for_inline(&event_id, &rep_id);
/// assert_eq!(aad, b"uc:inline:v1|test-event|test-rep".to_vec());
/// ```
pub fn for_inline(event_id: &EventId, rep_id: &RepresentationId) -> Vec<u8> {
    format!(
        "{AAD_NAMESPACE}:inline:{AAD_VERSION}|{}|{}",
        event_id.as_ref(),
        rep_id.as_ref()
    )
    .into_bytes()
}

/// Generates AAD for blob storage encryption/decryption.
///
/// # Format
///
/// `uc:blob:v1|{blob_id}`
///
/// # Arguments
///
/// * `blob_id` - The blob identifier
///
/// # Returns
///
/// AAD as bytes for use with AEAD encryption.
///
/// # Examples
///
/// ```rust
/// use uc_core::crypto::aad::for_blob;
/// use uc_core::ids::BlobId;
///
/// let blob_id = BlobId::from("test-blob");
/// let aad = for_blob(&blob_id);
/// assert_eq!(aad, b"uc:blob:v1|test-blob".to_vec());
/// ```
pub fn for_blob(blob_id: &BlobId) -> Vec<u8> {
    format!("{AAD_NAMESPACE}:blob:{AAD_VERSION}|{}", blob_id.as_ref()).into_bytes()
}

/// AAD format version for V2 blob storage (binary format with zstd compression).
const AAD_BLOB_V2_VERSION: &str = "v2";

/// Generates AAD for V2 blob storage encryption/decryption.
///
/// # Format
///
/// `uc:blob:v2|{blob_id}`
///
/// This is used for the new binary blob format that supports zstd compression.
/// The V1 `for_blob` function is kept unchanged for backward compatibility with
/// inline data tests and network clipboard operations.
///
/// # Arguments
///
/// * `blob_id` - The blob identifier
///
/// # Returns
///
/// AAD as bytes for use with AEAD encryption.
pub fn for_blob_v2(blob_id: &BlobId) -> Vec<u8> {
    format!(
        "{AAD_NAMESPACE}:blob:{AAD_BLOB_V2_VERSION}|{}",
        blob_id.as_ref()
    )
    .into_bytes()
}

/// Generates AAD for the per-entry search render payload encryption/decryption.
///
/// # Format
///
/// `uc:search_render:v1|{entry_id}`
///
/// Binds the encrypted render payload to its search-index row so a ciphertext
/// cannot be transplanted onto a different entry.
///
/// # Examples
///
/// ```rust
/// use uc_core::crypto::aad::for_search_render;
/// use uc_core::ids::EntryId;
///
/// let entry_id = EntryId::from("test-entry");
/// let aad = for_search_render(&entry_id);
/// assert_eq!(aad, b"uc:search_render:v1|test-entry".to_vec());
/// ```
pub fn for_search_render(entry_id: &EntryId) -> Vec<u8> {
    format!(
        "{AAD_NAMESPACE}:search_render:{AAD_VERSION}|{}",
        entry_id.as_ref()
    )
    .into_bytes()
}

/// Generates AAD for a per-line entry-file-set path column encryption.
///
/// # Format
///
/// `uc:file_set_line:v1|{entry_id}|{line_index}`
///
/// Binds a sealed path column (`original_text` / `relative_path`) to its exact
/// manifest row so a ciphertext cannot be transplanted onto a different entry
/// or a different line within the same entry.
///
/// # Examples
///
/// ```rust
/// use uc_core::crypto::aad::for_file_set_line;
/// use uc_core::ids::EntryId;
///
/// let entry_id = EntryId::from("test-entry");
/// let aad = for_file_set_line(&entry_id, 3);
/// assert_eq!(aad, b"uc:file_set_line:v1|test-entry|3".to_vec());
/// ```
pub fn for_file_set_line(entry_id: &EntryId, line_index: i64) -> Vec<u8> {
    format!(
        "{AAD_NAMESPACE}:file_set_line:{AAD_VERSION}|{}|{line_index}",
        entry_id.as_ref()
    )
    .into_bytes()
}

/// Generates AAD for an encrypted directory publication root mapping.
///
/// Binds recovery metadata to one entry and one receive attempt so ciphertext
/// cannot be transplanted across retries.
pub fn for_directory_publish_log(entry_id: &EntryId, attempt_id: &str) -> Vec<u8> {
    format!(
        "{AAD_NAMESPACE}:directory_publish:{AAD_VERSION}|{}|{attempt_id}",
        entry_id.as_ref()
    )
    .into_bytes()
}

/// Binds encrypted transfer metadata to its projection row.
pub fn for_file_transfer_metadata(transfer_id: &str) -> Vec<u8> {
    format!("{AAD_NAMESPACE}:file_transfer_metadata:{AAD_VERSION}|{transfer_id}").into_bytes()
}

/// Binds an encrypted transfer event to its exact timeline position and type.
pub fn for_file_transfer_event(transfer_id: &str, sequence: i32, event_type: &str) -> Vec<u8> {
    format!(
        "{AAD_NAMESPACE}:file_transfer_event:{AAD_VERSION}|{transfer_id}|{sequence}|{event_type}"
    )
    .into_bytes()
}

/// Binds encrypted receive artifact paths to one exact receive attempt.
pub fn for_receive_artifact_log(entry_id: &EntryId, attempt_id: &str) -> Vec<u8> {
    format!(
        "{AAD_NAMESPACE}:receive_artifact:{AAD_VERSION}|{}|{attempt_id}",
        entry_id.as_ref()
    )
    .into_bytes()
}

pub fn for_network_clipboard(message_id: &str) -> Vec<u8> {
    format!("{AAD_NAMESPACE}:net_clipboard:{AAD_VERSION}|{message_id}").into_bytes()
}

/// Generates AAD for chunk-level AEAD encryption in V2 clipboard transfers.
///
/// # Format
///
/// Binary: `transfer_id (16 bytes) || chunk_index (4 bytes LE)`
///
/// This is intentionally binary (not the text format used by other AAD helpers)
/// because it is used as AEAD context for XChaCha20-Poly1305 chunk encryption,
/// where binary concatenation is the standard practice.
///
/// The `transfer_id` is a UUID v4 in raw bytes (16 bytes).
/// The `chunk_index` is the 0-based chunk position (u32 little-endian).
///
/// This AAD is passed to `chacha20poly1305::aead::Payload.aad` for each chunk,
/// binding the ciphertext to its position within the transfer. This prevents
/// chunk reordering and replay attacks.
pub fn for_chunk_transfer(transfer_id: &[u8; 16], chunk_index: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(20);
    aad.extend_from_slice(transfer_id);
    aad.extend_from_slice(&chunk_index.to_le_bytes());
    aad
}
