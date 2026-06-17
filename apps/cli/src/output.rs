use serde::Serialize;

use crate::exit_codes;
use crate::ui;

/// Serialize `value` as pretty JSON to stdout, mapping a serialization failure
/// to a user-facing error. Returns the process exit code, so a command arm can
/// `return output::emit_json(&dto, "search query response");` directly.
///
/// Use this for the common `if json { ... }` branch; the human-readable branch
/// stays in the command (each command renders its own layout).
pub fn emit_json<T: Serialize>(value: &T, context: &str) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(rendered) => {
            println!("{rendered}");
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::error(&format!("Failed to serialize {context}: {err}"));
            exit_codes::EXIT_ERROR
        }
    }
}

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
