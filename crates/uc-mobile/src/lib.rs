//! `uc-mobile` — UniFFI boundary crate for the mobile spike.
//!
//! Scope (see `.planning/research/uc-mobile-spike-plan.md`):
//! - B1: synchronous pure function [`parse_connect_uri`] backed by
//!   `uc-mobile-proto`, proving the Rust → Swift/Kotlin codegen pipeline,
//!   plus the `with_foreign` constructor-argument probe (seam 2).
//! - B2 ([`client`]): [`uc_mobile_init`] (rustls ring provider, seam 1) and
//!   the async [`MobileSyncClient`] (reqwest + dedicated current_thread
//!   tokio runtime; seam-3 drop semantics documented there).
//!
//! ## FFI mirror types
//!
//! [`ConnectPayload`] / [`ConnectUriError`] are deliberate FFI-local mirrors
//! of the `uc_mobile_proto` types, not re-exports: UniFFI records cannot carry
//! `BTreeMap` / `&'static str` / `usize`, and keeping the proto crate free of
//! uniffi derives preserves its zero-heavy-deps leaf status. The `From` impls
//! below are the single conversion seam; golden-vector unit tests pin the
//! mapping.

use std::collections::HashMap;

pub mod client;
pub mod reducer;

pub use client::{
    first_reachable, uc_mobile_init, ClipboardKind, ClipboardMeta, MobileSyncClient,
    PlatformBridge, ProbeReport, ProbeResult, ServerConfig, SyncError,
};

uniffi::setup_scaffolding!();

/// FFI mirror of `uc_mobile_proto::ConnectPayload` (spec §3.1).
///
/// Field semantics are identical to the proto type; `o` becomes `other`
/// because single-letter wire names make terrible Swift/Kotlin API, and the
/// FFI surface is not the wire format — bytes are produced/consumed only by
/// `uc-mobile-proto`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConnectPayload {
    /// Payload schema version; v1 = 1.
    pub v: u32,
    /// Primary server base URL (equals `urls[0]` when `urls` is non-empty).
    pub url: String,
    /// Ordered candidate base URLs (empty for single-candidate codes).
    pub urls: Vec<String>,
    /// HTTP Basic Auth username.
    pub user: String,
    /// HTTP Basic Auth plaintext password (one-shot display semantics, spec §5.1).
    pub pwd: String,
    /// Extension metadata KV (`o` on the wire); unknown keys are preserved.
    pub other: HashMap<String, String>,
}

impl From<uc_mobile_proto::ConnectPayload> for ConnectPayload {
    fn from(p: uc_mobile_proto::ConnectPayload) -> Self {
        Self {
            v: p.v,
            url: p.url,
            urls: p.urls,
            user: p.user,
            pwd: p.pwd,
            other: p.o.into_iter().collect(),
        }
    }
}

/// FFI mirror of `uc_mobile_proto::ConnectUriError` (spec §4.2 error table).
///
/// Variant-for-variant identical; payload fields are widened to FFI-safe
/// types (`&'static str` → `String`, `usize` → `u64`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ConnectUriError {
    #[error("invalid scheme or host (must be uniclipboard://connect)")]
    InvalidScheme,
    #[error("unsupported version (only v=1 is supported)")]
    UnsupportedVersion,
    #[error("unsupported service (only svc=mobile-sync is supported)")]
    UnsupportedService,
    #[error("payload decode failed: {reason}")]
    PayloadDecodeFailed { reason: String },
    #[error("required field missing or empty: {field}")]
    MissingField { field: String },
    #[error("invalid url: must start with http:// or https://")]
    InvalidUrl,
    #[error("uri too long ({len} chars, max {max})")]
    UriTooLong { len: u64, max: u64 },
}

impl From<uc_mobile_proto::ConnectUriError> for ConnectUriError {
    fn from(e: uc_mobile_proto::ConnectUriError) -> Self {
        use uc_mobile_proto::ConnectUriError as Proto;
        match e {
            Proto::InvalidScheme => Self::InvalidScheme,
            Proto::UnsupportedVersion => Self::UnsupportedVersion,
            Proto::UnsupportedService => Self::UnsupportedService,
            Proto::PayloadDecodeFailed(reason) => Self::PayloadDecodeFailed { reason },
            Proto::MissingField(field) => Self::MissingField {
                field: field.to_string(),
            },
            Proto::InvalidUrl => Self::InvalidUrl,
            Proto::UriTooLong { len, max } => Self::UriTooLong {
                len: len as u64,
                max: max as u64,
            },
        }
    }
}

/// Parse a `uniclipboard://connect` QR text into its structured payload.
///
/// Thin FFI wrapper over [`uc_mobile_proto::parse_mobile_sync_connect_uri`];
/// all protocol semantics (and the golden vectors) live there.
#[uniffi::export]
pub fn parse_connect_uri(uri: String) -> Result<ConnectPayload, ConnectUriError> {
    uc_mobile_proto::parse_mobile_sync_connect_uri(&uri)
        .map(Into::into)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same golden vector as `uc-mobile-proto` spec §7.1 — pins the FFI
    /// mirror conversion, not the protocol itself.
    const GOLDEN_URI: &str = "uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ";

    #[test]
    fn parse_golden_uri_maps_all_fields() {
        let p = parse_connect_uri(GOLDEN_URI.to_string()).expect("golden vector parses");
        assert_eq!(p.v, 1);
        assert_eq!(p.url, "http://192.168.1.5:42720");
        assert!(p.urls.is_empty());
        assert_eq!(p.user, "mobile_aabbccdd");
        assert_eq!(p.pwd, "AbCdEfGhIjKlMnOpQrSt");
        assert_eq!(p.other.get("did").map(String::as_str), Some("did_0123abcd"));
        assert_eq!(p.other.get("label").map(String::as_str), Some("Test"));
        assert_eq!(
            p.other.get("proto").map(String::as_str),
            Some("syncclipboard")
        );
        assert_eq!(p.other.len(), 3);
    }

    #[test]
    fn error_variants_map_one_to_one() {
        use uc_mobile_proto::ConnectUriError as Proto;
        assert_eq!(
            ConnectUriError::from(Proto::MissingField("pwd")),
            ConnectUriError::MissingField {
                field: "pwd".into()
            }
        );
        assert_eq!(
            ConnectUriError::from(Proto::UriTooLong {
                len: 2001,
                max: 2000
            }),
            ConnectUriError::UriTooLong {
                len: 2001,
                max: 2000
            }
        );
        // Display strings must stay aligned with the proto error table so
        // Swift `localizedDescription` matches spec §4.2 wording.
        assert_eq!(
            ConnectUriError::from(Proto::InvalidUrl).to_string(),
            Proto::InvalidUrl.to_string()
        );
    }

    #[test]
    fn client_calls_back_into_bridge() {
        struct TestBridge;
        impl PlatformBridge for TestBridge {
            fn app_group_dir(&self) -> String {
                "/tmp/test-app-group".into()
            }
        }
        uc_mobile_init();
        let client = MobileSyncClient::new(std::sync::Arc::new(TestBridge), false)
            .expect("constructor succeeds after init");
        assert_eq!(client.bridge_probe(), "/tmp/test-app-group");
    }
}
