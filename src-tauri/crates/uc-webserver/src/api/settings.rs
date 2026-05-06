//! HTTP route handlers for settings endpoints.
//!
//! Provides read and write access to application settings.
//!
//! NOTE: Unlike the Tauri command (which applies OS-level side effects like
//! autostart registration and global shortcut updates), these handlers only
//! update the settings domain model — no autostart, no keyboard shortcuts.
use axum::extract::State;
use axum::routing::{get, put};
use axum::{Json, Router};
use tracing::{info, instrument};
use uc_application::facade::settings as app_settings;
use utoipa;

use crate::api::dto::error::ApiError;
use crate::api::dto::settings::{
    ContentTypesDto, ContentTypesPatchDto, FileSyncSettingsDto, GeneralSettingsDto,
    GetSettingsResponse, KeyboardShortcutsPatchDto, NetworkSettingsDto, PairingSettingsDto,
    RetentionPolicyDto, RetentionRuleDto, SecuritySettingsDto, SettingsDto, SettingsPatchDto,
    SyncSettingsDto, UpdateSettingsResponse,
};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler))
}

/// GET /settings
/// Returns the current application settings as a typed Settings struct.
#[utoipa::path(
    get,
    path =  "/settings",
    tag = "settings",
    responses(
        (status=200, body=GetSettingsResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
#[instrument(name = "api.settings.get", level = "info", skip(state))]
async fn get_settings_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<GetSettingsResponse>, ApiError> {
    info!("get settings request received");
    let app = state.app_facade_or_error()?;
    let settings = app.settings.get().await.map_err(settings_error_to_api)?;

    info!("get settings succeeded");
    Ok(Json(GetSettingsResponse {
        data: settings_view_to_dto(settings),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
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
    path =  "/settings",
    tag = "settings",
    responses(
        (status=200, body=UpdateSettingsResponse),
        (status = 400, description = "Invalid request", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
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
    )
)]
async fn update_settings_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SettingsPatchDto>,
) -> Result<Json<UpdateSettingsResponse>, ApiError> {
    info!("update settings request received");
    let app = state.app_facade_or_error()?;

    // D-D1：`network` 段非空（任何字段变更）触发 restart_required = true。
    // 当前 NetworkSettings 仅含 allow_relay_fallback；后续若加字段，仍走 is_some()
    // 兜底。其它字段（general / sync 等）不影响该信号 — 它们不需要重启。
    //
    // `general.telemetry_enabled` 历史曾通过这里触发 restart（260505-17q），后于
    // 260505-1np 改成运行时 gate（见 uc-observability::set_telemetry_enabled），
    // 不再需要重启 — 下面在 facade 写盘成功后直接把新值推进 atomic 即可立即生效。
    // Pitfall 3 防御：调用方（前端 Phase 95）必须显式承担"还没真正生效"。
    let restart_required = payload.network.is_some();

    // 取出可能存在的 telemetry 新值，再传 patch 给 facade 写盘 — 写盘成功后再
    // 把 atomic 推进新值，保证持久化与运行时状态保持单调一致（如果写盘失败，
    // 也不会污染运行时 gate）。
    let telemetry_update = payload.general.as_ref().and_then(|g| g.telemetry_enabled);

    let updated = app
        .settings
        .update(settings_patch_from_dto(payload))
        .await
        .map_err(settings_error_to_api)?;

    if let Some(enabled) = telemetry_update {
        uc_observability::set_telemetry_enabled(enabled);
    }

    info!(restart_required, "update settings succeeded");
    Ok(Json(UpdateSettingsResponse {
        success: true,
        data: settings_view_to_dto(updated),
        ts: chrono::Utc::now().timestamp_millis(),
        restart_required,
    }))
}

fn settings_error_to_api(err: app_settings::SettingsFacadeError) -> ApiError {
    tracing::error!(error = %err, "settings facade failed");
    ApiError::internal(err.to_string())
}

#[doc(hidden)]
pub fn settings_patch_from_dto(patch: SettingsPatchDto) -> app_settings::SettingsPatch {
    app_settings::SettingsPatch {
        general: patch
            .general
            .map(|general| app_settings::GeneralSettingsPatch {
                auto_start: general.auto_start,
                silent_start: general.silent_start,
                auto_check_update: general.auto_check_update,
                theme: general.theme.map(theme_from_dto),
                theme_color: general.theme_color,
                language: general.language,
                device_name: general.device_name,
                update_channel: general
                    .update_channel
                    .map(|channel| channel.map(update_channel_from_dto)),
                telemetry_enabled: general.telemetry_enabled,
            }),
        sync: patch.sync.map(|sync| app_settings::SyncSettingsPatch {
            auto_sync: sync.auto_sync,
            sync_frequency: sync.sync_frequency.map(sync_frequency_from_dto),
            content_types: sync.content_types.map(content_types_patch_from_dto),
        }),
        retention_policy: patch.retention_policy.map(|retention_policy| {
            app_settings::RetentionPolicyPatch {
                enabled: retention_policy.enabled,
                rules: retention_policy.rules.map(|rules| {
                    rules
                        .into_iter()
                        .map(retention_rule_patch_from_dto)
                        .collect()
                }),
                skip_pinned: retention_policy.skip_pinned,
                evaluation: retention_policy.evaluation.map(rule_evaluation_from_dto),
            }
        }),
        security: patch
            .security
            .map(|security| app_settings::SecuritySettingsPatch {
                encryption_enabled: security.encryption_enabled,
                auto_unlock_enabled: security.auto_unlock_enabled,
            }),
        pairing: patch
            .pairing
            .map(|pairing| app_settings::PairingSettingsPatch {
                step_timeout: pairing.step_timeout,
                user_verification_timeout: pairing.user_verification_timeout,
                session_timeout: pairing.session_timeout,
                max_retries: pairing.max_retries,
            }),
        keyboard_shortcuts: patch.keyboard_shortcuts.map(
            |KeyboardShortcutsPatchDto { shortcuts }| {
                shortcuts
                    .into_iter()
                    .map(|(name, value)| (name, value.map(shortcut_from_dto)))
                    .collect()
            },
        ),
        file_sync: patch
            .file_sync
            .map(|file_sync| app_settings::FileSyncSettingsPatch {
                file_sync_enabled: file_sync.file_sync_enabled,
                small_file_threshold: file_sync.small_file_threshold,
                max_file_size: file_sync.max_file_size,
                file_cache_quota_per_device: file_sync.file_cache_quota_per_device,
                file_retention_hours: file_sync.file_retention_hours,
                file_auto_cleanup: file_sync.file_auto_cleanup,
            }),
        network: patch
            .network
            .map(|network| app_settings::NetworkSettingsPatch {
                allow_relay_fallback: network.allow_relay_fallback,
                allow_overlay_network_addrs: network.allow_overlay_network_addrs,
            }),
    }
}

#[doc(hidden)]
pub fn settings_view_to_dto(value: app_settings::SettingsView) -> SettingsDto {
    SettingsDto {
        schema_version: value.schema_version,
        general: GeneralSettingsDto {
            auto_start: value.general.auto_start,
            silent_start: value.general.silent_start,
            auto_check_update: value.general.auto_check_update,
            theme: theme_to_dto(value.general.theme),
            theme_color: value.general.theme_color,
            language: value.general.language,
            device_name: value.general.device_name,
            update_channel: value.general.update_channel.map(update_channel_to_dto),
            telemetry_enabled: value.general.telemetry_enabled,
        },
        sync: SyncSettingsDto {
            auto_sync: value.sync.auto_sync,
            sync_frequency: sync_frequency_to_dto(value.sync.sync_frequency),
            content_types: content_types_to_dto(value.sync.content_types),
        },
        retention_policy: RetentionPolicyDto {
            enabled: value.retention_policy.enabled,
            rules: value
                .retention_policy
                .rules
                .into_iter()
                .map(retention_rule_to_dto)
                .collect(),
            skip_pinned: value.retention_policy.skip_pinned,
            evaluation: rule_evaluation_to_dto(value.retention_policy.evaluation),
        },
        security: SecuritySettingsDto {
            encryption_enabled: value.security.encryption_enabled,
            passphrase_configured: value.security.passphrase_configured,
            auto_unlock_enabled: value.security.auto_unlock_enabled,
        },
        pairing: PairingSettingsDto {
            step_timeout: value.pairing.step_timeout,
            user_verification_timeout: value.pairing.user_verification_timeout,
            session_timeout: value.pairing.session_timeout,
            max_retries: value.pairing.max_retries,
            protocol_version: value.pairing.protocol_version,
        },
        keyboard_shortcuts: value
            .keyboard_shortcuts
            .into_iter()
            .map(|(name, shortcut)| (name, shortcut_to_dto(shortcut)))
            .collect(),
        file_sync: FileSyncSettingsDto {
            file_sync_enabled: value.file_sync.file_sync_enabled,
            small_file_threshold: value.file_sync.small_file_threshold,
            max_file_size: value.file_sync.max_file_size,
            file_cache_quota_per_device: value.file_sync.file_cache_quota_per_device,
            file_retention_hours: value.file_sync.file_retention_hours,
            file_auto_cleanup: value.file_sync.file_auto_cleanup,
        },
        network: NetworkSettingsDto {
            allow_relay_fallback: value.network.allow_relay_fallback,
            allow_overlay_network_addrs: value.network.allow_overlay_network_addrs,
        },
    }
}

fn content_types_patch_from_dto(value: ContentTypesPatchDto) -> app_settings::ContentTypesPatch {
    app_settings::ContentTypesPatch {
        text: value.text,
        image: value.image,
        link: value.link,
        file: value.file,
        code_snippet: value.code_snippet,
        rich_text: value.rich_text,
    }
}

fn content_types_to_dto(value: app_settings::ContentTypesView) -> ContentTypesDto {
    ContentTypesDto {
        text: value.text,
        image: value.image,
        link: value.link,
        file: value.file,
        code_snippet: value.code_snippet,
        rich_text: value.rich_text,
    }
}

fn retention_rule_patch_from_dto(value: RetentionRuleDto) -> app_settings::RetentionRulePatchValue {
    match value {
        RetentionRuleDto::ByAge { max_age } => {
            app_settings::RetentionRulePatchValue::ByAge { max_age }
        }
        RetentionRuleDto::ByCount { max_items } => {
            app_settings::RetentionRulePatchValue::ByCount { max_items }
        }
        RetentionRuleDto::ByContentType {
            content_type,
            max_age,
        } => app_settings::RetentionRulePatchValue::ByContentType {
            content_type: app_settings::ContentTypesView {
                text: content_type.text,
                image: content_type.image,
                link: content_type.link,
                file: content_type.file,
                code_snippet: content_type.code_snippet,
                rich_text: content_type.rich_text,
            },
            max_age,
        },
        RetentionRuleDto::ByTotalSize { max_bytes } => {
            app_settings::RetentionRulePatchValue::ByTotalSize { max_bytes }
        }
        RetentionRuleDto::Sensitive { max_age } => {
            app_settings::RetentionRulePatchValue::Sensitive { max_age }
        }
    }
}

fn retention_rule_to_dto(value: app_settings::RetentionRuleView) -> RetentionRuleDto {
    match value {
        app_settings::RetentionRuleView::ByAge { max_age } => RetentionRuleDto::ByAge { max_age },
        app_settings::RetentionRuleView::ByCount { max_items } => {
            RetentionRuleDto::ByCount { max_items }
        }
        app_settings::RetentionRuleView::ByContentType {
            content_type,
            max_age,
        } => RetentionRuleDto::ByContentType {
            content_type: content_types_to_dto(content_type),
            max_age,
        },
        app_settings::RetentionRuleView::ByTotalSize { max_bytes } => {
            RetentionRuleDto::ByTotalSize { max_bytes }
        }
        app_settings::RetentionRuleView::Sensitive { max_age } => {
            RetentionRuleDto::Sensitive { max_age }
        }
    }
}

fn theme_from_dto(value: crate::api::dto::settings::ThemeDto) -> app_settings::ThemeView {
    match value {
        crate::api::dto::settings::ThemeDto::Light => app_settings::ThemeView::Light,
        crate::api::dto::settings::ThemeDto::Dark => app_settings::ThemeView::Dark,
        crate::api::dto::settings::ThemeDto::System => app_settings::ThemeView::System,
    }
}

fn theme_to_dto(value: app_settings::ThemeView) -> crate::api::dto::settings::ThemeDto {
    match value {
        app_settings::ThemeView::Light => crate::api::dto::settings::ThemeDto::Light,
        app_settings::ThemeView::Dark => crate::api::dto::settings::ThemeDto::Dark,
        app_settings::ThemeView::System => crate::api::dto::settings::ThemeDto::System,
    }
}

fn update_channel_from_dto(
    value: crate::api::dto::settings::UpdateChannelDto,
) -> app_settings::UpdateChannelView {
    match value {
        crate::api::dto::settings::UpdateChannelDto::Stable => {
            app_settings::UpdateChannelView::Stable
        }
        crate::api::dto::settings::UpdateChannelDto::Alpha => {
            app_settings::UpdateChannelView::Alpha
        }
        crate::api::dto::settings::UpdateChannelDto::Beta => app_settings::UpdateChannelView::Beta,
        crate::api::dto::settings::UpdateChannelDto::Rc => app_settings::UpdateChannelView::Rc,
    }
}

fn update_channel_to_dto(
    value: app_settings::UpdateChannelView,
) -> crate::api::dto::settings::UpdateChannelDto {
    match value {
        app_settings::UpdateChannelView::Stable => {
            crate::api::dto::settings::UpdateChannelDto::Stable
        }
        app_settings::UpdateChannelView::Alpha => {
            crate::api::dto::settings::UpdateChannelDto::Alpha
        }
        app_settings::UpdateChannelView::Beta => crate::api::dto::settings::UpdateChannelDto::Beta,
        app_settings::UpdateChannelView::Rc => crate::api::dto::settings::UpdateChannelDto::Rc,
    }
}

fn sync_frequency_from_dto(
    value: crate::api::dto::settings::SyncFrequencyDto,
) -> app_settings::SyncFrequencyView {
    match value {
        crate::api::dto::settings::SyncFrequencyDto::Realtime => {
            app_settings::SyncFrequencyView::Realtime
        }
        crate::api::dto::settings::SyncFrequencyDto::Interval => {
            app_settings::SyncFrequencyView::Interval
        }
    }
}

fn sync_frequency_to_dto(
    value: app_settings::SyncFrequencyView,
) -> crate::api::dto::settings::SyncFrequencyDto {
    match value {
        app_settings::SyncFrequencyView::Realtime => {
            crate::api::dto::settings::SyncFrequencyDto::Realtime
        }
        app_settings::SyncFrequencyView::Interval => {
            crate::api::dto::settings::SyncFrequencyDto::Interval
        }
    }
}

fn rule_evaluation_from_dto(
    value: crate::api::dto::settings::RuleEvaluationDto,
) -> app_settings::RuleEvaluationView {
    match value {
        crate::api::dto::settings::RuleEvaluationDto::AnyMatch => {
            app_settings::RuleEvaluationView::AnyMatch
        }
        crate::api::dto::settings::RuleEvaluationDto::AllMatch => {
            app_settings::RuleEvaluationView::AllMatch
        }
    }
}

fn rule_evaluation_to_dto(
    value: app_settings::RuleEvaluationView,
) -> crate::api::dto::settings::RuleEvaluationDto {
    match value {
        app_settings::RuleEvaluationView::AnyMatch => {
            crate::api::dto::settings::RuleEvaluationDto::AnyMatch
        }
        app_settings::RuleEvaluationView::AllMatch => {
            crate::api::dto::settings::RuleEvaluationDto::AllMatch
        }
    }
}

fn shortcut_from_dto(
    value: crate::api::dto::settings::ShortcutKeyDto,
) -> app_settings::ShortcutKeyView {
    match value {
        crate::api::dto::settings::ShortcutKeyDto::Single(v) => {
            app_settings::ShortcutKeyView::Single(v)
        }
        crate::api::dto::settings::ShortcutKeyDto::Multiple(v) => {
            app_settings::ShortcutKeyView::Multiple(v)
        }
    }
}

fn shortcut_to_dto(
    value: app_settings::ShortcutKeyView,
) -> crate::api::dto::settings::ShortcutKeyDto {
    match value {
        app_settings::ShortcutKeyView::Single(v) => {
            crate::api::dto::settings::ShortcutKeyDto::Single(v)
        }
        app_settings::ShortcutKeyView::Multiple(v) => {
            crate::api::dto::settings::ShortcutKeyDto::Multiple(v)
        }
    }
}
