//! Update one registered mobile device's editable management fields.
//!
//! The desktop daemon never stores plaintext passwords. A label-only edit can
//! keep credentials unchanged, but username changes need a fresh plaintext
//! password so the UI can show a scannable connect QR once. Password edits use
//! the same one-time echo rule as password rotation.

use std::{fmt, sync::Arc};

use tracing::{debug, instrument};

use uc_core::mobile_sync::{MintedCredentials, MobileDeviceError, MobileDeviceId};
use uc_core::ports::{
    MobileCredentialsMinterPort, MobileDeviceRepositoryPort, PasswordHasherError,
    PasswordHasherPort,
};

use super::register_device::{
    validate_label, validate_password_length, validate_username_shape,
    RegisterMobileShortcutDeviceError,
};

#[derive(Clone, PartialEq, Eq)]
pub enum MobileDevicePasswordEdit {
    Keep,
    AutoGenerate,
    Custom(String),
}

impl fmt::Debug for MobileDevicePasswordEdit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keep => f.write_str("Keep"),
            Self::AutoGenerate => f.write_str("AutoGenerate"),
            Self::Custom(_) => f.write_str("Custom([REDACTED])"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateMobileDeviceInput {
    pub device_id: MobileDeviceId,
    pub label: Option<String>,
    pub username: Option<String>,
    pub password: MobileDevicePasswordEdit,
}

#[derive(Clone)]
pub struct UpdateMobileDeviceOutput {
    pub device_id: MobileDeviceId,
    pub label: String,
    pub username: String,
    pub password: Option<String>,
}

// Manual Debug so the one-time plaintext password never reaches logs/traces.
// `password` is echoed to the caller exactly once; from then on it only exists
// as a server-side PHC hash. Mirror the `MobileDevicePasswordEdit` redaction.
impl fmt::Debug for UpdateMobileDeviceOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateMobileDeviceOutput")
            .field("device_id", &self.device_id)
            .field("label", &self.label)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateMobileDeviceError {
    #[error("device not found: {0}")]
    NotFound(MobileDeviceId),
    #[error("device label must not be empty")]
    LabelEmpty,
    #[error("device label too long (max 64 chars)")]
    LabelTooLong,
    #[error("username already taken: {0}")]
    UsernameTaken(String),
    #[error("username too short: must be at least {min} characters (got {got})")]
    UsernameTooShort { min: usize, got: usize },
    #[error("username too long: must be at most {max} characters (got {got})")]
    UsernameTooLong { max: usize, got: usize },
    #[error("username must start with an ASCII letter")]
    UsernameMustStartWithLetter,
    #[error("username contains forbidden characters (only letters, digits, underscore allowed)")]
    UsernameContainsForbiddenChars,
    #[error("password too short (min {min} chars)")]
    PasswordTooShort { min: usize },
    #[error("password too long (max {max} chars)")]
    PasswordTooLong { max: usize },
    #[error("password hashing failed: {0}")]
    PasswordHashFailed(String),
    #[error("device persistence failed: {0}")]
    PersistenceFailed(String),
}

pub(crate) struct UpdateMobileDeviceUseCase {
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    password_hasher: Arc<dyn PasswordHasherPort>,
    credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
}

impl UpdateMobileDeviceUseCase {
    pub(crate) fn new(
        device_repo: Arc<dyn MobileDeviceRepositoryPort>,
        password_hasher: Arc<dyn PasswordHasherPort>,
        credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
    ) -> Self {
        Self {
            device_repo,
            password_hasher,
            credentials_minter,
        }
    }

    #[instrument(
        skip(self, input),
        fields(
            label_changed = input.label.is_some(),
            username_changed = input.username.is_some(),
            password_mode = ?input.password,
        )
    )]
    pub(crate) async fn execute(
        &self,
        input: UpdateMobileDeviceInput,
    ) -> Result<UpdateMobileDeviceOutput, UpdateMobileDeviceError> {
        let mut device = self
            .device_repo
            .find_by_device_id(&input.device_id)
            .await
            .map_err(translate_device_error)?
            .ok_or_else(|| UpdateMobileDeviceError::NotFound(input.device_id.clone()))?;

        if let Some(label) = input.label {
            device.label = validate_label(label).map_err(map_register_validation)?;
        }

        let username_changed = match input.username {
            Some(username) => {
                let next = username.trim().to_string();
                validate_username_shape(&next).map_err(map_register_validation)?;
                if next != device.username {
                    self.ensure_username_available(&device.device_id, &next)
                        .await?;
                    device.username = next;
                    true
                } else {
                    false
                }
            }
            None => false,
        };

        let password_mode = if username_changed && input.password == MobileDevicePasswordEdit::Keep
        {
            MobileDevicePasswordEdit::AutoGenerate
        } else {
            input.password
        };

        // Each arm yields the one-time plaintext echo plus an optional
        // precomputed PHC hash. AutoGenerate reuses the minter's already-hashed
        // credential (the minter hashes once internally), so we must not run
        // Argon2 again over the same plaintext. Custom has no precomputed hash
        // and must be hashed below; Keep leaves credentials untouched.
        let (plaintext_password, precomputed_hash) = match password_mode {
            MobileDevicePasswordEdit::Keep => (None, None),
            MobileDevicePasswordEdit::AutoGenerate => {
                let MintedCredentials {
                    password,
                    password_hash,
                    ..
                } = self.credentials_minter.mint_credentials();
                (Some(password), Some(password_hash))
            }
            MobileDevicePasswordEdit::Custom(password) => {
                validate_password_length(&password).map_err(map_register_validation)?;
                (Some(password), None)
            }
        };

        if let Some(hash) = precomputed_hash {
            device.password_hash = hash;
        } else if let Some(password) = plaintext_password.as_ref() {
            device.password_hash = self
                .password_hasher
                .hash(password)
                .await
                .map_err(translate_hasher_error)?;
        }

        let updated = self
            .device_repo
            .update_mobile_device(&device)
            .await
            .map_err(|err| translate_update_error(err, &device.username))?;
        if !updated {
            debug!(
                device_id = %device.device_id,
                "device disappeared between find and update_mobile_device"
            );
            return Err(UpdateMobileDeviceError::NotFound(device.device_id));
        }

        Ok(UpdateMobileDeviceOutput {
            device_id: device.device_id,
            label: device.label,
            username: device.username,
            password: plaintext_password,
        })
    }

    async fn ensure_username_available(
        &self,
        current_id: &MobileDeviceId,
        username: &str,
    ) -> Result<(), UpdateMobileDeviceError> {
        match self.device_repo.find_by_username(username).await {
            Ok(Some(existing)) if existing.device_id != *current_id => {
                Err(UpdateMobileDeviceError::UsernameTaken(username.to_string()))
            }
            Ok(_) => Ok(()),
            Err(err) => Err(translate_device_error(err)),
        }
    }
}

