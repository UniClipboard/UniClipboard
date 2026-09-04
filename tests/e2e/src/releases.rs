use std::io::Cursor;
use std::path::{Component, Path};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::{DaemonEndpointDiscovery, NodeBinarySet};

pub const LEGACY_RELEASE_VERSION: &str = "0.20.0-alpha.6";
pub const LEGACY_RELEASE_TAG: &str = "v0.20.0-alpha.6";
pub const LEGACY_RELEASE_BASE_URL: &str =
    "https://github.com/UniClipboard/UniClipboard/releases/download";
pub const LEGACY_SHA256SUMS_SHA256: &str =
    "905876044fbe05128b155c6be35908ec214fd61eace1ee3a871d1f58585da4ca";
pub const V0_19_1_RELEASE_VERSION: &str = "0.19.1";
pub const V0_19_1_RELEASE_TAG: &str = "v0.19.1";
pub const V0_19_1_SHA256SUMS_SHA256: &str =
    "c572d2c25f98f6bdf18216a33e88a5dcce0c77dba06b0283855def77650f55af";

pub const V0_19_1_UPGRADE_RELEASE: UpgradeRelease = UpgradeRelease {
    version: V0_19_1_RELEASE_VERSION,
    tag: V0_19_1_RELEASE_TAG,
    manifest_sha256: V0_19_1_SHA256SUMS_SHA256,
    macos_aarch64_asset: ReleaseAsset {
        filename: "uniclipboard-cli-0.19.1-aarch64-apple-darwin.tar.gz",
        format: ArchiveFormat::TarGz,
    },
    macos_aarch64_asset_sha256: "df8966aa0b462c69de8a304a99f71228ea2c9119fa3a327a33bb48df9b27b4e6",
    endpoint_discovery: DaemonEndpointDiscovery::FixedProfilePort,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpgradeRelease {
    pub version: &'static str,
    pub tag: &'static str,
    pub manifest_sha256: &'static str,
    pub macos_aarch64_asset: ReleaseAsset,
    pub macos_aarch64_asset_sha256: &'static str,
    pub endpoint_discovery: DaemonEndpointDiscovery,
}

pub const UPGRADE_RELEASES: &[UpgradeRelease] = &[
    UpgradeRelease {
        version: "0.20.0-alpha.2",
        tag: "v0.20.0-alpha.2",
        manifest_sha256: "c86f90b2f6ae46c07445f0e17c0e7245bc8958ae81d01dcbd0f4d9e3398a422c",
        macos_aarch64_asset: ReleaseAsset {
            filename: "uniclipboard-cli-0.20.0-alpha.2-aarch64-apple-darwin.tar.gz",
            format: ArchiveFormat::TarGz,
        },
        macos_aarch64_asset_sha256:
            "898694075738ad0c23461b10dead4c0473bc8a89e743b965a20a234116f0a32c",
        endpoint_discovery: DaemonEndpointDiscovery::FixedProfilePort,
    },
    UpgradeRelease {
        version: "0.20.0-alpha.6",
        tag: "v0.20.0-alpha.6",
        manifest_sha256: LEGACY_SHA256SUMS_SHA256,
        macos_aarch64_asset: ReleaseAsset {
            filename: "uniclipboard-cli-0.20.0-alpha.6-aarch64-apple-darwin.tar.gz",
            format: ArchiveFormat::TarGz,
        },
        macos_aarch64_asset_sha256:
            "ffd37436526e817862727d18f215969acf631f04f0504909b69694c04f114323",
        endpoint_discovery: DaemonEndpointDiscovery::FixedProfilePort,
    },
    UpgradeRelease {
        version: "1.0.0-alpha.4",
        tag: "v1.0.0-alpha.4",
        manifest_sha256: "fd10e8b3e4d2c5d07fbcddd9748a532d0cd2635baf29d24a1ac74f5d81a430ff",
        macos_aarch64_asset: ReleaseAsset {
            filename: "uniclipboard-cli-1.0.0-alpha.4-aarch64-apple-darwin.tar.gz",
            format: ArchiveFormat::TarGz,
        },
        macos_aarch64_asset_sha256:
            "fb016f703b45be1cfb633db4680ac9207f65f599f4a0e6b5c77ee2800fdd048e",
        endpoint_discovery: DaemonEndpointDiscovery::ConnectionFile,
    },
];

pub fn selected_upgrade_release(version: &str) -> Option<&'static UpgradeRelease> {
    UPGRADE_RELEASES
        .iter()
        .find(|release| release.version == version)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub filename: &'static str,
    pub format: ArchiveFormat,
}

