/// Exit code for successful execution.
pub const EXIT_SUCCESS: i32 = 0;

/// Exit code for general errors.
pub const EXIT_ERROR: i32 = 1;

/// Exit code when the daemon is not running or unreachable.
pub const EXIT_DAEMON_UNREACHABLE: i32 = 5;

/// Exit code when no clipboard entry matched the selector (`uniclip get`).
pub const EXIT_NO_MATCH: i32 = 6;

/// Exit code when a matched entry exists but its payload is unavailable —
/// e.g. `payloadState == "Lost"` or the file/blob is no longer present
/// (`uniclip get`). Distinct from a hard error so scripts can branch on it.
pub const EXIT_CONTENT_UNAVAILABLE: i32 = 7;
