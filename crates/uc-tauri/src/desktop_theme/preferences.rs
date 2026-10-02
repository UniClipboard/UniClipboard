use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct Preferences {
    follow_omarchy_theme: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            follow_omarchy_theme: true,
        }
    }
}

fn path() -> std::io::Result<std::path::PathBuf> {
    uc_app_paths::app_data_root()
        .map(|root| root.join("desktop-preferences.json"))
        .ok_or_else(|| std::io::Error::other("Desktop data directory unavailable"))
}

pub(super) fn load() -> std::io::Result<bool> {
    read(&path()?)
}

fn read(path: &Path) -> std::io::Result<bool> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice::<Preferences>(&bytes)?.follow_omarchy_theme),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(Preferences::default().follow_omarchy_theme)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn save(enabled: bool) -> std::io::Result<()> {
    write(&path()?, enabled)
}

fn write(path: &Path, enabled: bool) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Missing parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(
        &mut temporary,
        &Preferences {
            follow_omarchy_theme: enabled,
        },
    )?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("desktop-preferences.json");
        assert!(read(&path).unwrap());
        std::fs::write(&path, "{}").unwrap();
        assert!(read(&path).unwrap());
        write(&path, true).unwrap();
        assert!(read(&path).unwrap());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({"followOmarchyTheme": true}));
        write(&path, false).unwrap();
        assert!(!read(&path).unwrap());
    }

    #[test]
    fn reports_corruption_and_failed_save() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences");
        std::fs::write(&path, "invalid").unwrap();
        assert!(read(&path).is_err());
        assert!(write(&path.join("child"), true).is_err());
    }
}
