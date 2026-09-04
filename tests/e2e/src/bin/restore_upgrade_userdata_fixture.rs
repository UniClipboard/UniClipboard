use std::path::PathBuf;

use uc_e2e_tests::{TestProfile, UpgradeUserdataFixture};

fn main() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let mut fixture = None;
    let mut profile = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--fixture" => fixture = arguments.next().map(PathBuf::from),
            "--profile" => profile = arguments.next(),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    let fixture = fixture.ok_or_else(|| "--fixture is required".to_string())?;
    let profile_name = profile.ok_or_else(|| "--profile is required".to_string())?;
    let profile = TestProfile::for_upgrade_fixture(&profile_name)?;
    profile.cleanup();
    UpgradeUserdataFixture::load(fixture)?.restore_into(
        profile.data_dir(),
        profile.cache_dir(),
        &profile.name,
    )?;
    println!("upgrade_fixture_profile={}", profile.name);
    std::mem::forget(profile);
    Ok(())
}