fn map_register_validation(err: RegisterMobileShortcutDeviceError) -> UpdateMobileDeviceError {
    match err {
        RegisterMobileShortcutDeviceError::UsernameTaken(username) => {
            UpdateMobileDeviceError::UsernameTaken(username)
        }
        RegisterMobileShortcutDeviceError::UsernameTooShort { min, got } => {
            UpdateMobileDeviceError::UsernameTooShort { min, got }
        }
        RegisterMobileShortcutDeviceError::UsernameTooLong { max, got } => {
            UpdateMobileDeviceError::UsernameTooLong { max, got }
        }
        RegisterMobileShortcutDeviceError::UsernameMustStartWithLetter => {
            UpdateMobileDeviceError::UsernameMustStartWithLetter
        }
        RegisterMobileShortcutDeviceError::UsernameContainsForbiddenChars => {
            UpdateMobileDeviceError::UsernameContainsForbiddenChars
        }
        RegisterMobileShortcutDeviceError::PasswordTooShort { min } => {
            UpdateMobileDeviceError::PasswordTooShort { min }
        }
        RegisterMobileShortcutDeviceError::PasswordTooLong { max } => {
            UpdateMobileDeviceError::PasswordTooLong { max }
        }
        RegisterMobileShortcutDeviceError::PasswordHashFailed(message) => {
            UpdateMobileDeviceError::PasswordHashFailed(message)
        }
        RegisterMobileShortcutDeviceError::PersistenceFailed(message) => {
            UpdateMobileDeviceError::PersistenceFailed(message)
        }
        RegisterMobileShortcutDeviceError::LabelEmpty => UpdateMobileDeviceError::LabelEmpty,
        RegisterMobileShortcutDeviceError::LabelTooLong => UpdateMobileDeviceError::LabelTooLong,
        // Register-only variants. The update path never reaches the LAN / QR /
        // settings stages, so these are not expected here. Map each one
        // explicitly (instead of a catch-all) so adding a new validation
        // variant fails to compile rather than silently degrading to a 500.
        err @ (RegisterMobileShortcutDeviceError::LanListenerDisabled
        | RegisterMobileShortcutDeviceError::QrRenderFailed(_)
        | RegisterMobileShortcutDeviceError::SettingsLoadFailed(_)
        | RegisterMobileShortcutDeviceError::NoLanInterfaceAvailable
        | RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed(_)) => {
            UpdateMobileDeviceError::PersistenceFailed(err.to_string())
        }
    }
}

