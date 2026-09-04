use std::path::{Path, PathBuf};

use flate2::{Compression, GzBuilder};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uc_e2e_tests::{
    v0_19_1_release_asset, verify_release_payload, NodeBinarySet, TestCli, TestDaemon, TestProfile,
    V0_19_1_SHA256SUMS_SHA256,
};

const PASSPHRASE: &str = "v0-19-1-upgrade-fixture-passphrase";
const DEVICE_NAME: &str = "fixture-v0191-single-node";
const SOURCE_ASSET_SHA256: &str =
    "df8966aa0b462c69de8a304a99f71228ea2c9119fa3a327a33bb48df9b27b4e6";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format_version: u16,
    source_version: &'static str,
    source_asset_sha256: &'static str,
    platform: &'static str,
    scenario: &'static str,
    environment: &'static str,
    archive: &'static str,
    archive_sha256: String,
    files: Vec<ManifestFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
}

#[tokio::main]
async fn main() -> Result<(), String> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("this fixture builder requires macOS aarch64".to_string());
    }
    let release_directory = std::env::var_os("UC_E2E_V0_19_1_RELEASE_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "UC_E2E_V0_19_1_RELEASE_DIR is required".to_string())?;
    let asset = v0_19_1_release_asset("macos", "aarch64")?;
    let checksum_manifest = std::fs::read(release_directory.join("SHA256SUMS.txt"))
        .map_err(|error| format!("read v0.19.1 checksum manifest failed: {error}"))?;
    let release_archive = std::fs::read(release_directory.join(asset.filename))
        .map_err(|error| format!("read v0.19.1 release archive failed: {error}"))?;
    verify_release_payload(
        &checksum_manifest,
        V0_19_1_SHA256SUMS_SHA256,
        &release_archive,
        asset,
    )?;
    let binaries = NodeBinarySet::fixed_release_dir("0.19.1", release_directory)?;
    let profile = TestProfile::new_v0_19_1_upgrade("fixture-single-node-empty");
    if !profile.name.starts_with("dev-upgrade-v0191-") {
        return Err("fixture generation requires a development profile".to_string());
    }
    let mut daemon = TestDaemon::start_clean_with(profile, &binaries, None).await?;
    let cli = TestCli::with_binaries(&daemon.profile, &binaries);
    let initialized = cli.run_capture(&[
        "init",
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        DEVICE_NAME,
    ]);
    if !initialized.success() {
        return Err(format!(
            "v0.19.1 fixture initialization failed: {}",
            initialized.stderr
        ));
    }
    daemon.stop_gracefully().await?;

    let database = daemon.profile.data_dir().join("uniclipboard.db");
    let database_size = std::fs::metadata(&database)
        .map_err(|error| format!("inspect fixture database failed: {error}"))?
        .len();
    let wal_size = std::fs::metadata(daemon.profile.data_dir().join("uniclipboard.db-wal"))
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if database_size <= 4096 && wal_size == 0 {
        return Err("fixture database contains neither a checkpoint nor a WAL".to_string());
    }
    disable_fixture_telemetry(daemon.profile.data_dir())?;

    let files = collect_fixture_files(daemon.profile.data_dir(), daemon.profile.cache_dir())?;
    let archive = build_archive(&files)?;
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/upgrades/v0.19.1/macos-aarch64/single-node-empty");
    std::fs::create_dir_all(&output)
        .map_err(|error| format!("create fixture output failed: {error}"))?;
    std::fs::write(output.join("userdata.tar.gz"), &archive)
        .map_err(|error| format!("write fixture archive failed: {error}"))?;
    let manifest = Manifest {
        format_version: 1,
        source_version: "0.19.1",
        source_asset_sha256: SOURCE_ASSET_SHA256,
        platform: "macos-aarch64",
        scenario: "single-node-empty",
        environment: "development",
        archive: "userdata.tar.gz",
        archive_sha256: sha256(&archive),
        files: files
            .iter()
            .map(|(path, bytes)| ManifestFile {
                path: path.to_string_lossy().replace('\\', "/"),
                size: bytes.len() as u64,
                sha256: sha256(bytes),
            })
            .collect(),
    };
    let mut manifest = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("encode fixture manifest failed: {error}"))?;
    manifest.push(b'\n');
    std::fs::write(output.join("manifest.json"), manifest)
        .map_err(|error| format!("write fixture manifest failed: {error}"))?;
    println!("fixture_directory={}", output.display());
    Ok(())
}

