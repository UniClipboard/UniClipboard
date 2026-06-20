//! Single source of truth for *which* secure-storage entries travel inside a
//! configuration bundle.
//!
//! The secure-storage key strings (`iroh-identity:v1`, `kek:v1:profile:{id}`)
//! are disk-compatibility invariants that must not leak into the domain layer.
//! This module concentrates the enumeration so the export side does not
//! open-code key names and a future addition (or rename) has exactly one place
//! to change.
//!
//! Scope rule (locked decision): only the **current** profile's KEK is carried.
//! The identity key is profile-independent and always carried.

use crate::network::iroh::identity_store::IDENTITY_STORE_KEY;

/// Name of the secrets member inside the bundle archive.
pub const SECRETS_MEMBER: &str = "secrets.json";

/// A secure-storage entry the bundle carries.
///
/// `required` distinguishes the device identity (whose absence defeats the
/// whole point of the migration) from the KEK (re-derivable from the
/// passphrase, so its absence only means "unlock will be required after
/// apply").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratableSecretKey {
    /// Secure-storage key string.
    pub key: String,
    /// Whether the migration is meaningless without this entry.
    pub required: bool,
    /// Classification used by the staging step to decide whether unlock will be
    /// required after apply.
    pub kind: MigratableSecretKind,
}

/// Classification of a migratable secret, independent of its concrete key
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigratableSecretKind {
    /// The long-term device identity key. Not re-derivable; its presence is the
    /// reason the target machine keeps the same network identity.
    DeviceIdentity,
    /// The key-encryption key for the current profile. Re-derivable from the
    /// passphrase, so optional.
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
    vec![
        MigratableSecretKey {
            key: IDENTITY_STORE_KEY.to_string(),
            required: true,
            kind: MigratableSecretKind::DeviceIdentity,
        },
        MigratableSecretKey {
            key: profile_kek_key(profile_id),
            required: false,
            kind: MigratableSecretKind::ProfileKek,
        },
    ]
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
    fn enumerates_identity_then_current_profile_kek() {
        let keys = migratable_secret_keys("default");
        assert_eq!(keys.len(), 2);

        assert_eq!(keys[0].key, "iroh-identity:v1");
        assert!(keys[0].required);
        assert_eq!(keys[0].kind, MigratableSecretKind::DeviceIdentity);

        assert_eq!(keys[1].key, "kek:v1:profile:default");
        assert!(!keys[1].required);
        assert_eq!(keys[1].kind, MigratableSecretKind::ProfileKek);
    }

    #[test]
    fn kek_key_tracks_profile_id() {
        let keys = migratable_secret_keys("alice");
        assert_eq!(keys[1].key, "kek:v1:profile:alice");
    }
}
