//! # Spool Directory Cleanup Tests (BLOB-04)
//!
//! These tests verify the sentinel-based one-time spool directory cleanup
//! that runs during app bootstrap. Old JSON-format blob files written before
//! the V2 binary format migration must be purged on the first startup after
//! upgrade (when `.v2_migrated` sentinel is absent), but must NOT be purged
//! on subsequent startups (when sentinel is present).
//!
//! ## Requirement: BLOB-04
//! Old blobs wiped on upgrade via sentinel-based one-time spool directory cleanup.
//!
//! ## Implementation Location
//! `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — inline logic inside
//! `create_platform_layer`, triggered from `wire_dependencies_with_identity_store`.
//!
//! ## Path Resolution
//! When `config.vault_key_path = {tmp}/vault/key` and `config.database_path = ":memory:"`:
//!   - vault_dir  = {tmp}/vault
//!   - blob_store = {tmp}/vault/blobs
//!   - sentinel   = {tmp}/vault/blobs/.v2_migrated

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::sync::mpsc;
use uc_core::config::AppConfig;
use uc_platform::ports::{IdentityStoreError, IdentityStorePort};
use uc_tauri::bootstrap::wiring::wire_dependencies_with_identity_store;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MemoryIdentityStore {
    identity: Mutex<Option<Vec<u8>>>,
}

impl IdentityStorePort for MemoryIdentityStore {
    fn load_identity(&self) -> Result<Option<Vec<u8>>, IdentityStoreError> {
        let guard = self
            .identity
            .lock()
            .map_err(|_| IdentityStoreError::Store("poisoned".to_string()))?;
        Ok(guard.clone())
    }

    fn store_identity(&self, identity: &[u8]) -> Result<(), IdentityStoreError> {
        let mut guard = self
            .identity
            .lock()
            .map_err(|_| IdentityStoreError::Store("poisoned".to_string()))?;
        *guard = Some(identity.to_vec());
        Ok(())
    }
}

fn test_identity_store() -> Arc<dyn IdentityStorePort> {
    Arc::new(MemoryIdentityStore::default())
}

/// Build a minimal AppConfig that points vault and DB paths into `base_dir`.
///
/// - database_path = {base_dir}/test.db   (real file; SQLite creates it)
/// - vault_key_path = {base_dir}/vault/key  (need not exist)
///
/// With these settings the wiring code resolves:
///   app_data_root = {base_dir}
///   vault_dir     = {base_dir}/vault
///   blob_store    = {base_dir}/vault/blobs
///   sentinel      = {base_dir}/vault/blobs/.v2_migrated
fn make_config(base_dir: &std::path::Path) -> AppConfig {
    let mut config = AppConfig::empty();
    config.database_path = base_dir.join("test.db");
    config.vault_key_path = base_dir.join("vault").join("key");
    config
}

/// Returns the expected blob store directory for a config built with `make_config`.
fn blob_dir(base_dir: &std::path::Path) -> PathBuf {
    base_dir.join("vault").join("blobs")
}

/// Returns the expected sentinel path for a config built with `make_config`.
fn sentinel_path(base_dir: &std::path::Path) -> PathBuf {
    blob_dir(base_dir).join(".v2_migrated")
}

/// Write dummy "old-format" blob files into the spool dir to simulate
/// the presence of pre-V2-migration blobs.
fn seed_old_blobs(blobs_dir: &std::path::Path, names: &[&str]) {
    fs::create_dir_all(blobs_dir).expect("failed to create blobs dir");
    for name in names {
        let path = blobs_dir.join(name);
        let mut f = fs::File::create(&path).expect("failed to create old blob file");
        f.write_all(b"{\"version\":\"v1\",\"data\":\"base64encoded\"}")
            .expect("failed to write old blob content");
    }
}

/// Call `wire_dependencies_with_identity_store` and discard the result.
/// We only care about the filesystem side-effects (blob cleanup).
/// The function may fail for other reasons (e.g. clipboard init on CI) —
/// we intentionally ignore all errors; the filesystem effects still happen
/// because the cleanup runs before the clipboard init.
fn trigger_wiring(config: &AppConfig) {
    let (cmd_tx, _cmd_rx) = mpsc::channel(10);
    let _ = wire_dependencies_with_identity_store(config, cmd_tx, Some(test_identity_store()));
}

// ---------------------------------------------------------------------------
// BLOB-04 — Scenario A
// When blob dir exists and sentinel is ABSENT: all files deleted, sentinel created
// ---------------------------------------------------------------------------

