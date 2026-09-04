use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonEndpointDiscovery {
    FixedProfilePort,
    ConnectionFile,
}

#[derive(Clone, Debug)]
pub struct NodeBinarySet {
    pub version: String,
    pub cli: PathBuf,
    pub daemon: PathBuf,
    pub endpoint_discovery: DaemonEndpointDiscovery,
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
            endpoint_discovery: DaemonEndpointDiscovery::ConnectionFile,
        }
    }

    /// Current daemon paired with a CLI built for hidden development commands.
    ///
    /// CI keeps the default CLI for the broad E2E suite and supplies its
    /// separately built development CLI through `UC_E2E_DEV_CLI`. Local runs
    /// that build `uc-cli` with `dev-tools` in the normal target directory do
    /// not need the override.
    pub fn current_dev_cli() -> Self {
        let mut binaries = Self::current();
        if let Some(cli) = std::env::var_os("UC_E2E_DEV_CLI") {
            binaries.cli = cli.into();
        }
        binaries.version = "current-dev-cli".to_string();
        binaries
    }

    pub fn fixed(
        version: impl Into<String>,
        cli: impl Into<PathBuf>,
        daemon: impl Into<PathBuf>,
    ) -> Self {
        let version = version.into();
        Self {
            endpoint_discovery: default_historical_discovery(&version),
            version,
            cli: cli.into(),
            daemon: daemon.into(),
        }
    }

    pub fn fixed_release_dir(
        version: impl Into<String>,
        directory: impl AsRef<Path>,
    ) -> Result<Self, String> {
        let version = version.into();
        Self::fixed_release_dir_with_discovery(
            version.clone(),
            directory,
            default_historical_discovery(&version),
        )
    }

    pub fn fixed_release_dir_with_discovery(
        version: impl Into<String>,
        directory: impl AsRef<Path>,
        endpoint_discovery: DaemonEndpointDiscovery,
    ) -> Result<Self, String> {
        let directory = directory.as_ref();
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let binaries = Self {
            version: version.into(),
            cli: directory.join(format!("uniclip{suffix}")),
            daemon: directory.join(format!("uniclipd{suffix}")),
            endpoint_discovery,
        };
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

fn default_historical_discovery(version: &str) -> DaemonEndpointDiscovery {
    if version.starts_with("0.") {
        DaemonEndpointDiscovery::FixedProfilePort
    } else {
        DaemonEndpointDiscovery::ConnectionFile
    }
}
