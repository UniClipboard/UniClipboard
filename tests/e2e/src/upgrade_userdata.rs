use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const FIXTURE_FORMAT_VERSION: u16 = 1;
const FIXTURE_ARCHIVE: &str = "userdata.tar.gz";
const MAX_FILE_COUNT: usize = 4096;
const MAX_FILE_SIZE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeUserdataManifest {
    format_version: u16,
    pub source_version: String,
    pub platform: String,
    pub scenario: String,
    pub environment: String,
    archive: String,
    archive_sha256: String,
    files: Vec<UpgradeUserdataFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpgradeUserdataFile {
    path: String,
    size: u64,
    sha256: String,
}

pub struct UpgradeUserdataFixture {
    manifest: UpgradeUserdataManifest,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl UpgradeUserdataFixture {
    pub fn load(directory: impl AsRef<Path>) -> Result<Self, String> {
        let directory = directory.as_ref();
        let manifest = std::fs::read(directory.join("manifest.json"))
            .map_err(|error| format!("read upgrade userdata manifest failed: {error}"))?;
        let archive = std::fs::read(directory.join(FIXTURE_ARCHIVE))
            .map_err(|error| format!("read upgrade userdata archive failed: {error}"))?;
        let (manifest, files) = verify_and_read_archive(&manifest, &archive)?;
        Ok(Self { manifest, files })
    }

    pub const fn manifest(&self) -> &UpgradeUserdataManifest {
        &self.manifest
    }

    pub fn restore_into(
        &self,
        data_root: &Path,
        cache_root: &Path,
        profile_name: &str,
    ) -> Result<(), String> {
        if profile_name.is_empty()
            || profile_name.starts_with('.')
            || !profile_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("upgrade userdata restore profile name is invalid".to_string());
        }
        ensure_absent(data_root)?;
        ensure_absent(cache_root)?;
        let result = (|| {
            for (path, bytes) in &self.files {
                let target = fixture_destination(path, data_root, cache_root, profile_name)?;
                let parent = target.parent().ok_or_else(|| {
                    format!("upgrade userdata path has no parent: {}", path.display())
                })?;
                std::fs::create_dir_all(parent).map_err(|error| {
                    format!("create upgrade userdata directory failed: {error}")
                })?;
                std::fs::write(&target, bytes)
                    .map_err(|error| format!("write upgrade userdata file failed: {error}"))?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(data_root);
            let _ = std::fs::remove_dir_all(cache_root);
        }
        result
    }
}

pub fn verify_upgrade_userdata_archive(
    manifest: &[u8],
    archive: &[u8],
) -> Result<UpgradeUserdataManifest, String> {
    verify_and_read_archive(manifest, archive).map(|(manifest, _)| manifest)
}

fn verify_and_read_archive(
    manifest_bytes: &[u8],
    archive: &[u8],
) -> Result<(UpgradeUserdataManifest, BTreeMap<PathBuf, Vec<u8>>), String> {
    let manifest: UpgradeUserdataManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|error| format!("decode upgrade userdata manifest failed: {error}"))?;
    manifest.validate()?;
    verify_sha256(
        "upgrade userdata archive",
        archive,
        &manifest.archive_sha256,
    )?;
    let files = read_archive_files(archive)?;
    manifest.verify_files(&files)?;
    Ok((manifest, files))
}

impl UpgradeUserdataManifest {
    fn validate(&self) -> Result<(), String> {
        if self.format_version != FIXTURE_FORMAT_VERSION
            || self.source_version.is_empty()
            || self.platform.is_empty()
            || self.scenario.is_empty()
            || self.environment != "development"
            || self.archive != FIXTURE_ARCHIVE
            || !is_sha256(&self.archive_sha256)
            || self.files.is_empty()
            || self.files.len() > MAX_FILE_COUNT
        {
            return Err("upgrade userdata manifest is invalid".to_string());
        }
        let mut paths = BTreeSet::new();
        let mut total_size = 0_u64;
        for file in &self.files {
            let path = validated_fixture_path(&file.path)?;
            if file.size > MAX_FILE_SIZE || !is_sha256(&file.sha256) || !paths.insert(path) {
                return Err("upgrade userdata file manifest is invalid".to_string());
            }
            total_size = total_size
                .checked_add(file.size)
                .ok_or_else(|| "upgrade userdata fixture is too large".to_string())?;
            if total_size > MAX_TOTAL_SIZE {
                return Err("upgrade userdata fixture is too large".to_string());
            }
        }
        Ok(())
    }

    fn verify_files(&self, files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<(), String> {
        if files.len() != self.files.len() {
            return Err("upgrade userdata archive file count mismatch".to_string());
        }
        for expected in &self.files {
            let path = validated_fixture_path(&expected.path)?;
            let bytes = files
                .get(&path)
                .ok_or_else(|| format!("upgrade userdata archive is missing {}", expected.path))?;
            if bytes.len() as u64 != expected.size {
                return Err(format!(
                    "upgrade userdata file size mismatch for {}",
                    expected.path
                ));
            }
            verify_sha256(&expected.path, bytes, &expected.sha256)?;
        }
        Ok(())
    }
}

fn read_archive_files(archive: &[u8]) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let decoder = GzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| format!("read upgrade userdata archive failed: {error}"))?;
    let mut files = BTreeMap::new();
    let mut total_size = 0_u64;
    for entry in entries {
        let mut entry =
            entry.map_err(|error| format!("read upgrade userdata entry failed: {error}"))?;
        if !entry.header().entry_type().is_file() {
            return Err("upgrade userdata archive contains a non-file entry".to_string());
        }
        let path = entry
            .path()
            .map_err(|error| format!("read upgrade userdata path failed: {error}"))?;
        let path = validated_fixture_path(path.to_string_lossy().as_ref())?;
        let size = entry.size();
        if size > MAX_FILE_SIZE || files.len() >= MAX_FILE_COUNT {
            return Err("upgrade userdata archive exceeds its limits".to_string());
        }
        total_size = total_size
            .checked_add(size)
            .ok_or_else(|| "upgrade userdata archive exceeds its limits".to_string())?;
        if total_size > MAX_TOTAL_SIZE {
            return Err("upgrade userdata archive exceeds its limits".to_string());
        }
        let mut bytes = Vec::with_capacity(size as usize);
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| format!("read upgrade userdata file failed: {error}"))?;
        if files.insert(path, bytes).is_some() {
            return Err("upgrade userdata archive contains a duplicate path".to_string());
        }
    }
    Ok(files)
}

