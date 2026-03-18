//! Shared daemon socket path resolution.

use std::path::{Path, PathBuf};

const SOCKET_NAME: &str = "uniclipboard-daemon.sock";

#[cfg(unix)]
const MAX_SOCKET_PATH_BYTES: usize = 103;

/// Resolve the daemon RPC socket path.
#[cfg(unix)]
pub fn resolve_daemon_socket_path() -> PathBuf {
    let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();
    let base = sanitize_xdg_runtime_dir(xdg_runtime_dir.as_deref());
    resolve_daemon_socket_path_from(base.as_deref())
}

/// Resolve the daemon RPC socket path.
#[cfg(not(unix))]
pub fn resolve_daemon_socket_path() -> PathBuf {
    std::env::temp_dir().join(SOCKET_NAME)
}

#[cfg(unix)]
fn sanitize_xdg_runtime_dir(xdg: Option<&str>) -> Option<PathBuf> {
    let value = xdg?.trim();
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

#[cfg(unix)]
fn resolve_daemon_socket_path_from(base: Option<&Path>) -> PathBuf {
    let fallback = Path::new("/tmp");
    let candidate_base = base.unwrap_or(fallback);
    let candidate = candidate_base.join(SOCKET_NAME);

    if socket_path_byte_len(&candidate) <= MAX_SOCKET_PATH_BYTES {
        return candidate;
    }

    let fallback_path = fallback.join(SOCKET_NAME);
    tracing::warn!(
        socket_path = %candidate.display(),
        fallback_path = %fallback_path.display(),
        socket_path_bytes = socket_path_byte_len(&candidate),
        max_socket_path_bytes = MAX_SOCKET_PATH_BYTES,
        "daemon socket path exceeds unix socket byte limit; falling back to /tmp"
    );
    fallback_path
}

#[cfg(unix)]
fn socket_path_byte_len(path: &Path) -> usize {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().len()
}

#[cfg(not(unix))]
fn socket_path_byte_len(path: &Path) -> usize {
    path.as_os_str().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path_ends_with_sock() {
        let path = resolve_daemon_socket_path();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(SOCKET_NAME)
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_socket_path_length() {
        let path = resolve_daemon_socket_path_from(None);
        assert!(socket_path_byte_len(&path) <= MAX_SOCKET_PATH_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn test_xdg_runtime_dir_override() {
        let path = resolve_daemon_socket_path_from(Some(Path::new("/run/user/1000")));
        assert_eq!(path, PathBuf::from("/run/user/1000").join(SOCKET_NAME));
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_xdg_none() {
        assert_eq!(sanitize_xdg_runtime_dir(None), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_xdg_empty() {
        assert_eq!(sanitize_xdg_runtime_dir(Some("")), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_xdg_whitespace_only() {
        assert_eq!(sanitize_xdg_runtime_dir(Some("   ")), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_xdg_valid() {
        assert_eq!(
            sanitize_xdg_runtime_dir(Some("/run/user/1000")),
            Some(PathBuf::from("/run/user/1000"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_from_none() {
        let path = resolve_daemon_socket_path_from(None);
        assert_eq!(path, PathBuf::from("/tmp").join(SOCKET_NAME));
    }

    #[cfg(unix)]
    #[test]
    fn test_xdg_runtime_dir_too_long() {
        let long_base = PathBuf::from("/").join("a".repeat(90));
        let path = resolve_daemon_socket_path_from(Some(long_base.as_path()));
        assert_eq!(path, PathBuf::from("/tmp").join(SOCKET_NAME));
    }

    #[cfg(unix)]
    #[test]
    fn test_socket_path_boundary() {
        let max_base_len = MAX_SOCKET_PATH_BYTES - 1 - SOCKET_NAME.len();
        let exact_base = PathBuf::from(format!("/{}", "x".repeat(max_base_len - 1)));
        let exact_path = resolve_daemon_socket_path_from(Some(exact_base.as_path()));
        assert_eq!(socket_path_byte_len(&exact_path), MAX_SOCKET_PATH_BYTES);
        assert_eq!(exact_path, exact_base.join(SOCKET_NAME));

        let too_long_base = PathBuf::from(format!("/{}", "x".repeat(max_base_len)));
        let too_long_path = resolve_daemon_socket_path_from(Some(too_long_base.as_path()));
        assert_eq!(too_long_path, PathBuf::from("/tmp").join(SOCKET_NAME));
    }
}