fn disable_fixture_telemetry(data_root: &Path) -> Result<(), String> {
    let path = data_root.join("settings.json");
    let bytes =
        std::fs::read(&path).map_err(|error| format!("read fixture settings failed: {error}"))?;
    let mut settings: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("decode fixture settings failed: {error}"))?;
    let general = settings
        .get_mut("general")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| "fixture settings have no general object".to_string())?;
    general.insert("telemetry_enabled".to_string(), false.into());
    general.insert("usage_analytics_enabled".to_string(), false.into());
    let mut bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|error| format!("encode fixture settings failed: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes).map_err(|error| format!("write fixture settings failed: {error}"))
}

fn collect_fixture_files(
    data_root: &Path,
    cache_root: &Path,
) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let mut files = Vec::new();
    for path in [
        "uniclipboard.db",
        "uniclipboard.db-wal",
        "settings.json",
        "upgrade-cursor.json",
        ".engine-upgrade-cursor.json",
        "vault/.setup_status",
        "vault/keyslot.json",
        "vault/device_id.txt",
    ] {
        collect_file_if_present(data_root, Path::new(path), Path::new("data"), &mut files)?;
    }
    for directory in ["keyring"] {
        collect_directory(
            data_root,
            Path::new(directory),
            Path::new("data"),
            &mut files,
        )?;
    }
    collect_directory(
        &data_root.join(format!("iroh-identity_{}", daemon_profile_name(data_root)?)),
        Path::new(""),
        Path::new("legacy-iroh-identity"),
        &mut files,
    )?;
    collect_directory(
        &data_root.join(format!("iroh-blobs_{}", daemon_profile_name(data_root)?)),
        Path::new(""),
        Path::new("legacy-iroh-blobs"),
        &mut files,
    )?;
    if cache_root.is_dir() {
        collect_directory(cache_root, Path::new(""), Path::new("cache"), &mut files)?;
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    for required in [
        "data/uniclipboard.db",
        "data/vault/.setup_status",
        "data/vault/keyslot.json",
        "data/vault/device_id.txt",
    ] {
        if !files.iter().any(|(path, _)| path == Path::new(required)) {
            return Err(format!("generated fixture is missing {required}"));
        }
    }
    for required_root in ["legacy-iroh-identity", "legacy-iroh-blobs"] {
        if !files
            .iter()
            .any(|(path, _)| path.starts_with(required_root))
        {
            return Err(format!("generated fixture is missing {required_root}"));
        }
    }
    Ok(files)
}

fn daemon_profile_name(data_root: &Path) -> Result<&str, String> {
    data_root
        .file_name()
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_prefix("app.uniclipboard.desktop-"))
        .ok_or_else(|| "fixture data root does not contain a profile name".to_string())
}

fn collect_directory(
    root: &Path,
    relative: &Path,
    archive_root: &Path,
    files: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<(), String> {
    let directory = root.join(relative);
    if !directory.is_dir() {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(&directory)
        .map_err(|error| format!("read fixture directory failed: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read fixture entry failed: {error}"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("read fixture file type failed: {error}"))?;
        let child = relative.join(entry.file_name());
        if file_type.is_dir() {
            collect_directory(root, &child, archive_root, files)?;
        } else if file_type.is_file() {
            collect_file_if_present(root, &child, archive_root, files)?;
        } else {
            return Err(format!(
                "fixture source contains a link or special file: {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn collect_file_if_present(
    root: &Path,
    relative: &Path,
    archive_root: &Path,
    files: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<(), String> {
    let source = root.join(relative);
    if !source.is_file() {
        return Ok(());
    }
    let bytes = std::fs::read(&source)
        .map_err(|error| format!("read fixture source file failed: {error}"))?;
    files.push((archive_root.join(relative), bytes));
    Ok(())
}

fn build_archive(files: &[(PathBuf, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::best());
    let mut archive = tar::Builder::new(encoder);
    for (path, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o600);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        archive
            .append_data(&mut header, path, bytes.as_slice())
            .map_err(|error| format!("append fixture archive file failed: {error}"))?;
    }
    let encoder = archive
        .into_inner()
        .map_err(|error| format!("finish fixture tar failed: {error}"))?;
    encoder
        .finish()
        .map_err(|error| format!("finish fixture gzip failed: {error}"))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
