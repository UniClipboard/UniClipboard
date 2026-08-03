use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct NodeBinarySet {
    pub version: String,
    pub cli: PathBuf,
    pub daemon: PathBuf,
}

impl NodeBinarySet {
    pub fn current() -> Self {
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
        let cli_name = if cfg!(windows) {
            "uniclip.exe"
        } else {
            "uniclip"
        };
        let daemon_name = if cfg!(windows) {
            "uniclipd.exe"
        } else {
            "uniclipd"
        };
        Self {
            version: "current".to_string(),
            cli: target_dir.join("debug").join(cli_name),
            daemon: target_dir.join("debug").join(daemon_name),
        }
    }

    pub fn fixed(
        version: impl Into<String>,
        cli: impl Into<PathBuf>,
        daemon: impl Into<PathBuf>,
    ) -> Self {
        Self {
            version: version.into(),
            cli: cli.into(),
            daemon: daemon.into(),
        }
    }

    pub fn fixed_release_dir(
        version: impl Into<String>,
        directory: impl AsRef<Path>,
    ) -> Result<Self, String> {
        let directory = directory.as_ref();
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let binaries = Self::fixed(
            version,
            directory.join(format!("uniclip{suffix}")),
            directory.join(format!("uniclipd{suffix}")),
        );
        for path in [&binaries.cli, &binaries.daemon] {
            if !path.is_file() {
                return Err(format!(
                    "fixed release binary is missing: {}",
                    path.display()
                ));
            }
        }
        Ok(binaries)
    }
}