/// BLOB-04-A: First startup after upgrade purges old blob files and creates sentinel.
///
/// Given:
///   - blob_store_dir exists with old JSON-format files
///   - sentinel (.v2_migrated) is NOT present
///
/// When:
///   - bootstrap wiring runs
///
/// Then:
///   - all regular files in blob_store_dir are deleted
///   - sentinel file (.v2_migrated) is created
#[test]
fn blob_cleanup_purges_old_files_and_creates_sentinel_when_no_sentinel() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let base = temp.path();
    let config = make_config(base);
    let blobs = blob_dir(base);
    let sentinel = sentinel_path(base);

    // Seed old blob files BEFORE the sentinel exists.
    seed_old_blobs(&blobs, &["abc123.blob", "def456.blob", "ghi789.blob"]);
    assert!(!sentinel.exists(), "sentinel must not exist before wiring");
    assert_eq!(
        count_regular_files(&blobs),
        3,
        "three old blobs should exist before wiring"
    );

    trigger_wiring(&config);

    // Sentinel must have been created.
    assert!(
        sentinel.exists(),
        "sentinel (.v2_migrated) must be created after cleanup"
    );

    // All old blob files must be gone.
    for name in &["abc123.blob", "def456.blob", "ghi789.blob"] {
        let path = blobs.join(name);
        assert!(
            !path.exists(),
            "old blob '{}' should have been deleted by first-boot cleanup",
            name
        );
    }

    // Only the sentinel itself should remain (count = 1).
    assert_eq!(
        count_regular_files(&blobs),
        1,
        "only the sentinel file should remain after cleanup (got {} files)",
        count_regular_files(&blobs)
    );
}

// ---------------------------------------------------------------------------
// BLOB-04 — Scenario B
// When blob dir exists and sentinel IS present: no files deleted (idempotent)
// ---------------------------------------------------------------------------

/// BLOB-04-B: Subsequent startups do NOT re-purge blob files.
///
/// Given:
///   - blob_store_dir exists with V2-format files
///   - sentinel (.v2_migrated) IS already present (from a previous startup)
///
/// When:
///   - bootstrap wiring runs again
///
/// Then:
///   - V2 blob files are preserved (not deleted)
///   - sentinel remains in place
#[test]
fn blob_cleanup_is_idempotent_when_sentinel_already_exists() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let base = temp.path();
    let config = make_config(base);
    let blobs = blob_dir(base);
    let sentinel = sentinel_path(base);

    // Create sentinel FIRST (simulating that a previous run already migrated).
    fs::create_dir_all(&blobs).expect("failed to create blobs dir");
    fs::File::create(&sentinel).expect("failed to create sentinel");

    // Place "new V2" blob files — these must survive.
    let v2_blobs = &["v2blob_001.ucbl", "v2blob_002.ucbl"];
    for name in v2_blobs {
        let path = blobs.join(name);
        let mut f = fs::File::create(&path).expect("failed to create V2 blob");
        // Write a fake UCBL header (magic bytes) to represent a valid V2 blob.
        f.write_all(&[0x55, 0x43, 0x42, 0x4C, 0x01])
            .expect("failed to write V2 blob content");
    }

    assert_eq!(
        count_regular_files(&blobs),
        3, // sentinel + 2 v2 blobs
        "sentinel + 2 V2 blobs should exist before second wiring"
    );

    trigger_wiring(&config);

    // V2 blobs must still be present.
    for name in v2_blobs {
        let path = blobs.join(name);
        assert!(
            path.exists(),
            "V2 blob '{}' should not be deleted on subsequent startup",
            name
        );
    }

    // Sentinel must still exist.
    assert!(
        sentinel.exists(),
        "sentinel must remain after subsequent startup"
    );
}

// ---------------------------------------------------------------------------
// BLOB-04 — Scenario C
// When blob dir does NOT exist: no error (graceful no-op)
// ---------------------------------------------------------------------------

/// BLOB-04-C: Wiring succeeds gracefully when blob dir does not exist.
///
/// Given:
///   - blob_store_dir does NOT exist
///
/// When:
///   - bootstrap wiring runs
///
/// Then:
///   - no panic or crash
///   - wiring either succeeds or fails for unrelated reasons (e.g. clipboard)
///   - crucially, no error is returned specifically because of the missing blob dir
#[test]
fn blob_cleanup_is_graceful_noop_when_blob_dir_absent() {
    let temp = TempDir::new().expect("failed to create temp dir");
    let base = temp.path();
    let config = make_config(base);
    let blobs = blob_dir(base);

    // Verify blob dir does NOT exist before wiring.
    assert!(
        !blobs.exists(),
        "blob dir must not exist before wiring for this test to be meaningful"
    );

    // This must not panic. We don't assert on Ok/Err because other parts of
    // wiring (clipboard init, network init) may fail in CI — the blob-cleanup
    // path itself must be panic-free.
    let result = std::panic::catch_unwind(|| {
        trigger_wiring(&config);
    });

    assert!(
        result.is_ok(),
        "wiring must not panic when blob dir does not exist"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count regular (non-directory) files in `dir`, ignoring any errors.
fn count_regular_files(dir: &std::path::Path) -> usize {
    if !dir.exists() {
        return 0;
    }
    fs::read_dir(dir)
        .map(|entries| entries.flatten().filter(|e| e.path().is_file()).count())
        .unwrap_or(0)
}
