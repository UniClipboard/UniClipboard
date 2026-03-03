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
//! use uc_core::security::aad;
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

use crate::ids::{BlobId, EventId, RepresentationId};

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
/// use uc_core::security::aad::for_inline;
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
/// use uc_core::security::aad::for_blob;
/// use uc_core::ids::BlobId;
///
/// let blob_id = BlobId::from("test-blob");
/// let aad = for_blob(&blob_id);
/// assert_eq!(aad, b"uc:blob:v1|test-blob".to_vec());
/// ```
pub fn for_blob(blob_id: &BlobId) -> Vec<u8> {
    format!("{AAD_NAMESPACE}:blob:{AAD_VERSION}|{}", blob_id.as_ref()).into_bytes()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_for_inline_is_deterministic() {
        let event_id = EventId::from("test-event");
        let rep_id = RepresentationId::from("test-rep");

        let aad1 = for_inline(&event_id, &rep_id);
        let aad2 = for_inline(&event_id, &rep_id);

        assert_eq!(aad1, aad2, "AAD should be deterministic for same inputs");
    }

    #[test]
    fn test_for_inline_includes_context() {
        let event_id = EventId::from("my-event");
        let rep_id = RepresentationId::from("my-rep");

        let aad = for_inline(&event_id, &rep_id);
        let aad_str = String::from_utf8(aad).unwrap();

        assert!(aad_str.contains("my-event"), "AAD should contain event ID");
        assert!(
            aad_str.contains("my-rep"),
            "AAD should contain representation ID"
        );
        assert!(
            aad_str.starts_with("uc:inline:v1|"),
            "AAD should have correct prefix"
        );
    }

    #[test]
    fn test_for_inline_differs_by_inputs() {
        let event_id1 = EventId::from("event-1");
        let event_id2 = EventId::from("event-2");
        let rep_id = RepresentationId::from("rep-1");

        let aad1 = for_inline(&event_id1, &rep_id);
        let aad2 = for_inline(&event_id2, &rep_id);

        assert_ne!(aad1, aad2, "AAD should differ for different event IDs");

        let rep_id2 = RepresentationId::from("rep-2");
        let aad3 = for_inline(&event_id1, &rep_id2);

        assert_ne!(
            aad1, aad3,
            "AAD should differ for different representation IDs"
        );
    }

    #[test]
    fn test_for_blob_is_deterministic() {
        let blob_id = BlobId::from("test-blob");

        let aad1 = for_blob(&blob_id);
        let aad2 = for_blob(&blob_id);

        assert_eq!(aad1, aad2, "AAD should be deterministic for same blob ID");
    }

    #[test]
    fn test_for_blob_includes_blob_id() {
        let blob_id = BlobId::from("my-blob");

        let aad = for_blob(&blob_id);
        let aad_str = String::from_utf8(aad).unwrap();

        assert!(aad_str.contains("my-blob"), "AAD should contain blob ID");
        assert!(
            aad_str.starts_with("uc:blob:v1|"),
            "AAD should have correct prefix"
        );
    }

    #[test]
    fn test_for_blob_differs_by_blob_id() {
        let blob_id1 = BlobId::from("blob-1");
        let blob_id2 = BlobId::from("blob-2");

        let aad1 = for_blob(&blob_id1);
        let aad2 = for_blob(&blob_id2);

        assert_ne!(aad1, aad2, "AAD should differ for different blob IDs");
    }

    #[test]
    fn test_inline_and_blob_aad_are_distinct() {
        let event_id = EventId::from("test-event");
        let rep_id = RepresentationId::from("test-rep");
        let blob_id = BlobId::from("test-id");

        let inline_aad = for_inline(&event_id, &rep_id);
        let blob_aad = for_blob(&blob_id);

        assert_ne!(
            inline_aad, blob_aad,
            "Inline and blob AAD should use different prefixes"
        );
    }

    #[test]
    fn test_aad_format_version() {
        // This test ensures version consistency across AAD types.
        // If the version changes, update AAD_VERSION constant.
        let event_id = EventId::from("test");
        let rep_id = RepresentationId::from("test");
        let blob_id = BlobId::from("test");
        let message_id = "test";

        let inline_aad = String::from_utf8(for_inline(&event_id, &rep_id)).unwrap();
        let blob_aad = String::from_utf8(for_blob(&blob_id)).unwrap();
        let network_clipboard_aad = String::from_utf8(for_network_clipboard(message_id)).unwrap();

        assert!(
            inline_aad.contains(":v1|"),
            "Inline AAD should use v1 format"
        );
        assert!(blob_aad.contains(":v1|"), "Blob AAD should use v1 format");
        assert!(
            network_clipboard_aad.contains(":v1|"),
            "Network clipboard AAD should use v1 format"
        );
    }

    #[test]
    fn test_for_network_clipboard_is_deterministic() {
        let message_id = "msg-123";

        let aad1 = for_network_clipboard(message_id);
        let aad2 = for_network_clipboard(message_id);

        assert_eq!(
            aad1, aad2,
            "Network clipboard AAD should be deterministic for same message ID"
        );
    }

    #[test]
    fn test_for_network_clipboard_includes_message_id_and_prefix() {
        let message_id = "my-message-id";

        let aad = String::from_utf8(for_network_clipboard(message_id)).unwrap();

        assert_eq!(
            aad, "uc:net_clipboard:v1|my-message-id",
            "Network clipboard AAD should match expected format"
        );
    }

    #[test]
    fn test_for_network_clipboard_differs_by_message_id() {
        let aad1 = for_network_clipboard("msg-1");
        let aad2 = for_network_clipboard("msg-2");

        assert_ne!(
            aad1, aad2,
            "Network clipboard AAD should differ for different message IDs"
        );
    }

    #[test]
    fn for_chunk_transfer_is_deterministic() {
        let id = [0x42u8; 16];
        let aad1 = for_chunk_transfer(&id, 5);
        let aad2 = for_chunk_transfer(&id, 5);
        assert_eq!(aad1, aad2);
    }

    #[test]
    fn for_chunk_transfer_length_is_20() {
        let id = [0u8; 16];
        let aad = for_chunk_transfer(&id, 0);
        assert_eq!(aad.len(), 20);
    }

    #[test]
    fn for_chunk_transfer_encodes_chunk_index_le() {
        let id = [0u8; 16];
        // chunk_index=1 in LE is [1, 0, 0, 0]
        let aad = for_chunk_transfer(&id, 1);
        assert_eq!(&aad[16..], &[1u8, 0, 0, 0]);
    }

    #[test]
    fn for_chunk_transfer_differs_by_chunk_index() {
        let id = [0u8; 16];
        assert_ne!(for_chunk_transfer(&id, 0), for_chunk_transfer(&id, 1));
    }

    #[test]
    fn for_chunk_transfer_differs_by_transfer_id() {
        let id1 = [0u8; 16];
        let id2 = [1u8; 16];
        assert_ne!(for_chunk_transfer(&id1, 0), for_chunk_transfer(&id2, 0));
    }
}