fn translate_device_error(err: MobileDeviceError) -> UpdateMobileDeviceError {
    match err {
        MobileDeviceError::UsernameCollision => {
            UpdateMobileDeviceError::UsernameTaken("username taken at save time".to_string())
        }
        MobileDeviceError::Storage(msg) => UpdateMobileDeviceError::PersistenceFailed(msg),
        MobileDeviceError::AlreadyExists(id) => {
            UpdateMobileDeviceError::PersistenceFailed(format!("device id collision: {id}"))
        }
    }
}

fn translate_update_error(err: MobileDeviceError, username: &str) -> UpdateMobileDeviceError {
    match err {
        MobileDeviceError::UsernameCollision => {
            UpdateMobileDeviceError::UsernameTaken(username.to_string())
        }
        other => translate_device_error(other),
    }
}

fn translate_hasher_error(err: PasswordHasherError) -> UpdateMobileDeviceError {
    match err {
        PasswordHasherError::InvalidPhc(msg) => {
            UpdateMobileDeviceError::PasswordHashFailed(format!("invalid phc: {msg}"))
        }
        PasswordHasherError::Internal(msg) => UpdateMobileDeviceError::PasswordHashFailed(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use mockall::predicate::eq;

    use uc_core::mobile_sync::{MintedCredentials, MobileClientType, MobileDevice, MobileDeviceId};

    use super::super::test_support::{MockDeviceRepo, MockHasher, MockMinter};

    fn fixture_device(id: &str, username: &str, label: &str) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new(id),
            label: label.into(),
            client_type: MobileClientType::IosShortcut,
            username: username.into(),
            password_hash: "phc:OLD".into(),
            created_at_ms: 1_000,
            last_seen_at_ms: Some(2_000),
            last_seen_ip: Some("192.168.1.6".into()),
            reported_name: Some("iPhone".into()),
            reported_os: Some("iOS".into()),
        }
    }

    /// Minter that emits `password` together with a deterministic PHC derived
    /// from it (`phc-minted:<password>`). The AutoGenerate path reuses this
    /// precomputed hash directly, so tests can assert on it without the hasher
    /// being invoked.
    fn minter_emitting(password: &'static str) -> MockMinter {
        let mut m = MockMinter::new();
        m.expect_mint_credentials()
            .returning(move || MintedCredentials {
                username: "mobile_unused".into(),
                password: password.into(),
                password_hash: format!("phc-minted:{password}"),
                device_id: MobileDeviceId::new("did_unused"),
            });
        m
    }

    fn identity_hasher_for(plaintext: &'static str) -> MockHasher {
        let mut h = MockHasher::new();
        h.expect_hash()
            .with(eq(plaintext))
            .returning(|p| Ok(format!("phc:{p}")));
        h
    }

    #[tokio::test]
    async fn updates_label_without_reissuing_credentials() {
        let device = fixture_device("did_x", "mobile_alice", "old phone");
        let device_id = device.device_id.clone();

        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id()
            .with(eq(device_id.clone()))
            .returning({
                let device = device.clone();
                move |_| Ok(Some(device.clone()))
            });
        repo.expect_find_by_username().never();
        repo.expect_update_mobile_device()
            .withf(|updated| {
                updated.label == "new phone"
                    && updated.username == "mobile_alice"
                    && updated.password_hash == "phc:OLD"
                    && updated.last_seen_at_ms == Some(2_000)
            })
            .returning(|_| Ok(true));
        let mut hasher = MockHasher::new();
        hasher.expect_hash().never();
        let mut minter = MockMinter::new();
        minter.expect_mint_credentials().never();

        let uc = UpdateMobileDeviceUseCase::new(Arc::new(repo), Arc::new(hasher), Arc::new(minter));

        let out = uc
            .execute(UpdateMobileDeviceInput {
                device_id: device_id.clone(),
                label: Some(" new phone ".into()),
                username: None,
                password: MobileDevicePasswordEdit::Keep,
            })
            .await
            .expect("ok");

        assert_eq!(out.device_id, device_id);
        assert_eq!(out.label, "new phone");
        assert_eq!(out.username, "mobile_alice");
        assert!(out.password.is_none());
    }

    #[tokio::test]
    async fn changing_username_with_keep_password_mints_new_password() {
        let device = fixture_device("did_x", "mobile_alice", "phone");
        let device_id = device.device_id.clone();

        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        repo.expect_find_by_username()
            .with(eq("mobile_bob".to_string()))
            .returning(|_| Ok(None));
        // AutoGenerate reuses the minter's precomputed PHC; the password hasher
        // must never be invoked over the minted plaintext (no redundant Argon2).
        repo.expect_update_mobile_device()
            .withf(|updated| {
                updated.label == "phone"
                    && updated.username == "mobile_bob"
                    && updated.password_hash == "phc-minted:minted-update-pw-22"
            })
            .returning(|_| Ok(true));
        let mut hasher = MockHasher::new();
        hasher.expect_hash().never();

        let uc = UpdateMobileDeviceUseCase::new(
            Arc::new(repo),
            Arc::new(hasher),
            Arc::new(minter_emitting("minted-update-pw-22")),
        );

        let out = uc
            .execute(UpdateMobileDeviceInput {
                device_id,
                label: None,
                username: Some(" mobile_bob ".into()),
                password: MobileDevicePasswordEdit::Keep,
            })
            .await
            .expect("ok");

        assert_eq!(out.username, "mobile_bob");
        assert_eq!(out.password.as_deref(), Some("minted-update-pw-22"));
    }

    #[tokio::test]
    async fn changing_password_keeps_username_and_returns_plaintext_once() {
        let device = fixture_device("did_x", "mobile_alice", "phone");
        let device_id = device.device_id.clone();

        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        repo.expect_find_by_username().never();
        repo.expect_update_mobile_device()
            .withf(|updated| {
                updated.username == "mobile_alice"
                    && updated.password_hash == "phc:brand-new-pass-42"
            })
            .returning(|_| Ok(true));
        let mut minter = MockMinter::new();
        minter.expect_mint_credentials().never();

        let uc = UpdateMobileDeviceUseCase::new(
            Arc::new(repo),
            Arc::new(identity_hasher_for("brand-new-pass-42")),
            Arc::new(minter),
        );

        let out = uc
            .execute(UpdateMobileDeviceInput {
                device_id,
                label: None,
                username: None,
                password: MobileDevicePasswordEdit::Custom("brand-new-pass-42".into()),
            })
            .await
            .expect("ok");

        assert_eq!(out.username, "mobile_alice");
        assert_eq!(out.password.as_deref(), Some("brand-new-pass-42"));
    }

    #[tokio::test]
    async fn rejects_username_taken_by_another_device() {
        let device = fixture_device("did_x", "mobile_alice", "phone");
        let other = fixture_device("did_y", "mobile_bob", "other");

        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        repo.expect_find_by_username()
            .with(eq("mobile_bob".to_string()))
            .returning({
                let other = other.clone();
                move |_| Ok(Some(other.clone()))
            });
        repo.expect_update_mobile_device().never();

        let uc = UpdateMobileDeviceUseCase::new(
            Arc::new(repo),
            Arc::new(identity_hasher_for("minted-update-pw-22")),
            Arc::new(minter_emitting("minted-update-pw-22")),
        );

        let err = uc
            .execute(UpdateMobileDeviceInput {
                device_id: device.device_id,
                label: None,
                username: Some("mobile_bob".into()),
                password: MobileDevicePasswordEdit::Keep,
            })
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            UpdateMobileDeviceError::UsernameTaken(username) if username == "mobile_bob"
        ));
    }

    #[tokio::test]
    async fn update_collision_reports_submitted_username() {
        let device = fixture_device("did_x", "mobile_alice", "phone");

        let mut repo = MockDeviceRepo::new();
        repo.expect_find_by_device_id().returning({
            let device = device.clone();
            move |_| Ok(Some(device.clone()))
        });
        repo.expect_find_by_username()
            .with(eq("mobile_bob".to_string()))
            .returning(|_| Ok(None));
        repo.expect_update_mobile_device()
            .returning(|_| Err(MobileDeviceError::UsernameCollision));

        let uc = UpdateMobileDeviceUseCase::new(
            Arc::new(repo),
            Arc::new(identity_hasher_for("minted-update-pw-22")),
            Arc::new(minter_emitting("minted-update-pw-22")),
        );

        let err = uc
            .execute(UpdateMobileDeviceInput {
                device_id: device.device_id,
                label: None,
                username: Some("mobile_bob".into()),
                password: MobileDevicePasswordEdit::Keep,
            })
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            UpdateMobileDeviceError::UsernameTaken(username) if username == "mobile_bob"
        ));
    }

    #[test]
    fn password_edit_debug_redacts_custom_plaintext() {
        let rendered = format!(
            "{:?}",
            MobileDevicePasswordEdit::Custom("must-not-appear-42".into())
        );

        assert!(!rendered.contains("must-not-appear-42"));
    }

    #[test]
    fn update_output_debug_redacts_plaintext_password() {
        let with_password = UpdateMobileDeviceOutput {
            device_id: MobileDeviceId::new("did_x"),
            label: "phone".into(),
            username: "mobile_alice".into(),
            password: Some("must-not-appear-42".into()),
        };
        let rendered = format!("{with_password:?}");
        assert!(!rendered.contains("must-not-appear-42"));
        assert!(rendered.contains("[REDACTED]"));

        let without_password = UpdateMobileDeviceOutput {
            password: None,
            ..with_password
        };
        let rendered_none = format!("{without_password:?}");
        assert!(rendered_none.contains("None"));
    }
}
