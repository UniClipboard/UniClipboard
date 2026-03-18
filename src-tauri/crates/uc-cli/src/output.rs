use serde::Serialize;

/// Print a value as either JSON or human-readable format.
///
/// When `json` is true, the value is serialized as pretty-printed JSON.
/// When `json` is false, the value's `Display` implementation is used.
///
/// Returns `Err` if JSON serialization fails. Callers handle the error
/// and return `EXIT_ERROR` -- no `process::exit()` inside this module.
pub fn print_result<T: Serialize + std::fmt::Display>(value: &T, json: bool) -> Result<(), String> {
    if json {
        let s = serde_json::to_string_pretty(value)
            .map_err(|e| format!("Failed to serialize to JSON: {}", e))?;
        println!("{}", s);
    } else {
        println!("{}", value);
    }
    Ok(())
}
