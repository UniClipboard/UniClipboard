use crate::visual_effects::EvidenceClass;
use uc_desktop::visual_capabilities::{self, VisualCapability};

// Cold native graphics discovery can exceed 500ms even on a capable machine.
// The UI remains static while the first authoritative snapshot is pending.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub async fn probe() -> EvidenceClass {
    if cfg!(target_os = "linux") {
        return EvidenceClass::Unknown;
    }
    let span = tracing::Span::current();
    let task = tokio::task::spawn_blocking(move || {
        span.in_scope(|| match visual_capabilities::detect() {
            VisualCapability::Capable => EvidenceClass::Capable,
            VisualCapability::Constrained => EvidenceClass::Constrained,
            VisualCapability::Unknown => EvidenceClass::Unknown,
        })
    });
    wait_for_probe(task).await
}

async fn wait_for_probe(task: tokio::task::JoinHandle<EvidenceClass>) -> EvidenceClass {
    match tokio::time::timeout(PROBE_TIMEOUT, task).await {
        Ok(Ok(evidence)) => evidence,
        _ => {
            tracing::warn!(
                error_kind = "visual_effects_probe_unavailable",
                "using conservative visual effects"
            );
            EvidenceClass::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn visual_effects_probe_accepts_cold_graphics_discovery() {
        let task = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(750)).await;
            EvidenceClass::Capable
        });
        assert_eq!(wait_for_probe(task).await, EvidenceClass::Capable);
    }

    #[tokio::test(start_paused = true)]
    async fn visual_effects_probe_has_a_bounded_unknown_fallback() {
        let task = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            EvidenceClass::Capable
        });
        let started = tokio::time::Instant::now();
        assert_eq!(wait_for_probe(task).await, EvidenceClass::Unknown);
        assert_eq!(started.elapsed(), PROBE_TIMEOUT);
    }

    #[tokio::test]
    #[ignore = "run explicitly on a supported graphics-capable desktop"]
    async fn visual_effects_probe_live_host_within_startup_budget() {
        let started = std::time::Instant::now();
        let result = probe().await;
        println!(
            "shell visual capability: {result:?}; elapsed: {:?}",
            started.elapsed()
        );
        assert_eq!(result, EvidenceClass::Capable);
    }
}
