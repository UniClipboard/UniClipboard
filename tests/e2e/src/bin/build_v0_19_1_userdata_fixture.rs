use std::path::PathBuf;

use uc_e2e_tests::{build_single_node_upgrade_fixture, V0_19_1_UPGRADE_RELEASE};

#[tokio::main]
async fn main() -> Result<(), String> {
    let release_directory = std::env::var_os("UC_E2E_V0_19_1_RELEASE_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "UC_E2E_V0_19_1_RELEASE_DIR is required".to_string())?;
    let output = build_single_node_upgrade_fixture(
        &release_directory,
        &V0_19_1_UPGRADE_RELEASE,
        "v0-19-1-upgrade-fixture-passphrase",
    )
    .await?;
    println!("fixture_directory={}", output.display());
    Ok(())
}
