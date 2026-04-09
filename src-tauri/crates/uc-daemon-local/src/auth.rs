//! Daemon-local auth token persistence and helpers.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use rand::RngCore;
use tracing::debug;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;

/// Internal daemon bearer token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonAuthToken(String);

impl DaemonAuthToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Load the daemon auth token from disk or create a new restricted file when missing.
pub fn load_or_create_auth_token(token_path: &Path) -> Result<DaemonAuthToken> {
    debug!(token_path = %token_path.display(), token_path_exists = token_path.exists(), "load_or_create_auth_token: entering");
    if token_path.exists() {
        let existing = fs::read_to_string(token_path).with_context(|| {
            format!(
                "failed to read daemon auth token at {}",
                token_path.display()
            )
        })?;
        let token = existing.trim().to_string();
        if !token.is_empty() {
            repair_token_permissions(token_path)?;
            return Ok(DaemonAuthToken(token));
        }
    }

    let token = generate_auth_token();
    persist_auth_token(token_path, &token)?;
    Ok(DaemonAuthToken(token))
}

/// Build the daemon's local HTTP and WebSocket connection metadata.
pub fn build_connection_info(
    host: &str,
    port: u16,
    token: &DaemonAuthToken,
    pid: u32,
) -> DaemonConnectionInfo {
    DaemonConnectionInfo {
        base_url: format!("http://{host}:{port}"),
        ws_url: format!("ws://{host}:{port}/ws"),
        token: token.as_str().to_string(),
        pid,
    }
}

/// Parse an HTTP Authorization header value and return the bearer token.
pub fn parse_bearer_token(header_value: &str) -> Option<&str> {
    let token = header_value.strip_prefix("Bearer ")?;
    if token.is_empty() {
        return None;
    }
    Some(token)
}

fn generate_auth_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn persist_auth_token(token_path: &Path, token: &str) -> Result<()> {
    if let Some(parent) = token_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create daemon auth token directory {}",
                parent.display()
            )
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(token_path)
        .with_context(|| {
            format!(
                "failed to open daemon auth token file {}",
                token_path.display()
            )
        })?;

    file.write_all(token.as_bytes()).with_context(|| {
        format!(
            "failed to write daemon auth token file {}",
            token_path.display()
        )
    })?;
    file.flush().with_context(|| {
        format!(
            "failed to flush daemon auth token file {}",
            token_path.display()
        )
    })?;

    repair_token_permissions(token_path)?;
    Ok(())
}

fn repair_token_permissions(token_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::metadata(token_path).with_context(|| {
            format!(
                "failed to read daemon auth token metadata {}",
                token_path.display()
            )
        })?;
        let current_mode = metadata.permissions().mode() & 0o777;
        if current_mode != 0o600 {
            let permissions = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(token_path, permissions).with_context(|| {
                format!(
                    "failed to repair daemon auth token permissions {}",
                    token_path.display()
                )
            })?;
        }
    }

    Ok(())
}
