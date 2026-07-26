use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const DEVICE_ID_FILE: &str = "device_id.txt";

pub struct LocalDeviceIdentity {
    device_id: String,
}

impl LocalDeviceIdentity {
    pub fn load_or_create(config_dir: PathBuf) -> Result<Self> {
        let device_id = match load_device_id_from_disk(&config_dir)? {
            Some(device_id) => device_id,
            None => {
                let device_id = uuid::Uuid::new_v4().to_string();
                save_device_id_to_disk(&config_dir, &device_id)?;
                device_id
            }
        };
        Ok(Self { device_id })
    }

    pub fn current_device_id(&self) -> &str {
        &self.device_id
    }
}

fn load_device_id_from_disk(config_dir: &Path) -> Result<Option<String>> {
    let path = config_dir.join(DEVICE_ID_FILE);
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("read device_id file failed: {}", path.display()))?;
    let device_id = content.trim();
    if device_id.is_empty() {
        return Ok(None);
    }

    uuid::Uuid::parse_str(device_id)
        .with_context(|| format!("invalid device_id UUID in file: {}", path.display()))?;
    Ok(Some(device_id.to_string()))
}

fn save_device_id_to_disk(config_dir: &Path, device_id: &str) -> Result<()> {
    std::fs::create_dir_all(config_dir)
        .with_context(|| format!("create config dir failed: {}", config_dir.display()))?;

    let path = config_dir.join(DEVICE_ID_FILE);
    let tmp_path = path.with_extension("txt.tmp");
    std::fs::write(&tmp_path, device_id)
        .with_context(|| format!("write temp device_id failed: {}", tmp_path.display()))?;

    match std::fs::rename(&tmp_path, &path) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            std::fs::write(&path, device_id).with_context(|| {
                format!(
                    "direct write device_id failed after rename error ({rename_error}): {}",
                    path.display()
                )
            })?;
            let _ = std::fs::remove_file(&tmp_path);
            Ok(())
        }
    }
}
