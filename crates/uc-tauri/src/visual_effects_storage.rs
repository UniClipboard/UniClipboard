use crate::visual_effects::{AutoResult, EffectsMode, EvidenceClass};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const POLICY_VERSION: u32 = 2;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StoredEffects {
    pub schema_version: u32,
    pub policy_version: u32,
    pub mode: EffectsMode,
    pub next_auto: Option<AutoResult>,
    pub evidence_class: EvidenceClass,
}

impl Default for StoredEffects {
    fn default() -> Self {
        Self {
            schema_version: 1,
            policy_version: POLICY_VERSION,
            mode: EffectsMode::Auto,
            next_auto: None,
            evidence_class: EvidenceClass::Unknown,
        }
    }
}

pub struct EffectsStorage {
    path: Option<PathBuf>,
    writable: bool,
}

impl EffectsStorage {
    pub fn load(path: Option<PathBuf>) -> (Self, StoredEffects) {
        let loaded = path.as_deref().map(read).transpose();
        match loaded {
            Ok(Some(stored)) => (
                Self {
                    path,
                    writable: true,
                },
                stored,
            ),
            Ok(None) => (
                Self {
                    path,
                    writable: false,
                },
                StoredEffects::default(),
            ),
            Err(error) => {
                tracing::warn!(error_kind = "visual_effects_read", source = %error, "visual preferences unavailable; preserving file");
                (
                    Self {
                        path,
                        writable: false,
                    },
                    StoredEffects::default(),
                )
            }
        }
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn save(&self, stored: &StoredEffects) -> std::io::Result<()> {
        let path = self
            .path
            .as_deref()
            .filter(|_| self.writable)
            .ok_or_else(|| std::io::Error::other("visual preferences are read-only"))?;
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("missing preferences directory"))?;
        fs::create_dir_all(parent)?;
        // NamedTempFile::persist atomically replaces an existing file on supported OSes.
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut temporary, stored)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn read(path: &Path) -> std::io::Result<StoredEffects> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StoredEffects::default())
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(std::io::Error::other("preferences too large"));
    }
    let stored: StoredEffects = serde_json::from_slice(&bytes)?;
    if stored.schema_version != 1 {
        return Err(std::io::Error::other("unsupported preferences version"));
    }
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visual_effects_replace_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("visual-effects.json");
        let (storage, mut value) = EffectsStorage::load(Some(path.clone()));
        storage.save(&value).unwrap();
        value.mode = EffectsMode::Effects;
        storage.save(&value).unwrap();
        assert_eq!(
            EffectsStorage::load(Some(path)).1.mode,
            EffectsMode::Effects
        );
    }
    #[test]
    fn visual_effects_preserve_invalid_files() {
        for bytes in [
            b"{".to_vec(),
            vec![b' '; 4097],
            br#"{"schemaVersion":2}"#.to_vec(),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("visual-effects.json");
            fs::write(&path, &bytes).unwrap();
            let (storage, value) = EffectsStorage::load(Some(path.clone()));
            assert!(!storage.writable());
            assert!(storage.save(&value).is_err());
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}
