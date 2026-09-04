use std::path::PathBuf;

use uc_e2e_tests::{
    prepare_selected_upgrade_release_from, selected_upgrade_release, LEGACY_RELEASE_BASE_URL,
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let arguments = parse_arguments(std::env::args().skip(1))?;
    let release = selected_upgrade_release(&arguments.version)
        .ok_or_else(|| format!("upgrade release is not selected: {}", arguments.version))?;
    let output = arguments.output.unwrap_or(default_output_dir(release.tag)?);
    let binaries = prepare_selected_upgrade_release_from(
        LEGACY_RELEASE_BASE_URL,
        &output,
        release,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .await?;
    println!("upgrade_release={}", release.tag);
    println!("upgrade_release_dir={}", output.display());
    println!("upgrade_cli={}", binaries.cli.display());
    println!("upgrade_daemon={}", binaries.daemon.display());
    Ok(())
}

struct Arguments {
    version: String,
    output: Option<PathBuf>,
}

fn parse_arguments(mut args: impl Iterator<Item = String>) -> Result<Arguments, String> {
    let mut version = None;
    let mut output = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--version" => {
                version = Some(
                    args.next()
                        .ok_or_else(|| "--version requires a value".to_string())?,
                );
            }
            "--output" => {
                output =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "--output requires a directory".to_string()
                    })?));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: prepare-upgrade-release --version <version> [--output <directory>]"
                        .to_string(),
                );
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Arguments {
        version: version.ok_or_else(|| "--version is required".to_string())?,
        output,
    })
}

fn default_output_dir(tag: &str) -> Result<PathBuf, String> {
    dirs_next::cache_dir()
        .map(|root| {
            root.join("uniclipboard/e2e/releases")
                .join(tag)
                .join(format!(
                    "{}-{}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
        })
        .ok_or_else(|| "cannot resolve the user cache directory".to_string())
}
