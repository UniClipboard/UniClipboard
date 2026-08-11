//! Daemon-local auth token persistence and helpers.

use std::path::Path;

use anyhow::Result;
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
    /// use uc_daemon_local::auth::load_or_create_auth_token_from_conn;
    ///
    /// let tmp = tempfile::tempdir().unwrap();
    /// let token = load_or_create_auth_token_from_conn(&tmp.path().join("daemon.conn")).unwrap();
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

/// Ensure a daemon bearer token exists and return it (ADR-011).
///
/// The token now lives inside the `daemon.conn` connection file, which is the
/// single source of truth for the daemon's connection info. If a non-empty
/// token can be read from an existing connection file, it is returned
/// (token persists across daemon restarts); otherwise a fresh
/// cryptographically-random token is generated. The token is NOT persisted
/// here — the HTTP server publishes it as part of the connection file once
/// the loopback listener is bound.
///
/// # Parameters
///
/// - `conn_path`: Filesystem path of the `daemon.conn` connection file.
///
/// # Returns
///
/// `DaemonAuthToken` containing the token read from disk or a newly generated token.
///
/// # Examples
///
/// ```
/// use uc_daemon_local::auth::load_or_create_auth_token_from_conn;
///
/// let tmp = tempfile::tempdir().unwrap();
/// let path = tmp.path().join("daemon.conn");
/// let token = load_or_create_auth_token_from_conn(&path).unwrap();
/// assert_eq!(token.as_str().len(), 64);
/// // A second call on an existing connection file reads the persisted token.
/// uc_daemon_process::socket::write_daemon_conn_file_at(
///     &path,
///     &uc_daemon_process::socket::DaemonConnFile::new("127.0.0.1", 43127, token.as_str(), 42),
/// )
/// .unwrap();
/// assert_eq!(load_or_create_auth_token_from_conn(&path).unwrap(), token);
/// ```
pub fn load_or_create_auth_token_from_conn(conn_path: &Path) -> Result<DaemonAuthToken> {
    debug!(conn_path = %conn_path.display(), conn_path_exists = conn_path.exists(), "load_or_create_auth_token_from_conn: entering");
    if conn_path.exists() {
        // A corrupt / unsupported connection file is NOT fatal: the daemon
        // will overwrite the file on this very start, so falling back to a
        // fresh token self-heals (and every client re-reads the file anyway).
        match uc_daemon_process::socket::read_daemon_conn_file_at(conn_path) {
            Ok(Some(conn)) => {
                let token = conn.token.trim().to_string();
                if !token.is_empty() {
                    return Ok(DaemonAuthToken(token));
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "existing daemon connection file is unreadable; generating a fresh token"
                );
            }
        }
    }

    Ok(DaemonAuthToken(generate_auth_token()))
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
/// use uc_daemon_local::auth::{
///     build_connection_info, load_or_create_auth_token_from_conn,
/// };
///
/// let tmp = tempfile::tempdir().unwrap();
/// let token = load_or_create_auth_token_from_conn(&tmp.path().join("daemon.conn")).unwrap();
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

    // ── load_or_create_auth_token_from_conn (ADR-011) ────────────────────

    #[test]
    fn load_or_create_from_conn_generates_fresh_token_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        assert!(!path.exists());

        let token = load_or_create_auth_token_from_conn(&path).unwrap();
        assert_eq!(token.as_str().len(), 64);
        // The token is NOT persisted here — the HTTP server publishes it
        // inside the connection file after binding.
        assert!(
            !path.exists(),
            "token must not be persisted by load-or-create"
        );
    }

    #[test]
    fn load_or_create_from_conn_reads_existing_token() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        uc_daemon_process::socket::write_daemon_conn_file_at(
            &path,
            &uc_daemon_process::socket::DaemonConnFile::new(
                "127.0.0.1",
                43127,
                "persisted-token",
                7,
            ),
        )
        .unwrap();

        let token = load_or_create_auth_token_from_conn(&path).unwrap();
        assert_eq!(token.as_str(), "persisted-token");
    }

    #[test]
    fn load_or_create_from_conn_is_idempotent_returning_same_token() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        uc_daemon_process::socket::write_daemon_conn_file_at(
            &path,
            &uc_daemon_process::socket::DaemonConnFile::new("127.0.0.1", 43127, "tok", 7),
        )
        .unwrap();

        let first = load_or_create_auth_token_from_conn(&path).unwrap();
        let second = load_or_create_auth_token_from_conn(&path).unwrap();
        assert_eq!(first, second, "second call must read the persisted token");
    }

    #[test]
    fn load_or_create_from_conn_regenerates_on_corrupt_file() {
        // A corrupt / unknown-format connection file must not block daemon
        // startup: the daemon overwrites the file on this very start, so a
        // fresh token self-heals.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.conn");
        std::fs::write(&path, "not-json").unwrap();

        let token = load_or_create_auth_token_from_conn(&path).unwrap();
        assert_eq!(token.as_str().len(), 64);
    }
}
