use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::diagnostics::{
    DebugStatusDto, DiagnosticCaptureStartRequestDto, DiagnosticCaptureStopRequestDto,
    DiagnosticCaptureStopResultDto, DiagnosticStatusDto, LogExportRequestDto, LogExportResultDto,
    UpdateDebugModeRequestDto, UpdateDebugModeResultDto,
};
use uc_daemon_contract::constants::http_route;

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;

#[derive(Clone)]
pub struct DaemonDiagnosticsClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonDiagnosticsClient {
    pub fn new(connection_state: DaemonConnectionState) -> Result<Self> {
        Ok(Self {
            http: Arc::new(crate::build_local_http_client()?),
            connection_state,
            client_type: "gui".to_string(),
        })
    }

    pub(crate) fn with_http_conn_state_and_type(
        http: Arc<reqwest::Client>,
        connection_state: DaemonConnectionState,
        client_type: String,
    ) -> Self {
        Self {
            http,
            connection_state,
            client_type,
        }
    }

    pub async fn debug_status(&self) -> Result<DebugStatusDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            http_route::DIAGNOSTICS_DEBUG,
            |r| r,
        )
        .await?)
    }

    pub async fn set_debug_mode(&self, enabled: bool) -> Result<UpdateDebugModeResultDto> {
        let req_body = UpdateDebugModeRequestDto { enabled };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::PUT,
            http_route::DIAGNOSTICS_DEBUG,
            |r| r.json(&req_body),
        )
        .await?)
    }

    pub async fn export_logs(&self, since_hours: Option<u32>) -> Result<LogExportResultDto> {
        let req_body = LogExportRequestDto { since_hours };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::DIAGNOSTICS_LOG_EXPORT,
            |r| r.json(&req_body),
        )
        .await?)
    }

    pub async fn capture_status(&self) -> Result<DiagnosticStatusDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            http_route::DIAGNOSTICS_CAPTURE,
            |request| request,
        )
        .await?)
    }

    pub async fn start_capture(&self, duration_seconds: u16) -> Result<DiagnosticStatusDto> {
        let body = DiagnosticCaptureStartRequestDto { duration_seconds };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::DIAGNOSTICS_CAPTURE_START,
            |request| request.json(&body),
        )
        .await?)
    }

    pub async fn stop_capture(&self, capture_id: String) -> Result<DiagnosticCaptureStopResultDto> {
        let body = DiagnosticCaptureStopRequestDto { capture_id };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::DIAGNOSTICS_CAPTURE_STOP,
            |request| request.json(&body),
        )
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use uc_daemon_contract::api::dto::diagnostics::{
        DiagnosticCaptureModeDto, DiagnosticCaptureStopResultDto,
    };
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn capture_commands_share_the_authenticated_daemon_routes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/connect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "sessionToken": "test-session",
                    "expiresInSecs": 300,
                    "refreshAtSecs": 240
                },
                "ts": 1
            })))
            .expect(3)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/diagnostics/capture"))
            .and(header("authorization", "Session test-session"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(status_envelope("standard", None)),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/diagnostics/capture/start"))
            .and(header("authorization", "Session test-session"))
            .and(body_json(serde_json::json!({ "durationSeconds": 600 })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(status_envelope("detailed", Some("capture-1"))),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/diagnostics/capture/stop"))
            .and(header("authorization", "Session test-session"))
            .and(body_json(serde_json::json!({ "captureId": "capture-1" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": "stopped",
                "ts": 4
            })))
            .expect(1)
            .mount(&server)
            .await;

        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: server.uri(),
            ws_url: "ws://127.0.0.1/unused".to_string(),
            token: "test-bearer".to_string(),
            pid: 42,
        });
        let client = DaemonDiagnosticsClient::with_http_conn_state_and_type(
            Arc::new(reqwest::Client::new()),
            connection_state,
            "test".to_string(),
        );

        let initial = client.capture_status().await.expect("status");
        let active = client.start_capture(600).await.expect("start");
        let stopped = client
            .stop_capture("capture-1".to_string())
            .await
            .expect("stop");

        assert_eq!(initial.capture.mode, DiagnosticCaptureModeDto::Standard);
        assert_eq!(active.capture.mode, DiagnosticCaptureModeDto::Detailed);
        assert_eq!(active.capture.capture_id.as_deref(), Some("capture-1"));
        assert_eq!(stopped, DiagnosticCaptureStopResultDto::Stopped);
    }

    fn status_envelope(mode: &str, capture_id: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "data": {
                "runId": "run-1",
                "capture": {
                    "mode": mode,
                    "captureId": capture_id,
                    "remainingMs": if capture_id.is_some() { 600_000 } else { 0 },
                    "startedAtUtc": null,
                    "endReason": null,
                    "lastCaptureId": null,
                    "revision": "1"
                },
                "observedRecords": "0",
                "policyFilteredRecords": "0",
                "schemaRejectedRecords": "0",
                "correlationLimitedRecords": "0",
                "engineVersion": "1.1.0",
                "sourceCommit": "abc123",
                "counterScope": "typed_events_only",
                "sources": [],
                "localFile": "ready",
                "closed": false
            },
            "ts": 2
        })
    }
}