pub fn fixed_legacy_release_asset(os: &str, arch: &str) -> Result<ReleaseAsset, String> {
    let (filename, format) = match (os, arch) {
        ("macos", "aarch64") => (
            "uniclipboard-cli-0.20.0-alpha.6-aarch64-apple-darwin.tar.gz",
            ArchiveFormat::TarGz,
        ),
        ("macos", "x86_64") => (
            "uniclipboard-cli-0.20.0-alpha.6-x86_64-apple-darwin.tar.gz",
            ArchiveFormat::TarGz,
        ),
        ("linux", "aarch64") => (
            "uniclipboard-cli-0.20.0-alpha.6-aarch64-unknown-linux-musl.tar.gz",
            ArchiveFormat::TarGz,
        ),
        ("linux", "x86_64") => (
            "uniclipboard-cli-0.20.0-alpha.6-x86_64-unknown-linux-musl.tar.gz",
            ArchiveFormat::TarGz,
        ),
        ("windows", "x86_64") => (
            "uniclipboard-cli-0.20.0-alpha.6-x86_64-pc-windows-msvc.zip",
            ArchiveFormat::Zip,
        ),
        _ => return Err(format!("unsupported legacy release target: {os}/{arch}")),
    };
    Ok(ReleaseAsset { filename, format })
}

pub fn v0_19_1_release_asset(os: &str, arch: &str) -> Result<ReleaseAsset, String> {
    let (filename, format) = match (os, arch) {
        ("macos", "aarch64") => (
            "uniclipboard-cli-0.19.1-aarch64-apple-darwin.tar.gz",
            ArchiveFormat::TarGz,
        ),
        ("macos", "x86_64") => (
            "uniclipboard-cli-0.19.1-x86_64-apple-darwin.tar.gz",
            ArchiveFormat::TarGz,
        ),
        ("linux", "aarch64") => (
            "uniclipboard-cli-0.19.1-aarch64-unknown-linux-musl.tar.gz",
            ArchiveFormat::TarGz,
        ),
        ("linux", "x86_64") => (
            "uniclipboard-cli-0.19.1-x86_64-unknown-linux-musl.tar.gz",
            ArchiveFormat::TarGz,
        ),
        ("windows", "x86_64") => (
            "uniclipboard-cli-0.19.1-x86_64-pc-windows-msvc.zip",
            ArchiveFormat::Zip,
        ),
        _ => return Err(format!("unsupported v0.19.1 release target: {os}/{arch}")),
    };
    Ok(ReleaseAsset { filename, format })
}

pub fn checksum_for_asset(manifest: &str, filename: &str) -> Result<String, String> {
    manifest
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let checksum = fields.next()?;
            let entry = fields.next()?.trim_start_matches('*');
            (entry == filename).then(|| checksum.to_string())
        })
        .next()
        .ok_or_else(|| format!("checksum manifest does not contain {filename}"))
}

pub fn verify_release_payload(
    manifest: &[u8],
    expected_manifest_sha256: &str,
    archive: &[u8],
    asset: ReleaseAsset,
) -> Result<(), String> {
    verify_sha256("SHA256SUMS.txt", manifest, expected_manifest_sha256)?;
    let manifest = std::str::from_utf8(manifest)
        .map_err(|error| format!("SHA256SUMS.txt is not UTF-8: {error}"))?;
    let expected_archive_sha256 = checksum_for_asset(manifest, asset.filename)?;
    verify_sha256(asset.filename, archive, &expected_archive_sha256)
}

pub fn extract_release_archive(
    format: ArchiveFormat,
    archive: &[u8],
    output_dir: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(output_dir)
        .map_err(|error| format!("create release directory failed: {error}"))?;
    match format {
        ArchiveFormat::TarGz => extract_tar_gz(archive, output_dir),
        ArchiveFormat::Zip => extract_zip(archive, output_dir),
    }
}

pub async fn prepare_fixed_legacy_release_from(
    release_base_url: &str,
    output_dir: &Path,
    os: &str,
    arch: &str,
    expected_manifest_sha256: &str,
) -> Result<NodeBinarySet, String> {
    let asset = fixed_legacy_release_asset(os, arch)?;
    prepare_release_from(
        release_base_url,
        output_dir,
        LEGACY_RELEASE_VERSION,
        LEGACY_RELEASE_TAG,
        asset,
        expected_manifest_sha256,
        DaemonEndpointDiscovery::FixedProfilePort,
    )
    .await
}

pub async fn prepare_v0_19_1_release_from(
    release_base_url: &str,
    output_dir: &Path,
    os: &str,
    arch: &str,
    expected_manifest_sha256: &str,
) -> Result<NodeBinarySet, String> {
    let asset = v0_19_1_release_asset(os, arch)?;
    prepare_release_from(
        release_base_url,
        output_dir,
        V0_19_1_RELEASE_VERSION,
        V0_19_1_RELEASE_TAG,
        asset,
        expected_manifest_sha256,
        DaemonEndpointDiscovery::FixedProfilePort,
    )
    .await
}

