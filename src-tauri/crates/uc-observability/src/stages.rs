//! Stage name constants for clipboard capture flow correlation.
//!
//! Each constant represents a discrete stage in the clipboard capture pipeline.
//! Used as tracing span names to provide consistent, queryable stage identifiers
//! across the `uc-app` and `uc-tauri` crates.

pub const DETECT: &str = "detect";
pub const NORMALIZE: &str = "normalize";
pub const PERSIST_EVENT: &str = "persist_event";
pub const CACHE_REPRESENTATIONS: &str = "cache_representations";
pub const SELECT_POLICY: &str = "select_policy";
pub const PERSIST_ENTRY: &str = "persist_entry";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_constants_are_lowercase_snake_case() {
        let stages = [
            ("DETECT", DETECT),
            ("NORMALIZE", NORMALIZE),
            ("PERSIST_EVENT", PERSIST_EVENT),
            ("CACHE_REPRESENTATIONS", CACHE_REPRESENTATIONS),
            ("SELECT_POLICY", SELECT_POLICY),
            ("PERSIST_ENTRY", PERSIST_ENTRY),
        ];
        for (name, value) in stages {
            assert_eq!(
                value,
                name.to_lowercase(),
                "Stage {} should equal its lowercased const name",
                name
            );
        }
    }

    #[test]
    fn all_stages_are_non_empty() {
        assert!(!DETECT.is_empty());
        assert!(!NORMALIZE.is_empty());
        assert!(!PERSIST_EVENT.is_empty());
        assert!(!CACHE_REPRESENTATIONS.is_empty());
        assert!(!SELECT_POLICY.is_empty());
        assert!(!PERSIST_ENTRY.is_empty());
    }
}
