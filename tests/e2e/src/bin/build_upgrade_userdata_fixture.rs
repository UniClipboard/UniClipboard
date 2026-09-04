use std::path::PathBuf;

use uc_e2e_tests::{build_single_node_upgrade_fixture, selected_upgrade_release};

#[tokio::main]
async fn main() -> Result<(), String> {
    let arguments = parse_arguments(std::env::args().skip(1))?;
    let release = selected_upgrade_release(&arguments.version)
        .ok_or_else(|| format!("upgrade release is not selected: {}", arguments.version))?;
    let output = build_single_node_upgrade_fixture(
        &arguments.release_directory,
        release,
        "upgrade-fixture-passphrase",
    )
    .await?;
    println!("fixture_directory={}", output.display());
    Ok(())
}

struct Arguments {
    version: String,
    release_directory: PathBuf,
}

fn parse_arguments(mut args: impl Iterator<Item = String>) -> Result<Arguments, String> {
    let mut version = None;
    let mut release_directory = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--version" => {
                version = Some(
                    args.next()
                        .ok_or_else(|| "--version requires a value".to_string())?,
                );
            }
            "--release-dir" => {
                release_directory =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "--release-dir requires a directory".to_string()
                    })?));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: build-upgrade-userdata-fixture --version <version> --release-dir <directory>"
                        .to_string(),
                );
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Arguments {
        version: version.ok_or_else(|| "--version is required".to_string())?,
        release_directory: release_directory
            .ok_or_else(|| "--release-dir is required".to_string())?,
    })
}
