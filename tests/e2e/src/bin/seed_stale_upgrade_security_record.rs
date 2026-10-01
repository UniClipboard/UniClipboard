//! Seeds an isolated synthetic profile with the stale upgrade-backup security record shape: a
//! `security-current` file exists in the profile's upgrade-backup directory while the record
//! key is absent from the profile keyring. The file content is synthetic and never a real key.

use uc_e2e_tests::TestProfile;

const RECORD_KEY: &str = "profile_upgrade_backup_record_key:v1";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let mut profile = None;
    let mut shared_directory = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--profile" => profile = arguments.next(),
            // Leaves the backup directory readable by others, which the daemon rejects with an
            // ordinary, retryable startup failure instead of the missing-key failure.
            "--shared-directory" => shared_directory = true,
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    let profile_name = profile.ok_or_else(|| "--profile is required".to_string())?;
    let profile = TestProfile::for_upgrade_fixture(&profile_name)?;
    let key_file = profile
        .data_dir()
        .join("keyring")
        .join(format!("{}.bin", hex(RECORD_KEY.as_bytes())));
    if key_file.exists() {
        return Err("the synthetic profile must not already hold the record key".to_string());
    }
    let directory = profile.upgrade_backup_dir().join(
        blake3::hash(profile.data_dir().as_os_str().as_encoded_bytes())
            .to_hex()
            .as_str(),
    );
    // The daemon rejects backup directories shared with other users, so mirror its private modes.
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(if shared_directory { 0o755 } else { 0o700 });
    }
    builder
        .create(&directory)
        .map_err(|error| error.to_string())?;
    if shared_directory {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
                .map_err(|error| error.to_string())?;
        }
        println!("shared_backup_directory_profile={}", profile.name);
        std::mem::forget(profile);
        return Ok(());
    }
    let record = directory.join("security-current");
    std::fs::write(&record, b"synthetic stale security record")
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    println!("stale_security_record_profile={}", profile.name);
    std::mem::forget(profile);
    Ok(())
}
