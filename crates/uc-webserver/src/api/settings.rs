//! HTTP route handlers for settings endpoints.
//!
//! Provides read and write access to application settings.
//!
//! NOTE: Unlike the Tauri command (which applies OS-level side effects like
//! autostart registration and global shortcut updates), these handlers only
//! update the settings domain model — no autostart, no keyboard shortcuts.
use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tracing::{info, instrument};
use uc_application::facade::settings as app_settings;
use utoipa;

use uc_daemon_contract::api::dto::envelope::ApiEnvelope;

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::dto::settings::{
    RelayProbeOutcomeDto, RelayProbeRequestDto, SettingsDto, SettingsPatchDto,
    SettingsUpdateResultDto,
};
use crate::api::projection::{IntoApiDto, IntoDomain};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler))
        .route("/settings/relay-probe", post(probe_relay_url_handler))
}

/// GET /settings
/// Returns the current application settings as a typed Settings struct.
#[utoipa::path(
    get,
    path = "/settings",
    tag = "settings",
    operation_id = "getSettings",
    responses(
        (status = 200, description = "Current application settings", body = SettingsEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.settings.get", level = "info", skip(state))]
async fn get_settings_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<SettingsDto>>, ApiError> {
    info!("get settings request received");
    let app = state.app_facade_or_error()?;
    let settings = app
        .settings
        .get()
        .await
        .map_err(|e| settings_error_to_api("get_settings", e))?;

    info!("get settings succeeded");
    Ok(Json(ApiEnvelope::now(settings.into_api_dto())))
}

/// PUT /settings
/// Updates application settings. Accepts a partial settings object and merges it
/// with the existing settings.
///
/// NOTE: Unlike the Tauri command, this handler does NOT apply OS-level side
/// effects (no autostart registration, no keyboard shortcut updates). It only
/// persists the settings domain model.
#[utoipa::path(
    put,
    path = "/settings",
    tag = "settings",
    operation_id = "updateSettings",
    request_body = SettingsPatchDto,
    responses(
        (status = 200, description = "Settings persisted; carries success + restart-required signal", body = SettingsUpdateResultEnvelope),
        (status = 400, description = "Invalid request", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.settings.update",
    level = "info",
    skip(state, payload),
    fields(
        has_general = payload.general.is_some(),
        has_sync = payload.sync.is_some(),
        has_security = payload.security.is_some(),
        has_pairing = payload.pairing.is_some(),
        has_file_sync = payload.file_sync.is_some(),
        has_network = payload.network.is_some(),
        has_retention_policy = payload.retention_policy.is_some(),
        has_keyboard_shortcuts = payload.keyboard_shortcuts.is_some(),
        has_quick_panel = payload.quick_panel.is_some(),
    )
)]
async fn update_settings_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SettingsPatchDto>,
) -> Result<Json<ApiEnvelope<SettingsUpdateResultDto>>, ApiError> {
    info!("update settings request received");
    let app = state.app_facade_or_error()?;

    // D-D1：`network` 段非空（任何字段变更）触发 restart_required = true。
    // network 段里的 iroh 相关字段都是 endpoint bind-time 常量，仍走
    // `payload.network.is_some()` 统一触发重启。其它字段（general / sync 等）
    // 不影响该信号 — 它们不需要重启。
    //
    // `general.telemetry_enabled` 历史曾通过这里触发 restart（260505-17q），后于
    // 260505-1np 改成运行时 gate（见 uc-observability::set_telemetry_enabled），
    // 不再需要重启 — 下面在 facade 写盘成功后直接把新值推进 atomic 即可立即生效。
    // Pitfall 3 防御：调用方（前端 Phase 95）必须显式承担"还没真正生效"。
    let restart_required = payload.network.is_some();

    // 取出可能存在的 telemetry 新值，再传 patch 给 facade 写盘 — 写盘成功后再
    // 把 atomic 推进新值，保证持久化与运行时状态保持单调一致（如果写盘失败，
    // 也不会污染运行时 gate）。
    //
    // `usage_analytics_enabled` 走同样的"先取值、写盘、再推 gate"流程，但
    // 与 `telemetry_enabled` 是两个独立的开关（schema doc §6.4，GDPR
    // 友好实践）：前者控制 Sentry 错误上报，后者控制产品 telemetry。
    let telemetry_update = payload.general.as_ref().and_then(|g| g.telemetry_enabled);
    let analytics_update = payload
        .general
        .as_ref()
        .and_then(|g| g.usage_analytics_enabled);

    // The facade persists the patch. ADR-008 §0.1 folds `success` +
    // `restart_required` INTO the payload DTO, so the updated `SettingsView` is
    // no longer echoed back on the wire (the FE re-reads settings via GET). The
    // write must still happen for its side effects and error propagation.
    app.settings
        .update(payload.into_domain())
        .await
        .map_err(|e| settings_error_to_api("update_settings", e))?;

    if let Some(enabled) = telemetry_update {
        uc_observability::set_telemetry_enabled(enabled);
    }
    if let Some(enabled) = analytics_update {
        uc_observability::set_analytics_enabled(enabled);
    }

    info!(restart_required, "update settings succeeded");
    // ADR-008 §0.1: wire is `ApiEnvelope<SettingsUpdateResultDto>` —
    // `{ data: { success, restartRequired }, ts }`. The previously top-level
    // `success` / `restartRequired` siblings are folded into the payload.
    Ok(Json(ApiEnvelope::now(SettingsUpdateResultDto {
        success: true,
        restart_required,
    })))
}

/// POST /settings/relay-probe
///
/// Probes a candidate relay URL for reachability. Reads/writes no persisted
/// settings, so it is safe to call repeatedly ("test before save"). A probe
/// that fails to reach the relay is a NORMAL categorized outcome returned 200
/// (mirrors the Tauri command contract) — only a missing relay-diagnostic
/// adapter (server misconfiguration) becomes a 500 `ApiError`.
#[utoipa::path(
    post,
    path = "/settings/relay-probe",
    tag = "settings",
    operation_id = "probeRelayUrl",
    request_body = RelayProbeRequestDto,
    responses(
        (status = 200, description = "Relay probe outcome (reachable or a categorized failure)", body = RelayProbeOutcomeEnvelope),
        (status = 500, description = "Relay-diagnostic adapter unavailable / internal error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.settings.relay_probe", level = "info", skip(state, payload), fields(relay_url = %payload.url))]
async fn probe_relay_url_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<RelayProbeRequestDto>,
) -> Result<Json<ApiEnvelope<RelayProbeOutcomeDto>>, ApiError> {
    info!("relay probe request received");
    let app = state.app_facade_or_error()?;

    let result = app.settings.probe_relay_url(&payload.url).await;
    let outcome = probe_result_to_outcome(result)
        .map_err(|other| settings_error_to_api("relay_probe", other))?;

    info!("relay probe completed");
    Ok(Json(ApiEnvelope::now(outcome)))
}

