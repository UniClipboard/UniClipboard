use uc_daemon_contract::api::dto::diagnostics::{
    DiagnosticCaptureEndReasonDto, DiagnosticCaptureModeDto, DiagnosticCaptureStartRequestDto,
    DiagnosticCaptureStateDto, DiagnosticFileSourceCountsDto, DiagnosticSetupStatusDto,
    DiagnosticSourceCapabilityDto, DiagnosticSourceCollectionDto, DiagnosticSourceCoverageDto,
    DiagnosticSourceDto, DiagnosticStatusDto,
};

#[test]
fn capture_contract_uses_closed_camel_case_values() {
    let status = DiagnosticStatusDto {
        run_id: "run-1".to_string(),
        capture: DiagnosticCaptureStateDto {
            mode: DiagnosticCaptureModeDto::Detailed,
            capture_id: Some("capture-1".to_string()),
            remaining_ms: 30_000,
            started_at_utc: Some("2026-09-11T00:00:00Z".to_string()),
            end_reason: Some(DiagnosticCaptureEndReasonDto::Requested),
            last_capture_id: None,
            revision: "2".to_string(),
        },
        observed_records: "4".to_string(),
        policy_filtered_records: "1".to_string(),
        schema_rejected_records: "0".to_string(),
        correlation_limited_records: "0".to_string(),
        engine_version: "1.1.0-rc.14".to_string(),
        source_commit: "abc123".to_string(),
        counter_scope: "typed_events_only".to_string(),
        sources: vec![DiagnosticSourceCoverageDto {
            source: DiagnosticSourceDto::HostApplication,
            capability: DiagnosticSourceCapabilityDto::Partial,
            collection: DiagnosticSourceCollectionDto::Enabled,
            observed_count: "2".to_string(),
            policy_filtered_count: "0".to_string(),
        }],
        local_file: DiagnosticSetupStatusDto::Ready,
        closed: false,
    };

    let json = serde_json::to_value(status).expect("serialize status");
    assert_eq!(json["runId"], "run-1");
    assert_eq!(json["capture"]["mode"], "detailed");
    assert_eq!(json["capture"]["endReason"], "requested");
    assert_eq!(json["sources"][0]["source"], "hostApplication");
    assert_eq!(json["sources"][0]["capability"], "partial");
}

#[test]
fn capture_duration_is_an_explicit_bounded_input() {
    let request: DiagnosticCaptureStartRequestDto =
        serde_json::from_value(serde_json::json!({ "durationSeconds": 600 }))
            .expect("deserialize request");
    assert_eq!(request.duration_seconds, 600);
}

#[test]
fn file_coverage_keeps_drop_and_write_failures_distinct() {
    let counts = DiagnosticFileSourceCountsDto {
        source: DiagnosticSourceDto::Connections,
        accepted_count: "9".to_string(),
        written_count: "6".to_string(),
        queue_dropped_count: "1".to_string(),
        quota_dropped_count: "1".to_string(),
        write_failed_count: "1".to_string(),
        last_written_at_ms: Some("1787900000000".to_string()),
    };

    let json = serde_json::to_value(counts).expect("serialize counts");
    assert_eq!(json["queueDroppedCount"], "1");
    assert_eq!(json["quotaDroppedCount"], "1");
    assert_eq!(json["writeFailedCount"], "1");
}
