//! Generates the canonical OpenAPI document at repo-root `schema/openapi.json`.
//!
//! Output is deterministic: utoipa 4.2.3 stores `paths` and `components.{schemas,
//! responses,security_schemes}` in `BTreeMap`s (always emitted alphabetically),
//! and `serde_json` 1.0.149 is built WITHOUT `preserve_order` (no `indexmap` in
//! the lockfile), so struct fields serialize in fixed definition order. Re-running
//! on any machine therefore yields a byte-identical file -> clean git diffs.
//!
//! This bin is a HARD GATE, not just a dumper: before writing it walks every
//! `$ref` and panics if any target is missing from `components.schemas`, asserts
//! the bare generic `ApiEnvelope` never leaked in as a component, and freezes the
//! §D operation/path cardinality. A broken contract fails the build here.
//!
//! Scope: production L2+ surface only (`ApiDoc`). `ApiDocDev` (the dev
//! `/auth/dev-token` doc) is `#[cfg(debug_assertions)]`-gated and intentionally
//! excluded so the committed artifact is build-profile-independent.
//!
//! Path resolution is ROBUST: the repo root is derived from `CARGO_MANIFEST_DIR`
//! (= .../src-tauri/crates/uc-webserver), NOT the current working directory, so
//! it works no matter where `cargo` is invoked from.
//!
//! Run: `cargo run -p uc-webserver --bin gen-openapi`

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::{fs, io};

use serde_json::Value;
use utoipa::OpenApi;

use uc_webserver::api::openapi::ApiDoc;

/// Frozen §D cardinality. The `paths(...)` list registers 59 handler operations,
/// but 5 paths carry two HTTP methods each (`/settings` GET+PUT,
/// `/clipboard/entries/{id}` GET+DELETE, `/member/{device_id}/sync-preferences`
/// GET+PATCH, `/mobile-sync/devices` GET+POST, `/mobile-sync/settings` GET+PATCH),
/// so they collapse to 54 unique path templates / 59 operations.
/// ADR-008 P3-1 (D15) added `POST /encryption/unlock-with-passphrase`,
/// `POST /encryption/factory-reset`, `GET /clipboard/entries/{id}/delivery`.
/// ADR-008 P3-b added the 7 `/mobile-sync/*` operations.
/// ADR-008 P3-c (D20) added `POST /analytics/capture`.
/// ADR-008 P3-3 (B2'-1) added `POST /settings/relay-probe`.
/// ADR-008 P5-L (L8d-1) surfaced `POST /lifecycle/restart`: +1 path, +1 operation.
/// ADR-008 P5-1b added the binary endpoint `GET /clipboard/entries/{id}/file`
/// (doc-only, octet-stream): +1 path, +1 operation.
const EXPECTED_PATHS: usize = 57;
const EXPECTED_OPERATIONS: usize = 62;
const SCHEMA_PREFIX: &str = "#/components/schemas/";
const HTTP_METHODS: [&str; 7] = ["get", "put", "post", "delete", "patch", "head", "options"];

/// Recursively collects every `"$ref"` string value anywhere in `value`.
/// (Mirrors the `api_doc_has_no_dangling_refs` test so the bin enforces the same
/// integrity invariant at generation time.)
fn collect_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "$ref" {
                    if let Value::String(s) = child {
                        out.push(s.clone());
                    }
                }
                collect_refs(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_refs(item, out);
            }
        }
        _ => {}
    }
}

/// Validates the materialized doc as a hard gate. Panics on any contract defect.
/// Returns `(path_count, operation_count)` for the success printout.
fn validate(value: &Value) -> (usize, usize) {
    // ── declared component schema names ───────────────────────────────────
    let schema_keys: BTreeSet<String> = value
        .get("components")
        .and_then(|c| c.get("schemas"))
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .expect("OpenAPI doc must declare components.schemas");

    // ── $ref integrity: every schema ref must resolve to a real component ──
    let mut refs = Vec::new();
    collect_refs(value, &mut refs);
    assert!(!refs.is_empty(), "expected at least one $ref in the doc");

    let mut dangling: BTreeSet<String> = BTreeSet::new();
    for r in &refs {
        if let Some(name) = r.strip_prefix(SCHEMA_PREFIX) {
            if !schema_keys.contains(name) {
                dangling.insert(name.to_string());
            }
        } else {
            panic!("unexpected non-schema $ref form: `{r}`");
        }
    }
    assert!(
        dangling.is_empty(),
        "dangling $refs (not present in components.schemas): {dangling:?}"
    );

    // ── the bare generic must never leak in as a component key ─────────────
    assert!(
        !schema_keys.contains("ApiEnvelope"),
        "bare generic `ApiEnvelope` must never appear as a component key — only \
         the concrete `#[aliases(...)]` instantiations are registered"
    );

    // ── §D cardinality (catches a dropped handler OR a dropped path) ───────
    let paths = value
        .get("paths")
        .and_then(Value::as_object)
        .expect("OpenAPI doc must declare paths");
    let path_count = paths.len();
    assert_eq!(
        path_count,
        EXPECTED_PATHS,
        "expected exactly {EXPECTED_PATHS} path templates, found {path_count}: {:?}",
        paths.keys().collect::<Vec<_>>()
    );
    let operation_count: usize = paths
        .values()
        .filter_map(Value::as_object)
        .map(|item| {
            item.keys()
                .filter(|k| HTTP_METHODS.contains(&k.as_str()))
                .count()
        })
        .sum();
    assert_eq!(
        operation_count, EXPECTED_OPERATIONS,
        "expected exactly {EXPECTED_OPERATIONS} operations across all paths, \
         found {operation_count}"
    );

    (path_count, operation_count)
}

fn main() -> io::Result<()> {
    // Production L2+ surface only. ApiDocDev is debug-gated and excluded so the
    // committed artifact is build-profile-independent.
    let doc = ApiDoc::openapi();

    // Materialize once for validation, reusing the SAME serde_json model that
    // produces the pretty output below (so the gate inspects exactly what we
    // are about to write).
    let value: Value =
        serde_json::to_value(&doc).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let (path_count, operation_count) = validate(&value);

    // Pretty JSON = serde_json::to_string_pretty (2-space indent). Deterministic
    // because utoipa uses BTreeMaps and serde_json has no preserve_order.
    let mut json = serde_json::to_string_pretty(&value)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    json.push('\n'); // POSIX-clean trailing newline.

    let schema_count = value
        .get("components")
        .and_then(|c| c.get("schemas"))
        .and_then(Value::as_object)
        .map(|m| m.len())
        .unwrap_or(0);

    // CARGO_MANIFEST_DIR = .../crates/uc-webserver
    // repo root = manifest_dir/../.. (2 ancestors up); schema dir = root/schema
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .ancestors()
        .nth(2)
        .expect("manifest dir has 2 ancestors up to repo root");
    let schema_dir = repo_root.join("schema");
    let out_path = schema_dir.join("openapi.json");

    fs::create_dir_all(&schema_dir)?;
    fs::write(&out_path, &json)?;

    println!("gen-openapi: wrote {}", out_path.display());
    println!(
        "gen-openapi: {schema_count} component schemas, {path_count} path templates, \
         {operation_count} operations, {} bytes",
        json.len()
    );
    Ok(())
}
