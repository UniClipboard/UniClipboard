use dbus_secret_service::{EncryptionType, SecretService};
use keyring::secret_service::{create_collection, decode_error, get_collection, SsCredential};

const BINARY_CONTENT_TYPE: &str = "application/octet-stream";

/// Preserve keyring's matching, legacy lookup, uniqueness and unlock behavior,
/// while declaring raw bytes correctly on both update and creation.
pub(super) fn set_secret(service: &str, key: &str, value: &[u8]) -> keyring::Result<()> {
    let credential = SsCredential::new_with_target(None, service, key)?;
    match credential.map_matching_items(
        |item| {
            item.set_secret(value, BINARY_CONTENT_TYPE)
                .map_err(decode_error)
        },
        true,
    ) {
        Ok(_) => return Ok(()),
        Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(error),
    }

    let ss = SecretService::connect(EncryptionType::Dh)
        .map_err(|error| keyring::Error::PlatformFailure(Box::new(error)))?;
    // Keep keyring's default collection resolution and error behavior.
    let collection =
        get_collection(&ss, "default").or_else(|_| create_collection(&ss, "default"))?;
    collection
        .create_item(
            &credential.label,
            credential
                .attributes
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect(),
            value,
            true,
            BINARY_CONTENT_TYPE,
        )
        .map_err(|error| keyring::Error::PlatformFailure(Box::new(error)))?;
    Ok(())
}
