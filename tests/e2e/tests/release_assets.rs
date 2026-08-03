use uc_e2e_tests::{
    checksum_for_asset, extract_release_archive, fixed_legacy_release_asset,
    prepare_fixed_legacy_release_from, verify_release_payload, ArchiveFormat, NodeBinarySet,
    ReleaseAsset, LEGACY_RELEASE_BASE_URL, LEGACY_RELEASE_TAG, LEGACY_RELEASE_VERSION,
    LEGACY_SHA256SUMS_SHA256,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn legacy_release_identity_is_immutable() {
    assert_eq!(LEGACY_RELEASE_VERSION, "0.20.0-alpha.6");
    assert_eq!(LEGACY_RELEASE_TAG, "v0.20.0-alpha.6");
    assert_eq!(
        LEGACY_RELEASE_BASE_URL,
        "https://github.com/UniClipboard/UniClipboard/releases/download"
    );
    assert_eq!(
        LEGACY_SHA256SUMS_SHA256,
        "905876044fbe05128b155c6be35908ec214fd61eace1ee3a871d1f58585da4ca"
    );
}

#[test]
fn legacy_release_asset_matches_supported_host_targets() {
    let mac = fixed_legacy_release_asset("macos", "aarch64").expect("macOS ARM asset");
    assert_eq!(
        mac.filename,
        "uniclipboard-cli-0.20.0-alpha.6-aarch64-apple-darwin.tar.gz"
    );
    assert_eq!(mac.format, ArchiveFormat::TarGz);

    let mac_x64 = fixed_legacy_release_asset("macos", "x86_64").expect("macOS x64 asset");
    assert_eq!(
        mac_x64.filename,
        "uniclipboard-cli-0.20.0-alpha.6-x86_64-apple-darwin.tar.gz"
    );

    let linux = fixed_legacy_release_asset("linux", "x86_64").expect("Linux x64 asset");
    assert_eq!(
        linux.filename,
        "uniclipboard-cli-0.20.0-alpha.6-x86_64-unknown-linux-musl.tar.gz"
    );
    assert_eq!(linux.format, ArchiveFormat::TarGz);

    let linux_arm = fixed_legacy_release_asset("linux", "aarch64").expect("Linux ARM asset");
    assert_eq!(
        linux_arm.filename,
        "uniclipboard-cli-0.20.0-alpha.6-aarch64-unknown-linux-musl.tar.gz"
    );

    let windows = fixed_legacy_release_asset("windows", "x86_64").expect("Windows x64 asset");
    assert_eq!(
        windows.filename,
        "uniclipboard-cli-0.20.0-alpha.6-x86_64-pc-windows-msvc.zip"
    );
    assert_eq!(windows.format, ArchiveFormat::Zip);
}

#[test]
fn legacy_release_rejects_an_unpublished_target() {
    let error = fixed_legacy_release_asset("windows", "aarch64")
        .expect_err("the fixed release has no Windows ARM CLI archive");
    assert!(error.contains("unsupported legacy release target"));
}

#[test]
fn checksum_lookup_requires_an_exact_manifest_entry() {
    let manifest = concat!(
        "ffd37436526e817862727d18f215969acf631f04f0504909b69694c04f114323  ",
        "uniclipboard-cli-0.20.0-alpha.6-aarch64-apple-darwin.tar.gz\n",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  ",
        "other-uniclipboard-cli-0.20.0-alpha.6-aarch64-apple-darwin.tar.gz\n",
    );

    assert_eq!(
        checksum_for_asset(
            manifest,
            "uniclipboard-cli-0.20.0-alpha.6-aarch64-apple-darwin.tar.gz"
        )
        .expect("exact manifest entry"),
        "ffd37436526e817862727d18f215969acf631f04f0504909b69694c04f114323"
    );
    assert!(checksum_for_asset(manifest, "missing.tar.gz").is_err());
}

#[test]
fn release_payload_verification_rejects_tampered_inputs() {
    let archive = b"fixed archive";
    let archive_sha = sha256(archive);
    let manifest = format!("{archive_sha}  fixed.tar.gz\n");
    let manifest_sha = sha256(manifest.as_bytes());
    let asset = ReleaseAsset {
        filename: "fixed.tar.gz",
        format: ArchiveFormat::TarGz,
    };

    verify_release_payload(manifest.as_bytes(), &manifest_sha, archive, asset)
        .expect("matching release payload");
    assert!(verify_release_payload(manifest.as_bytes(), &"0".repeat(64), archive, asset).is_err());
    assert!(
        verify_release_payload(manifest.as_bytes(), &manifest_sha, b"tampered", asset).is_err()
    );
}

#[test]
fn tar_release_extracts_only_the_cli_pair() {
    let archive = tar_gz(&[
        ("uniclip", b"cli"),
        ("uniclipd", b"daemon"),
        ("ignored.txt", b"ignored"),
    ]);
    let output = tempfile::tempdir().expect("output tempdir");

    extract_release_archive(ArchiveFormat::TarGz, &archive, output.path())
        .expect("extract release archive");
    let binaries = NodeBinarySet::fixed_release_dir(LEGACY_RELEASE_VERSION, output.path())
        .expect("fixed binary pair");

    assert_eq!(std::fs::read(&binaries.cli).expect("read cli"), b"cli");
    assert_eq!(
        std::fs::read(&binaries.daemon).expect("read daemon"),
        b"daemon"
    );
    assert!(!output.path().join("ignored.txt").exists());
}

#[test]
fn zip_release_extracts_only_the_cli_pair() {
    let cli = binary_name("uniclip");
    let daemon = binary_name("uniclipd");
    let archive = zip_archive(&[
        (&cli, b"cli"),
        (&daemon, b"daemon"),
        ("nested/ignored.txt", b"ignored"),
    ]);
    let output = tempfile::tempdir().expect("output tempdir");

    extract_release_archive(ArchiveFormat::Zip, &archive, output.path())
        .expect("extract release archive");
    let binaries = NodeBinarySet::fixed_release_dir(LEGACY_RELEASE_VERSION, output.path())
        .expect("fixed binary pair");

    assert_eq!(std::fs::read(&binaries.cli).expect("read cli"), b"cli");
    assert_eq!(
        std::fs::read(&binaries.daemon).expect("read daemon"),
        b"daemon"
    );
    assert!(!output.path().join("nested").exists());
}

#[test]
fn fixed_release_dir_requires_both_binaries() {
    let output = tempfile::tempdir().expect("output tempdir");
    std::fs::write(output.path().join(binary_name("uniclip")), b"cli").expect("write cli");

    let error = NodeBinarySet::fixed_release_dir(LEGACY_RELEASE_VERSION, output.path())
        .expect_err("daemon binary is required");
    assert!(error.contains("uniclipd"));
}

#[tokio::test]
async fn release_preparer_downloads_verifies_and_promotes_atomically() {
    let asset = fixed_legacy_release_asset(std::env::consts::OS, std::env::consts::ARCH)
        .expect("host release asset");
    let cli = binary_name("uniclip");
    let daemon = binary_name("uniclipd");
    let archive = match asset.format {
        ArchiveFormat::TarGz => tar_gz(&[(&cli, b"cli"), (&daemon, b"daemon")]),
        ArchiveFormat::Zip => zip_archive(&[(&cli, b"cli"), (&daemon, b"daemon")]),
    };
    let archive_sha = sha256(&archive);
    let manifest = format!("{archive_sha}  {}\n", asset.filename);
    let manifest_sha = sha256(manifest.as_bytes());
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{LEGACY_RELEASE_TAG}/SHA256SUMS.txt")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(manifest.clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{LEGACY_RELEASE_TAG}/{}", asset.filename)))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
        .mount(&server)
        .await;
    let root = tempfile::tempdir().expect("cache root");
    let output = root.path().join("fixed-release");

    let binaries = prepare_fixed_legacy_release_from(
        &server.uri(),
        &output,
        std::env::consts::OS,
        std::env::consts::ARCH,
        &manifest_sha,
    )
    .await
    .expect("prepare fixed release");

    assert_eq!(std::fs::read(binaries.cli).expect("read cli"), b"cli");
    assert_eq!(
        std::fs::read(binaries.daemon).expect("read daemon"),
        b"daemon"
    );
    assert!(output.join("SHA256SUMS.txt").is_file());
    assert!(output.join(asset.filename).is_file());
}

