use crate::visual_effects::EvidenceClass;

pub async fn probe() -> EvidenceClass {
    if cfg!(target_os = "linux") {
        return EvidenceClass::Unknown;
    }
    let span = tracing::Span::current();
    let task = tokio::task::spawn_blocking(move || {
        span.in_scope(|| {
            let mut system = sysinfo::System::new();
            system.refresh_memory();
            system.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
            let _capacity = (system.cpus().len(), system.total_memory());
            // CPU/RAM alone do not establish the active WebView compositor's capability.
            // Keep unknown until the calibration report supplies validated combined rules.
            EvidenceClass::Unknown
        })
    });
    match tokio::time::timeout(std::time::Duration::from_millis(500), task).await {
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
