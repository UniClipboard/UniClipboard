use serde::{Deserialize, Serialize};

/// Operating-system family used in low-cardinality analytics properties.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Macos,
    Windows,
    Linux,
    Ios,
    Android,
    Other,
}