pub async fn prepare_selected_upgrade_release_from(
    release_base_url: &str,
    output_dir: &Path,
    release: &UpgradeRelease,
    os: &str,
    arch: &str,
) -> Result<NodeBinarySet, String> {
    if (os, arch) != ("macos", "aarch64") {
        return Err(format!(
            "unsupported upgrade fixture release target: {os}/{arch}"
        ));
    }
    prepare_release_from(
        release_base_url,
        output_dir,
        release.version,
        release.tag,
        release.macos_aarch64_asset,
        release.manifest_sha256,
        release.endpoint_discovery,
    )
    .await
}

async fn prepare_release_from(
    release_base_url: &str,
    output_dir: &Path,
    version: &str,
    tag: &str,
    asset: ReleaseAsset,
    expected_manifest_sha256: &str,
    endpoint_discovery: DaemonEndpointDiscovery,
) -> Result<NodeBinarySet, String> {
    if output_dir.exists() {
        return load_verified_release(
            output_dir,
            version,
            asset,
            expected_manifest_sha256,
            endpoint_discovery,
        );
    }

    let client = reqwest::Client::builder()
        .user_agent(format!("uc-e2e-tests/{version}"))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("build release download client failed: {error}"))?;
    let base_url = format!("{}/{}", release_base_url.trim_end_matches('/'), tag);
    let manifest = download(&client, &format!("{base_url}/SHA256SUMS.txt")).await?;
    let archive = download(&client, &format!("{base_url}/{}", asset.filename)).await?;
    verify_release_payload(&manifest, expected_manifest_sha256, &archive, asset)?;

    let parent = output_dir
        .parent()
        .ok_or_else(|| format!("release output has no parent: {}", output_dir.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create release cache root failed: {error}"))?;
    let staging = tempfile::Builder::new()
        .prefix(".legacy-release-")
        .tempdir_in(parent)
        .map_err(|error| format!("create release staging directory failed: {error}"))?;
    std::fs::write(staging.path().join("SHA256SUMS.txt"), &manifest)
        .map_err(|error| format!("write checksum manifest failed: {error}"))?;
    std::fs::write(staging.path().join(asset.filename), &archive)
        .map_err(|error| format!("write release archive failed: {error}"))?;
    extract_release_archive(asset.format, &archive, staging.path())?;
    NodeBinarySet::fixed_release_dir_with_discovery(version, staging.path(), endpoint_discovery)?;

    let staging_path = staging.keep();
    if let Err(error) = std::fs::rename(&staging_path, output_dir) {
        let _ = std::fs::remove_dir_all(&staging_path);
        return Err(format!("promote verified release failed: {error}"));
    }
    NodeBinarySet::fixed_release_dir_with_discovery(version, output_dir, endpoint_discovery)
}

fn load_verified_release(
    output_dir: &Path,
    version: &str,
    asset: ReleaseAsset,
    expected_manifest_sha256: &str,
    endpoint_discovery: DaemonEndpointDiscovery,
) -> Result<NodeBinarySet, String> {
    let manifest = std::fs::read(output_dir.join("SHA256SUMS.txt"))
        .map_err(|error| format!("read cached checksum manifest failed: {error}"))?;
    let archive = std::fs::read(output_dir.join(asset.filename))
        .map_err(|error| format!("read cached release archive failed: {error}"))?;
    verify_release_payload(&manifest, expected_manifest_sha256, &archive, asset)?;
    NodeBinarySet::fixed_release_dir_with_discovery(version, output_dir, endpoint_discovery)
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("download {url} failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("download {url} failed: {error}"))?;
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| format!("read {url} failed: {error}"))
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

fn extract_tar_gz(archive: &[u8], output_dir: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(archive));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| format!("read tar archive failed: {error}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("read tar entry failed: {error}"))?;
        let path = entry
            .path()
            .map_err(|error| format!("read tar entry path failed: {error}"))?;
        let Some(name) = accepted_binary_name(&path).map(str::to_string) else {
            continue;
        };
        entry
            .unpack(output_dir.join(&name))
            .map_err(|error| format!("extract {name} failed: {error}"))?;
    }
    Ok(())
}

fn extract_zip(archive: &[u8], output_dir: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive))
        .map_err(|error| format!("read zip archive failed: {error}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("read zip entry failed: {error}"))?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = accepted_binary_name(&path).map(str::to_string) else {
            continue;
        };
        let destination = output_dir.join(&name);
        let mut file = std::fs::File::create(&destination)
            .map_err(|error| format!("create {} failed: {error}", destination.display()))?;
        std::io::copy(&mut entry, &mut file)
            .map_err(|error| format!("extract {name} failed: {error}"))?;
    }
    Ok(())
}

fn accepted_binary_name(path: &Path) -> Option<&str> {
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    let accepted = if cfg!(windows) {
        ["uniclip.exe", "uniclipd.exe"].contains(&name)
    } else {
        ["uniclip", "uniclipd"].contains(&name)
    };
    accepted.then_some(name)
}