#[tokio::test]
async fn release_preparer_leaves_no_output_after_checksum_failure() {
    let asset = fixed_legacy_release_asset(std::env::consts::OS, std::env::consts::ARCH)
        .expect("host release asset");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes("tampered"))
        .mount(&server)
        .await;
    let root = tempfile::tempdir().expect("cache root");
    let output = root.path().join("fixed-release");

    let result = prepare_fixed_legacy_release_from(
        &server.uri(),
        &output,
        std::env::consts::OS,
        std::env::consts::ARCH,
        LEGACY_SHA256SUMS_SHA256,
    )
    .await;

    assert!(result.is_err());
    assert!(!output.exists());
    assert!(root
        .path()
        .read_dir()
        .expect("read cache root")
        .next()
        .is_none());
    assert!(!asset.filename.is_empty());
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;

    let mut encoded = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut encoded, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (name, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            archive
                .append_data(&mut header, name, *body)
                .expect("append tar entry");
        }
        let encoder = archive.into_inner().expect("finish tar");
        encoder.finish().expect("finish gzip");
    }
    encoded.flush().expect("flush archive");
    encoded
}

fn binary_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn zip_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::{Cursor, Write};

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, body) in entries {
        archive.start_file(name, options).expect("start zip entry");
        archive.write_all(body).expect("write zip entry");
    }
    archive.finish().expect("finish zip").into_inner()
}
