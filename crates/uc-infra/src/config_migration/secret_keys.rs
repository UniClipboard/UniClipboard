//! Single source of truth for *which* secure-storage entries travel inside a
//! configuration bundle's `secrets.json`.
//!
//! The secure-storage key strings (`kek:v1:profile:{id}`) are disk-compatibility
//! invariants that must not leak into the domain layer. This module
//! concentrates the enumeration so the export side does not open-code key names
//! and a future addition (or rename) has exactly one place to change.
//!
//! Scope rule (locked decision): only the **current** profile's KEK is carried.
//!
//! The iroh device identity is deliberately *not* enumerated here. It is not a
//! credential-store secret — production wiring persists it as `0600` files in a
//! dedicated directory (see `IROH_IDENTITY_PREFIX` in [`super::staging`]), so it
//! migrates as files, not as a `secrets.json` entry.

/// Name of the secrets member inside the bundle archive.
pub const SECRETS_MEMBER: &str = "secrets.json";

/// A secure-storage entry the bundle carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratableSecretKey {
    /// Secure-storage key string.
    pub key: String,
    /// Classification of what this entry is.
    pub kind: MigratableSecretKind,
}

/// Classification of a migratable secret, independent of its concrete key
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigratableSecretKind {
    /// The key-encryption key for the current profile. Re-derivable from the
    /// passphrase, so its presence only decides whether unlock is required
    /// after apply.
    ProfileKek,
}

/// Build the centralized list of secure-storage entries to migrate for
/// `profile_id`.
///
/// The KEK key string is built from the same `kek:v1:profile:{id}` convention
/// the key-material store uses; keeping it here (rather than reaching across
/// modules) is intentional so the migration manifest of keys lives in one
/// place.
pub fn migratable_secret_keys(profile_id: &str) -> Vec<MigratableSecretKey> {
    vec![MigratableSecretKey {
        key: profile_kek_key(profile_id),
        kind: MigratableSecretKind::ProfileKek,
    }]
}

/// Compose the secure-storage key for the current profile's KEK.
///
/// Mirrors `KeyMaterialStore`'s `kek:v1:{scope_identifier}` layout where the
/// scope identifier is `profile:{profile_id}`. This is a disk-compatibility
/// invariant — changing it would orphan existing installs' KEK entries.
fn profile_kek_key(profile_id: &str) -> String {
    format!("kek:v1:profile:{profile_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_only_current_profile_kek() {
        let keys = migratable_secret_keys("default");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key, "kek:v1:profile:default");
        assert_eq!(keys[0].kind, MigratableSecretKind::ProfileKek);
    }

    #[test]
    fn kek_key_tracks_profile_id() {
        let keys = migratable_secret_keys("alice");
        assert_eq!(keys[0].key, "kek:v1:profile:alice");
    }
}
