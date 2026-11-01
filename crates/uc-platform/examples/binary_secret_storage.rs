//! Process E2E driver. Only run on the private session bus created by the runner.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    std::process::exit(77);
}

#[cfg(target_os = "linux")]
mod linux {
    use anyhow::{bail, ensure, Context};
    use dbus_secret_service::{EncryptionType, SecretService};
    use keyring::secret_service::SsCredential;
    use uc_platform::{
        ports::SecureStorageProvider,
        secure_storage::create_default_secure_storage_in_app_data_root,
        system_secure_storage::SystemSecureStorage,
    };

    fn payload(kind: &str) -> anyhow::Result<Vec<u8>> {
        Ok(match kind {
            "all-bytes" => (0..=255).collect(),
            "updated" => (0..=255).rev().chain([0, 255, 128, 0]).collect(),
            "empty" => vec![],
            "text" => b"synthetic legacy fixture".to_vec(),
            _ => bail!("unknown fixture"),
        })
    }

    pub fn run() -> anyhow::Result<()> {
        ensure!(
            std::env::var("UC_BINARY_SECRET_E2E_PRIVATE_BUS").as_deref() == Ok("1")
                && std::env::var("UC_PROFILE").as_deref() == Ok("binary-secret-e2e")
                && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
            "private E2E environment required"
        );
        let args: Vec<_> = std::env::args().collect();
        let operation = args.get(1).context("operation required")?.as_str();
        let key = args.get(2).map(String::as_str).unwrap_or("fixture");
        let kind = args.get(3).map(String::as_str).unwrap_or("all-bytes");
        let storage = SystemSecureStorage::new();
        let service = "UniClipboard-binary-secret-e2e";
        match operation {
            "write" => storage.set(key, &payload(kind)?)?,
            "read" => ensure!(storage.get(key)? == Some(payload(kind)?), "byte mismatch"),
            "delete" => storage.delete(key)?,
            "absent" => ensure!(storage.get(key)?.is_none(), "entry still exists"),
            "write-fails" => ensure!(
                storage.set(key, &payload(kind)?).is_err(),
                "write succeeded"
            ),
            "read-fails" => ensure!(storage.get(key).is_err(), "read succeeded"),
            "delete-fails" => ensure!(storage.delete(key).is_err(), "delete succeeded"),
            "write-denied" => ensure!(
                matches!(
                    storage.set(key, &payload(kind)?),
                    Err(uc_platform::ports::SecureStorageError::PermissionDenied(_))
                ),
                "expected permission denial"
            ),
            "read-denied" => ensure!(
                matches!(
                    storage.get(key),
                    Err(uc_platform::ports::SecureStorageError::PermissionDenied(_))
                ),
                "expected permission denial"
            ),
            "delete-denied" => ensure!(
                matches!(
                    storage.delete(key),
                    Err(uc_platform::ports::SecureStorageError::PermissionDenied(_))
                ),
                "expected permission denial"
            ),
            "selected" | "selected-fails" => {
                let root = std::env::var_os("UC_BINARY_SECRET_E2E_ROOT")
                    .context("profile root required")?;
                let selected = create_default_secure_storage_in_app_data_root(root.into())?;
                let result = selected.storage.get("__binary_secret_absent__");
                ensure!(
                    result.is_ok() == (operation == "selected"),
                    "probe result mismatch"
                );
            }
            _ => {
                let ss = SecretService::connect(EncryptionType::Dh)?;
                let collection = ss.get_default_collection()?;
                let credential = SsCredential::new_with_target(None, service, key)?;
                let mut attributes = credential.attributes.clone();
                match operation {
                    "seed" | "seed-no-target" | "seed-duplicate" => {
                        if operation == "seed-no-target" {
                            attributes.remove("target");
                        }
                        attributes.insert("e2e-sentinel".into(), "preserved".into());
                        let count = if operation == "seed-duplicate" { 2 } else { 1 };
                        for _ in 0..count {
                            let item = collection.create_item(
                                "synthetic legacy label",
                                attributes
                                    .iter()
                                    .map(|(k, v)| (k.as_str(), v.as_str()))
                                    .collect(),
                                &payload(kind)?,
                                false,
                                "text/plain",
                            )?;
                            let mut saved = item.get_attributes()?;
                            saved.insert("e2e-object-path".into(), item.path.to_string());
                            item.set_attributes(
                                saved
                                    .iter()
                                    .map(|(k, v)| (k.as_str(), v.as_str()))
                                    .collect(),
                            )?;
                        }
                    }
                    "inspect" | "inspect-legacy" | "inspect-no-target" | "inspect-duplicate" => {
                        let items = collection
                            .search_items([("service", service), ("username", key)].into())?;
                        let expected_count = if operation == "inspect-duplicate" {
                            2
                        } else {
                            1
                        };
                        ensure!(items.len() == expected_count, "item count mismatch");
                        for item in items {
                            let attrs = item.get_attributes()?;
                            if operation != "inspect" {
                                ensure!(
                                    attrs.get("e2e-object-path").map(String::as_str)
                                        == Some(item.path.as_ref()),
                                    "item replaced"
                                );
                                ensure!(
                                    attrs.get("e2e-sentinel").map(String::as_str)
                                        == Some("preserved"),
                                    "attribute changed"
                                );
                                ensure!(
                                    item.get_label()? == "synthetic legacy label",
                                    "label changed"
                                );
                            }
                            ensure!(
                                attrs.get("target").map(String::as_str)
                                    == if operation == "inspect-no-target" {
                                        None
                                    } else {
                                        Some("default")
                                    },
                                "target changed"
                            );
                            let expected_mime = if operation == "inspect-duplicate" {
                                "text/plain"
                            } else {
                                "application/octet-stream"
                            };
                            ensure!(
                                item.get_secret_content_type()? == expected_mime,
                                "content type mismatch"
                            );
                            ensure!(item.get_secret()? == payload(kind)?, "stored bytes changed");
                        }
                    }
                    _ => bail!("unknown operation"),
                }
            }
        }
        Ok(())
    }
}
