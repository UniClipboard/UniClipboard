use std::fmt;

use serde::{Deserialize, Serialize};

const DEFAULT_PROFILE_ID: &str = "default";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    app_version: String,
    profile_id: String,
}

impl EngineConfig {
    pub fn new(app_version: impl Into<String>) -> Self {
        Self {
            app_version: app_version.into(),
            profile_id: DEFAULT_PROFILE_ID.to_string(),
        }
    }

    pub fn with_profile_id(mut self, profile_id: impl Into<String>) -> Self {
        self.profile_id = profile_id.into();
        self
    }

    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }
}

impl fmt::Debug for EngineConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineConfig")
            .field("app_version", &self.app_version)
            .field("profile_id", &"[REDACTED]")
            .finish()
    }
}
