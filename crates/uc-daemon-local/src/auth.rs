//! Daemon-local auth token persistence and helpers.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use rand::RngCore;
use subtle::ConstantTimeEq;
use tracing::debug;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;

/// Internal daemon bearer token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonAuthToken(String);

impl DaemonAuthToken {
    /// Get a string slice of the inner daemon token.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_daemon_local::auth::load_or_create_auth_token;
    ///
    /// let tmp = tempfile::tempdir().unwrap();
    /// let token = load_or_create_auth_token(&tmp.path().join("daemon.token")).unwrap();
    /// assert_eq!(token.as_str().len(), 64);
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Verify a candidate token against this token in constant time.
    ///
    /// Uses a constant-time byte comparison (`subtle::ConstantTimeEq`) so the
    /// running time does not depend on how many leading bytes match. A naive
    /// `==` short-circuits on the first mismatching byte, which lets a local
    /// process on the loopback interface probe the token byte-by-byte via a
    /// timing side-channel. Always prefer this over comparing `as_str()`.
    ///
    /// The token length (fixed 64 hex chars, see `generate_auth_token`) is not
    /// secret, so the length-mismatch fast path inside `ct_eq` is acceptable.
    pub fn verify(&self, candidate: &str) -> bool {
        self.0.as_bytes().ct_eq(candidate.as_bytes()).into()
    }
}

/// Ensure a daemon authentication token exists at the provided path and return it.
///
/// If a non-empty token file exists at `token_path`, the function repairs its permissions (on Unix)
/// and returns the contained token. If the file is missing or contains no token, a new
/// cryptographically-random token is generated, persisted to `token_path`, and returned. On Unix,
/// persisted token files are created with permission mode `0o600`.
///
/// # Parameters
///
/// - `token_path`: Filesystem path where the daemon token is read from or written to.
///
/// # Returns
///
/// `DaemonAuthToken` containing the token read from disk or the newly generated token.
///
/// # Examples
///
/// ```
/// use uc_daemon_local::auth::load_or_create_auth_token;
///
/// let tmp = tempfile::tempdir().unwrap();
/// let path = tmp.path().join("daemon.token");
/// let token = load_or_create_auth_token(&path).unwrap();
/// assert_eq!(token.as_str().len(), 64);
/// // A second call reads the persisted token back.
/// assert_eq!(load_or_create_auth_token(&path).unwrap(), token);
/// ```
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

/// Constructs connection metadata for the local daemon.
///
/// The returned `DaemonConnectionInfo` contains:
/// - `base_url`: `http://{host}:{port}`
/// - `ws_url`: `ws://{host}:{port}/ws`
/// - `token`: the provided daemon token as a `String`
/// - `pid`: the provided process id
///
/// # Examples
///
/// ```
/// use uc_daemon_local::auth::{build_connection_info, load_or_create_auth_token};
///
/// let tmp = tempfile::tempdir().unwrap();
/// let token = load_or_create_auth_token(&tmp.path().join("daemon.token")).unwrap();
/// let info = build_connection_info("127.0.0.1", 8080, &token, 12345);
/// assert_eq!(info.base_url, "http://127.0.0.1:8080");
/// assert_eq!(info.ws_url, "ws://127.0.0.1:8080/ws");
/// assert_eq!(info.token, token.as_str());
/// assert_eq!(info.pid, 12345);
/// ```
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

/// Extracts the bearer token from an HTTP `Authorization` header value.
///
/// Returns `Some(&str)` with the token when the header uses the `Bearer` scheme
/// (case-sensitive) and contains a non-empty token, otherwise returns `None`.
///
/// # Examples
///
/// ```
/// use uc_daemon_local::auth::parse_bearer_token;
///
/// assert_eq!(parse_bearer_token("Bearer abc123"), Some("abc123"));
/// assert_eq!(parse_bearer_token("bearer xyz"), None);
/// assert_eq!(parse_bearer_token("Basic abc"), None);
/// assert_eq!(parse_bearer_token("Bearer "), None);
/// assert_eq!(parse_bearer_token("JustOnePart"), None);
/// ```
pub fn parse_bearer_token(header_value: &str) -> Option<&str> {
    let parts: Vec<&str> = header_value.splitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    if parts[0] != "Bearer" {
        return None;
    }
    let token = parts[1];
    if token.is_empty() {
        return None;
    }
    Some(token)
}

