use std::fmt;

use uuid::Uuid;

/// Host-owned identity capability used by analytics aggregation workflows.
pub trait AnalyticsIdentityPort: Send + Sync {
    fn adopt_space_person(
        &self,
        space_person_id: Uuid,
    ) -> Result<AdoptOutcome, AnalyticsIdentityError>;

    fn release_space_person(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError>;

    fn current_space_person_id(&self) -> Option<Uuid>;

    fn reset_telemetry_identity(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdoptOutcome {
    pub previous_distinct_id: Uuid,
    pub new_distinct_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseOutcome {
    pub previous_distinct_id: Uuid,
    pub new_distinct_id: Uuid,
}

/// Derive the privacy-preserving group key shared by event context and use cases.
pub fn hash_space_id_for_telemetry(space_id: &str) -> String {
    use sha2::Digest;

    let digest = sha2::Sha256::digest(space_id.as_bytes());
    let mut out = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[derive(Debug)]
pub enum AnalyticsIdentityError {
    ContextNotInitialised,
    PersistFailed(anyhow::Error),
}

impl fmt::Display for AnalyticsIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextNotInitialised => write!(
                f,
                "global EventContext not initialised; bootstrap compose_event_context must run first"
            ),
            Self::PersistFailed(_) => write!(f, "persist space_person_id failed"),
        }
    }
}

impl std::error::Error for AnalyticsIdentityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ContextNotInitialised => None,
            Self::PersistFailed(error) => Some(error.as_ref()),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopAnalyticsIdentity;

impl AnalyticsIdentityPort for NoopAnalyticsIdentity {
    fn adopt_space_person(
        &self,
        space_person_id: Uuid,
    ) -> Result<AdoptOutcome, AnalyticsIdentityError> {
        Ok(AdoptOutcome {
            previous_distinct_id: Uuid::nil(),
            new_distinct_id: space_person_id,
        })
    }

    fn release_space_person(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
        Ok(ReleaseOutcome {
            previous_distinct_id: Uuid::nil(),
            new_distinct_id: Uuid::nil(),
        })
    }

    fn current_space_person_id(&self) -> Option<Uuid> {
        None
    }

    fn reset_telemetry_identity(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
        Ok(ReleaseOutcome {
            previous_distinct_id: Uuid::nil(),
            new_distinct_id: Uuid::nil(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_hash_is_stable_and_redacted() {
        let hash = hash_space_id_for_telemetry("space-test");
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|character| character.is_ascii_hexdigit()));
        assert_eq!(hash, hash_space_id_for_telemetry("space-test"));
    }
}
