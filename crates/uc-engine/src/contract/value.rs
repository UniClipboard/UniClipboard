use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(Vec<u8>);

impl SecretString {
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().as_bytes().to_vec())
    }

    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostFileHandle(String);

impl HostFileHandle {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for HostFileHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HostFileHandle([REDACTED])")
    }
}
