use std::path::PathBuf;

use uc_e2e_tests::{
    prepare_v0_19_1_release_from, LEGACY_RELEASE_BASE_URL, V0_19_1_RELEASE_TAG,
    V0_19_1_SHA256SUMS_SHA256,
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let output = parse_output(std::env::args().skip(1))?;
    let binaries = prepare_v0_19_1_release_from(
        LEGACY_RELEASE_BASE_URL,
        &output,
        std::env::consts::OS,
        std::env::consts::ARCH,
        V0_19_1_SHA256SUMS_SHA256,
    )
    .await?;
    println!("legacy_release={V0_19_1_RELEASE_TAG}");
    println!("legacy_release_dir={}", output.display());
    println!("legacy_cli={}", binaries.cli.display());
    println!("legacy_daemon={}", binaries.daemon.display());
    Ok(())
}

fn parse_output(mut args: impl Iterator<Item = String>) -> Result<PathBuf, String> {
    let mut output = default_output_dir()?;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--output" => {
                output = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--output requires a directory".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: prepare-v0-19-1-release [--output <directory>]".to_string());
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(output)
}

fn default_output_dir() -> Result<PathBuf, String> {
    dirs_next::cache_dir()
        .map(|root| {
            root.join("uniclipboard/e2e/releases")
                .join(V0_19_1_RELEASE_TAG)
                .join(format!(
                    "{}-{}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
        })
        .ok_or_else(|| "cannot resolve the user cache directory".to_string())
}
