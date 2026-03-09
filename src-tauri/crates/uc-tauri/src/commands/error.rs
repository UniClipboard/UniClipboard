/// Centralized error mapping for commands.
///
/// This function provides a single upgrade path for future
/// CommandError enhancements (e.g., error codes).
pub fn map_err(err: anyhow::Error) -> String {
    err.to_string()
}