/// Translate a `probe_relay_url` facade result into the wire outcome.
///
/// The probe-failure variants are expected user-facing outcomes (returned 200
/// so the FE can pick copy without catching an exception — parity with the
/// Tauri command). A missing relay-diagnostic adapter (`RelayProbeUnavailable`)
/// or any non-probe variant leaking through is a genuine server-side fault and
/// is propagated as `Err` for the caller to map to a 500.
fn probe_result_to_outcome(
    result: Result<app_settings::RelayProbeReportView, app_settings::SettingsFacadeError>,
) -> Result<RelayProbeOutcomeDto, app_settings::SettingsFacadeError> {
    use app_settings::SettingsFacadeError as E;
    Ok(match result {
        Ok(report) => RelayProbeOutcomeDto::Success {
            latency_ms: report.latency_ms,
        },
        Err(E::RelayProbeInvalidUrl(message)) => RelayProbeOutcomeDto::InvalidUrl { message },
        Err(E::RelayProbeDns(message)) => RelayProbeOutcomeDto::Dns { message },
        Err(E::RelayProbeTls(message)) => RelayProbeOutcomeDto::Tls { message },
        Err(E::RelayProbeHandshake(message)) => RelayProbeOutcomeDto::Handshake { message },
        Err(E::RelayProbeTimeout) => RelayProbeOutcomeDto::Timeout,
        Err(E::RelayProbeOther(message)) => RelayProbeOutcomeDto::Other { message },
        Err(other) => return Err(other),
    })
}