fn validated_fixture_path(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("upgrade userdata archive path is invalid".to_string());
    }
    let mut components = path.components();
    let root = components.next().and_then(|component| match component {
        Component::Normal(value) => value.to_str(),
        _ => None,
    });
    if !matches!(
        root,
        Some("data" | "cache" | "legacy-iroh-identity" | "legacy-iroh-blobs")
    ) || components.next().is_none()
    {
        return Err("upgrade userdata archive path is outside allowed roots".to_string());
    }
    Ok(path.to_path_buf())
}

fn fixture_destination(
    path: &Path,
    data_root: &Path,
    cache_root: &Path,
    profile_name: &str,
) -> Result<PathBuf, String> {
    let mut components = path.components();
    let root = components.next().and_then(|component| match component {
        Component::Normal(value) => value.to_str(),
        _ => None,
    });
    let relative = components.as_path();
    match root {
        Some("data") => Ok(data_root.join(relative)),
        Some("cache") => Ok(cache_root.join(relative)),
        Some("legacy-iroh-identity") => Ok(data_root
            .join(format!("iroh-identity_{profile_name}"))
            .join(relative)),
        Some("legacy-iroh-blobs") => Ok(data_root
            .join(format!("iroh-blobs_{profile_name}"))
            .join(relative)),
        _ => Err("upgrade userdata archive path is outside allowed roots".to_string()),
    }
}

fn ensure_absent(path: &Path) -> Result<(), String> {
    if path.exists() {
        Err(format!(
            "upgrade userdata restore target already exists: {}",
            path.display()
        ))
    } else {
        Ok(())
    }
}

fn verify_sha256(label: &str, bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "SHA-256 mismatch for {label}: expected {expected}, got {actual}"
        ))
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
