//! Cross-cutting OpenAPI metadata for the daemon HTTP API (ADR-008 §C.5).
//!
//! This module owns the parts of the OpenAPI document that have no handler-path
//! dependency: info / servers / tags and the security schemes. It is applied by
//! the webserver's `#[derive(OpenApi)] ApiDoc` via `modifiers(&...)` in a later
//! phase (P2); P1 only defines it.
//!
//! It fixes the documented-vs-real auth transport mismatch by registering BOTH
//! schemes: `session_query` (`?auth=Session <token>`, the real browser/GUI
//! transport) and `session_header` (`Authorization: Session <token>`, the native
//! Rust client).

use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityRequirement, SecurityScheme};
use utoipa::openapi::OpenApi;
use utoipa::Modify;

/// L1/public paths that must NOT inherit the session security schemes.
const PUBLIC_PATHS: &[&str] = &["/health", "/auth/connect"];

/// Production tag taxonomy (flat, single-level per utoipa v4). The dev-only
/// `dev` tag lives in the webserver's `ApiDocDev` and is intentionally absent.
pub fn tags() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "clipboard",
            "Clipboard entry CRUD, stats, resources, history actions, and delivery",
        ),
        ("search", "Query, index status, and index rebuild"),
        ("storage", "Storage stats and cache maintenance"),
        ("device", "Local device identity"),
        ("member", "Per-space-member sync preferences"),
        ("pairing", "Space-member unpair lifecycle"),
        ("encryption", "Encryption state and session lock/unlock"),
        (
            "settings",
            "Persisted settings read/update (no OS side effects)",
        ),
        (
            "lifecycle",
            "Daemon lifecycle state, retry, and ready-signal",
        ),
        ("upgrade", "Version upgrade detection and acknowledgement"),
        (
            "system",
            "Diagnostics and topology: health, status, peer/member snapshots, presence, websocket",
        ),
        ("setup-v2", "Stateless v2 space-setup and invitation flow"),
    ]
}

/// Register the dual session security schemes and apply them to every operation
/// except the L1/public paths in [`PUBLIC_PATHS`].
///
/// Call this from a webserver `utoipa::Modify` impl so it runs against the fully
/// derived `ApiDoc`.
pub fn apply_metadata(doc: &mut OpenApi) {
    let comps = doc.components.get_or_insert_with(Default::default);

    // FIX documented-vs-real auth transport mismatch: register BOTH.
    // Primary (browser/GUI): `?auth=Session <token>` query param.
    comps.add_security_scheme(
        "session_query",
        SecurityScheme::ApiKey(ApiKey::Query(ApiKeyValue::new("auth"))),
    );
    // Native Rust client: `Authorization: Session <token>` header.
    comps.add_security_scheme(
        "session_header",
        SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("Authorization"))),
    );

    for (path, item) in doc.paths.paths.iter_mut() {
        if PUBLIC_PATHS.contains(&path.as_str()) {
            continue;
        }
        for op in item.operations.values_mut() {
            let reqs = op.security.get_or_insert_with(Default::default);
            reqs.push(SecurityRequirement::new(
                "session_query",
                std::iter::empty::<String>(),
            ));
            reqs.push(SecurityRequirement::new(
                "session_header",
                std::iter::empty::<String>(),
            ));
        }
    }
}

/// `utoipa::Modify` adapter wrapping [`apply_metadata`]. The webserver references
/// this from `#[openapi(modifiers(&SecurityAddon))]`.
pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut OpenApi) {
        apply_metadata(openapi);
    }
}
