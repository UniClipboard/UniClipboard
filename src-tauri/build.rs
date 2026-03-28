fn main() {
    // Stage daemon binary first so tauri_build::build() can validate externalBin paths.
    copy_daemon_binary_to_binaries();
    tauri_build::build();
}

/// Copies the compiled `uniclipboard-daemon` binary to `src-tauri/binaries/`
/// with the Tauri-required target-triple suffix for sidecar bundling.
///
/// Non-fatal if the daemon binary doesn't exist yet (first build or clean checkout).
fn copy_daemon_binary_to_binaries() {
    let target_triple = std::env::var("TAURI_ENV_TARGET_TRIPLE")
        .unwrap_or_else(|_| construct_triple_from_cfg());

    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"),
    );

    // Source: target/{profile}/uniclipboard-daemon
    let target_dir = manifest_dir.join("target").join(&profile);
    let src_name = if cfg!(target_os = "windows") {
        "uniclipboard-daemon.exe"
    } else {
        "uniclipboard-daemon"
    };
    let src = target_dir.join(src_name);

    // Destination: src-tauri/binaries/uniclipboard-daemon-{triple}[.exe]
    let binaries_dir = manifest_dir.join("binaries");
    if let Err(e) = std::fs::create_dir_all(&binaries_dir) {
        println!("cargo:warning=Failed to create binaries dir: {e}");
        return;
    }
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let dest_name = format!("uniclipboard-daemon-{target_triple}{ext}");
    let dest = binaries_dir.join(&dest_name);

    if src.exists() {
        match std::fs::copy(&src, &dest) {
            Ok(_) => println!("cargo:warning=Daemon binary staged to {}", dest.display()),
            Err(e) => println!("cargo:warning=Failed to copy daemon binary: {e}"),
        }
    } else {
        println!(
            "cargo:warning=Daemon binary not found at {} — build uc-daemon first",
            src.display()
        );
    }

    println!("cargo:rerun-if-changed={}", src.display());
}

fn construct_triple_from_cfg() -> String {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    match (arch.as_str(), os.as_str(), env.as_str()) {
        ("aarch64", "macos", _) => "aarch64-apple-darwin".to_string(),
        ("x86_64", "macos", _) => "x86_64-apple-darwin".to_string(),
        ("x86_64", "linux", "gnu") => "x86_64-unknown-linux-gnu".to_string(),
        ("aarch64", "linux", "gnu") => "aarch64-unknown-linux-gnu".to_string(),
        ("x86_64", "windows", "msvc") => "x86_64-pc-windows-msvc".to_string(),
        ("aarch64", "windows", "msvc") => "aarch64-pc-windows-msvc".to_string(),
        _ => format!("{arch}-unknown-{os}-{env}"),
    }
}
