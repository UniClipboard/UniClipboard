//! Consumes the Engine's complete resource-release receipt.
//!
//! `Engine::shutdown_until_complete` folds `Released { error: Some(_) }` into
//! success and drops the error. Hosts must keep that error visible, and must not
//! treat anything but a positive `Released` as proof that resources are free.

use uc_engine::{EngineError, ResourceReleaseOutcome};

/// Interpret one release receipt.
///
/// - `Released { error: None }`: release confirmed.
/// - `Released { error: Some(_) }`: release confirmed; the operational error is
///   logged (stable code and category only) and not retried away.
/// - `Failed` / `Pending`: release is unconfirmed; the error is returned and the
///   caller keeps ownership of the Engine instead of reporting success.
pub fn settle_engine_release(outcome: ResourceReleaseOutcome) -> Result<(), EngineError> {
    match outcome {
        ResourceReleaseOutcome::Released { error: None } => Ok(()),
        ResourceReleaseOutcome::Released { error: Some(error) } => {
            tracing::warn!(
                code = error.code(),
                category = %error.category(),
                "engine resources released with an operational error"
            );
            Ok(())
        }
        outcome => outcome.into_result(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_engine::EngineErrorCategory;

    fn error() -> EngineError {
        EngineError::new(1002, EngineErrorCategory::Internal, false)
    }

    #[test]
    fn released_is_confirmed_even_with_an_operational_error() {
        assert!(settle_engine_release(ResourceReleaseOutcome::Released { error: None }).is_ok());
        assert!(settle_engine_release(ResourceReleaseOutcome::Released {
            error: Some(error())
        })
        .is_ok());
    }

    #[test]
    fn failed_and_pending_are_not_release_evidence() {
        assert!(settle_engine_release(ResourceReleaseOutcome::Failed { error: error() }).is_err());
        assert!(settle_engine_release(ResourceReleaseOutcome::Pending).is_err());
    }
}