/// Creates a 64-character lowercase hexadecimal authentication token using cryptographically secure randomness.
///
/// The returned string encodes 32 random bytes as two-digit lowercase hex characters (64 hex characters total).
///
/// # Examples
///
/// Private helper — not importable from doctests; behavior is covered by the
/// `generate_auth_token_*` unit tests below.
///
/// ```ignore
/// let token = generate_auth_token();
/// assert_eq!(token.len(), 64);
/// assert!(token.chars().all(|c| c.is_ascii_hexdigit() && c.is_ascii_lowercase()));
/// ```
fn generate_auth_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Persists the daemon auth token to disk, creating parent directories if needed and restricting file permissions.
///
/// Writes `token` to `token_path`, truncating any existing file, flushes the file, and (on Unix) ensures the file mode is set to `0o600`. This function will create any missing parent directories before writing.
///
/// # Errors
///
/// Returns an error if creating directories, opening the file, writing, flushing, or repairing file permissions fails.
///
/// # Examples
///
/// Private helper — not importable from doctests; behavior is covered by the
/// `persist_auth_token_*` unit tests below.
///
/// ```ignore
/// let tmp = tempfile::tempdir().unwrap();
/// let path = tmp.path().join("auth_token");
/// persist_auth_token(&path, "0123abcd").unwrap();
/// assert!(path.exists());
/// ```
fn persist_auth_token(token_path: &Path, token: &str) -> Result<()> {
    if let Some(parent) = token_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create daemon auth token directory {}",
                parent.display()
            )
        })?;
    }

    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);

    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(token_path).with_context(|| {
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

/// Ensure the token file has restrictive Unix permissions (mode `0o600`) when running on Unix; on non-Unix platforms this is a no-op.
///
/// Attempts to read the file metadata and, if the file's permission bits are not `0o600`, set them to `0o600`. Errors are returned with contextual messages if metadata cannot be read or permissions cannot be changed.
///
/// # Examples
///
/// Private helper — not importable from doctests; behavior is covered by the
/// `repair_token_permissions_*` unit tests below.
///
/// ```ignore
/// use std::path::Path;
/// // After creating or writing the token file:
/// repair_token_permissions(Path::new("/path/to/daemon_token")).unwrap();
/// ```
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_matches_exact_token() {
        let token = DaemonAuthToken(generate_auth_token());
        let candidate = token.as_str().to_string();
        assert!(token.verify(&candidate));
    }

    #[test]
    fn verify_rejects_wrong_token() {
        let token = DaemonAuthToken("abcd1234".into());
        // Same length, differing last byte: must not match.
        assert!(!token.verify("abcd1235"));
        // Differing first byte: must not match.
        assert!(!token.verify("Xbcd1234"));
    }

    #[test]
    fn verify_rejects_length_mismatch() {
        let token = DaemonAuthToken("abcd1234".into());
        assert!(!token.verify("abcd"));
        assert!(!token.verify("abcd12345"));
        assert!(!token.verify(""));
    }

    #[test]
    fn verify_rejects_a_different_generated_token() {
        let token = DaemonAuthToken(generate_auth_token());
        let other = generate_auth_token();
        assert!(!token.verify(&other));
    }

    // ── generate_auth_token ──────────────────────────────────────────────

    #[test]
    fn generate_auth_token_is_64_lowercase_hex() {
        let token = generate_auth_token();
        assert_eq!(token.len(), 64, "32 random bytes encode to 64 hex chars");
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "token must be lowercase hex, got {token}"
        );
    }

    #[test]
    fn generate_auth_token_is_unpredictable_across_calls() {
        // 1024 draws with zero collisions — guards against a constant / seeded-RNG
        // regression that would make every daemon share a predictable token.
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for _ in 0..1024 {
            assert!(
                seen.insert(generate_auth_token()),
                "generated a duplicate token"
            );
        }
    }

    // ── parse_bearer_token ───────────────────────────────────────────────

    #[test]
    fn parse_bearer_token_accepts_well_formed_header() {
        assert_eq!(parse_bearer_token("Bearer abc123"), Some("abc123"));
    }

    #[test]
    fn parse_bearer_token_preserves_token_with_inner_spaces() {
        // splitn(2, ' ') keeps everything after the first space verbatim.
        assert_eq!(parse_bearer_token("Bearer abc def"), Some("abc def"));
    }

    #[test]
    fn parse_bearer_token_is_scheme_case_sensitive() {
        assert_eq!(parse_bearer_token("bearer xyz"), None);
        assert_eq!(parse_bearer_token("BEARER xyz"), None);
        assert_eq!(parse_bearer_token("Basic abc"), None);
    }

    #[test]
    fn parse_bearer_token_rejects_empty_or_single_part() {
        assert_eq!(parse_bearer_token("Bearer "), None);
        assert_eq!(parse_bearer_token("JustOnePart"), None);
        assert_eq!(parse_bearer_token(""), None);
    }

    // ── build_connection_info ────────────────────────────────────────────

    #[test]
    fn build_connection_info_formats_urls_and_carries_token_and_pid() {
        let token = DaemonAuthToken("deadbeef".to_string());
        let info = build_connection_info("127.0.0.1", 8080, &token, 12345);
        assert_eq!(info.base_url, "http://127.0.0.1:8080");
        assert_eq!(info.ws_url, "ws://127.0.0.1:8080/ws");
        assert_eq!(info.token, "deadbeef");
        assert_eq!(info.pid, 12345);
    }

    // ── persist_auth_token ───────────────────────────────────────────────

    #[test]
    fn persist_auth_token_creates_missing_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp
            .path()
            .join("nested")
            .join("deeper")
            .join("daemon.token");
        persist_auth_token(&path, "0123abcd").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "0123abcd");
    }

    #[cfg(unix)]
    #[test]
    fn persist_auth_token_writes_file_as_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.token");
        persist_auth_token(&path, "secret").unwrap();
        assert_eq!(
            mode_of(&path),
            0o600,
            "freshly persisted token must be 0600"
        );
    }

    // ── repair_token_permissions ─────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn repair_token_permissions_tightens_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("loose.token");
        fs::write(&path, "tok").unwrap();
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(mode_of(&path), 0o644);

        repair_token_permissions(&path).unwrap();
        assert_eq!(mode_of(&path), 0o600, "0644 must be tightened to 0600");
    }

    #[cfg(unix)]
    #[test]
    fn repair_token_permissions_is_noop_when_already_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ok.token");
        fs::write(&path, "tok").unwrap();
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        repair_token_permissions(&path).unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    // ── load_or_create_auth_token ────────────────────────────────────────

    #[test]
    fn load_or_create_generates_and_persists_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.token");
        assert!(!path.exists());

        let token = load_or_create_auth_token(&path).unwrap();
        assert_eq!(token.as_str().len(), 64);
        assert!(path.exists(), "token must be persisted on first creation");
        assert_eq!(fs::read_to_string(&path).unwrap(), token.as_str());
    }

    #[cfg(unix)]
    #[test]
    fn load_or_create_persists_new_token_as_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.token");
        let _ = load_or_create_auth_token(&path).unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    #[test]
    fn load_or_create_is_idempotent_returning_same_token() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.token");
        let first = load_or_create_auth_token(&path).unwrap();
        let second = load_or_create_auth_token(&path).unwrap();
        assert_eq!(first, second, "second call must read the persisted token");
    }

    #[test]
    fn load_or_create_trims_surrounding_whitespace_in_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.token");
        fs::write(&path, "  padded-token\n").unwrap();
        let token = load_or_create_auth_token(&path).unwrap();
        assert_eq!(token.as_str(), "padded-token");
    }

    #[test]
    fn load_or_create_regenerates_when_existing_file_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.token");
        fs::write(&path, "").unwrap();
        let token = load_or_create_auth_token(&path).unwrap();
        assert_eq!(
            token.as_str().len(),
            64,
            "an empty token file must be replaced with a fresh token"
        );
    }

    #[test]
    fn load_or_create_regenerates_when_existing_file_is_whitespace_only() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.token");
        fs::write(&path, "   \n\t").unwrap();
        let token = load_or_create_auth_token(&path).unwrap();
        assert_eq!(token.as_str().len(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn load_or_create_repairs_permissions_on_existing_loose_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.token");
        fs::write(&path, "existing-token").unwrap();
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let token = load_or_create_auth_token(&path).unwrap();
        assert_eq!(token.as_str(), "existing-token");
        assert_eq!(
            mode_of(&path),
            0o600,
            "loading an existing token must tighten its perms"
        );
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }
}
