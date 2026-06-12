//! Opaque invitation credential carried between sponsor and joiner.
//!
//! Slice 1 decision Q-ε: core does not validate the wire shape of the code —
//! the adapter (rendezvous client) owns format, length, and character-set
//! rules. Core only treats it as an identifier that travels through domain
//! types without dropping back to `String`.

use serde::{Deserialize, Serialize};

/// Short invitation code (sponsor→joiner handshake credential).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InvitationCode(String);

impl InvitationCode {
    /// Wrap an adapter-provided string without performing format validation.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for InvitationCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
