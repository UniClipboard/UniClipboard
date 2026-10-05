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

/// Directory holding the binaries under test: `$CARGO_TARGET_DIR/debug`
/// (default `<repo>/target/debug`).
fn debug_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("debug")
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

impl NodeBinarySet {
    /// Current daemon paired with the user-facing CLI.
    ///
    /// The daemon is the Rust `uniclipd` and the CLI is the Go `uniclip`
    /// (`apps/cli-go`), both resolved from `$CARGO_TARGET_DIR/debug`. Build the
    /// CLI there with `scripts/e2e/build-cli.sh`. `UC_E2E_CLI` points the suite
    /// at a different CLI binary, for example to compare against a baseline.
    pub fn current() -> Self {
        let debug = debug_dir();
        let cli = std::env::var_os("UC_E2E_CLI")
            .map(PathBuf::from)
            .unwrap_or_else(|| debug.join(exe_name("uniclip")));
        Self {
            version: "current".to_string(),
            cli,
            daemon: debug.join(exe_name("uniclipd")),
            endpoint_discovery: DaemonEndpointDiscovery::ConnectionFile,
        }
    }

    /// Current daemon paired with the Rust development CLI `uc-dev-cli`, for the
    /// hidden development commands (`dev seed-clipboard` and friends) that have
    /// no daemon API and therefore no equivalent in the Go CLI.
    ///
    /// Build it with `cargo build -p uc-dev-cli --features uc-dev-cli/dev-tools`;
    /// it is resolved from `$CARGO_TARGET_DIR/debug`. CI builds it into a
    /// separate target directory and passes it through `UC_E2E_DEV_CLI`.
    pub fn current_dev_cli() -> Self {
        let mut binaries = Self::current();
        binaries.cli = std::env::var_os("UC_E2E_DEV_CLI")
            .map(PathBuf::from)
            .unwrap_or_else(|| debug_dir().join(exe_name("uc-dev-cli")));
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
