mod blob_cipher_adapter;
pub mod crypto_model;
mod decrypting_clipboard_event_repo;
mod decrypting_representation_repo;
mod default_current_profile;
mod encrypted_blob_store;
mod encrypting_clipboard_event_writer;
mod hashing;
mod identity_fingerprint;
mod key_material;
mod key_migration_adapter;
mod scope_identifier;
mod secrets;
mod session;
mod space_access_adapter;
pub(crate) mod v1_aead;

pub use blob_cipher_adapter::BlobCipherAdapter;
pub use crypto_model::{
    EncryptedBlob, KdfParams, KdfParamsV1, KeyScope, KeySlot, KeySlotConvertError, KeySlotFile,
    WrappedMasterKey,
};
pub use decrypting_clipboard_event_repo::DecryptingClipboardEventRepository;
pub use decrypting_representation_repo::DecryptingClipboardRepresentationRepository;
pub use default_current_profile::DefaultCurrentProfile;
pub use encrypted_blob_store::EncryptedBlobStore;
pub use encrypting_clipboard_event_writer::EncryptingClipboardEventWriter;
pub use hashing::{hash_pin, verify_pin, Argon2PinHasher, Blake3Hasher};
pub use identity_fingerprint::{
    FingerprintDerivationError, Sha256IdentityFingerprintFactory, Sha256ShortCodeGenerator,
    ShortCodeGenerator,
};
pub use key_material::KeyMaterialStore;
pub use key_migration_adapter::DefaultKeyMigrationAdapter;
pub(crate) use secrets::MasterKey;
pub use session::InMemorySession;
pub use space_access_adapter::DefaultSpaceAccessAdapter;