fn settings_error_to_api(op: &'static str, err: app_settings::SettingsFacadeError) -> ApiError {
    use app_settings::SettingsFacadeError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::Load(msg) => (
            "load",
            ApiError::internal(format!("failed to load settings: {msg}")),
        ),
        E::Save(msg) => (
            "save",
            ApiError::internal(format!("failed to save settings: {msg}")),
        ),
        E::Invalid(msg) => ("invalid", ApiError::bad_request(msg)),
        // Webserver 不调用 `SettingsFacade::probe_relay_url`,也不暴露探测端
        // 点 —— 这 7 个变体在当前 wiring 下无法被 `update_settings_handler` /
        // `get_settings_handler` 产出。穷举 match 让 rustc 在 facade 未来新增
        // 变体时强制 review;同时显式映射成内部错误而不是 panic,避免某天
        // 有人把 probe 接进新 handler 时让 daemon 在请求路径上直接挂掉。
        // 实际触达 = facade wiring 出 bug,变体名通过 log_facade_failure 写
        // 入 tracing 便于事后定位。
        E::RelayProbeUnavailable => {
            relay_probe_unexpected("relay_probe_unavailable", "Unavailable")
        }
        E::RelayProbeInvalidUrl(msg) => {
            relay_probe_unexpected("relay_probe_invalid_url", &format!("InvalidUrl: {msg}"))
        }
        E::RelayProbeDns(msg) => relay_probe_unexpected("relay_probe_dns", &format!("Dns: {msg}")),
        E::RelayProbeTls(msg) => relay_probe_unexpected("relay_probe_tls", &format!("Tls: {msg}")),
        E::RelayProbeHandshake(msg) => {
            relay_probe_unexpected("relay_probe_handshake", &format!("Handshake: {msg}"))
        }
        E::RelayProbeTimeout => relay_probe_unexpected("relay_probe_timeout", "Timeout"),
        E::RelayProbeOther(msg) => {
            relay_probe_unexpected("relay_probe_other", &format!("Other: {msg}"))
        }
    };
    log_facade_failure("settings", op, variant, api.status, &api.message);
    api
}

/// Map an unexpected `RelayProbe*` variant to a logged 500. Reaching this means
/// a probe-emitting use case is now plugged into a handler that doesn't model
/// probe errors — bug worth tracing, not crashing the request thread.
fn relay_probe_unexpected(variant: &'static str, detail: &str) -> (&'static str, ApiError) {
    tracing::error!(
        variant,
        detail,
        "settings_error_to_api received an unexpected RelayProbe* variant; \
         facade wiring exposed a probe path through this handler"
    );
    (
        variant,
        ApiError::internal(
            "settings facade returned an unexpected relay probe error \
             through a non-probe endpoint",
        ),
    )
}

// All view↔DTO field mappings live in `crate::api::projection::settings`
// (single source of truth per architecture-rules §Cross-Crate Type Conversion).

#[cfg(test)]
mod tests {
    use super::*;
    use app_settings::{RelayProbeReportView, SettingsFacadeError as E};

    #[test]
    fn probe_outcome_maps_success_with_latency() {
        let out = probe_result_to_outcome(Ok(RelayProbeReportView { latency_ms: 42 }))
            .expect("success must not be an error");
        assert_eq!(out, RelayProbeOutcomeDto::Success { latency_ms: 42 });
    }

    #[test]
    fn probe_outcome_maps_each_probe_failure_to_200_variant() {
        let cases = [
            (
                E::RelayProbeInvalidUrl("bad".into()),
                RelayProbeOutcomeDto::InvalidUrl {
                    message: "bad".into(),
                },
            ),
            (
                E::RelayProbeDns("nxdomain".into()),
                RelayProbeOutcomeDto::Dns {
                    message: "nxdomain".into(),
                },
            ),
            (
                E::RelayProbeTls("cert".into()),
                RelayProbeOutcomeDto::Tls {
                    message: "cert".into(),
                },
            ),
            (
                E::RelayProbeHandshake("nope".into()),
                RelayProbeOutcomeDto::Handshake {
                    message: "nope".into(),
                },
            ),
            (E::RelayProbeTimeout, RelayProbeOutcomeDto::Timeout),
            (
                E::RelayProbeOther("boom".into()),
                RelayProbeOutcomeDto::Other {
                    message: "boom".into(),
                },
            ),
        ];
        for (err, expected) in cases {
            let out = probe_result_to_outcome(Err(err))
                .expect("probe-failure variants are 200 outcomes, not errors");
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn probe_outcome_propagates_unavailable_as_error() {
        // Adapter-not-wired is a server-side fault → Err so the handler maps 500.
        let err = probe_result_to_outcome(Err(E::RelayProbeUnavailable))
            .expect_err("RelayProbeUnavailable must propagate as an error, not a 200 outcome");
        assert!(matches!(err, E::RelayProbeUnavailable));
    }

    #[test]
    fn probe_outcome_propagates_non_probe_variant_as_error() {
        // A non-probe variant leaking through this path is a wiring bug → Err.
        let err = probe_result_to_outcome(Err(E::Load("db".into())))
            .expect_err("non-probe variants must propagate as errors");
        assert!(matches!(err, E::Load(_)));
    }
}
