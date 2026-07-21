use std::time::Duration;

use uc_application::facade::settings as app;
use uc_application::facade::AppFacade;
use uc_core::settings::model::ShortcutKey;

use crate::{
    CongestionControllerSummary, EngineError, EngineErrorCategory, FileSyncSettingsSummary,
    GeneralSettingsSummary, NetworkSettingsSummary, OperationResult, PairingSettingsSummary,
    QuickPanelDoubleTapModifierSummary, QuickPanelPositionSummary, QuickPanelSettingsSummary,
    RelayProbeInput, RelayProbeOutcome, RetentionPolicySummary, RetentionRulePatch,
    RetentionRuleSummary, RuleEvaluationSummary, SecuritySettingsSummary, SettingsContentTypes,
    SettingsContentTypesPatch, SettingsPatch, SettingsSummary, SettingsUpdateOutcome,
    ShortcutKeySummary, StartupModeSummary, SyncFrequencySummary, SyncSettingsSummary,
    ThemeSummary, UpdateChannelSummary,
};

pub const QUERY_SETTINGS_FAILED_CODE: u32 = 1391;
pub const UPDATE_SETTINGS_FAILED_CODE: u32 = 1392;
pub const PROBE_RELAY_UNAVAILABLE_CODE: u32 = 1393;

pub(crate) async fn execute_query_settings(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let settings = facade
        .settings
        .get()
        .await
        .map_err(|_| internal_error(QUERY_SETTINGS_FAILED_CODE))?;
    Ok(OperationResult::Settings(Box::new(map_settings(settings))))
}

pub(crate) async fn execute_update_settings(
    facade: &AppFacade,
    patch: SettingsPatch,
) -> Result<OperationResult, EngineError> {
    let patch = match map_patch(patch) {
        Ok(patch) => patch,
        Err(reason) => {
            return Ok(OperationResult::SettingsUpdated(
                SettingsUpdateOutcome::Rejected { reason },
            ));
        }
    };
    match facade.settings.update(patch).await {
        Ok(settings) => Ok(OperationResult::SettingsUpdated(
            SettingsUpdateOutcome::Updated(Box::new(map_settings(settings))),
        )),
        Err(app::SettingsFacadeError::Invalid(reason)) => Ok(OperationResult::SettingsUpdated(
            SettingsUpdateOutcome::Rejected { reason },
        )),
        Err(_) => Err(internal_error(UPDATE_SETTINGS_FAILED_CODE)),
    }
}

pub(crate) async fn execute_probe_relay(
    facade: &AppFacade,
    input: RelayProbeInput,
) -> Result<OperationResult, EngineError> {
    let outcome = match facade.settings.probe_relay_url(&input.url).await {
        Ok(report) => RelayProbeOutcome::Success {
            latency_ms: report.latency_ms,
        },
        Err(app::SettingsFacadeError::RelayProbeInvalidUrl(message)) => {
            RelayProbeOutcome::InvalidUrl { message }
        }
        Err(app::SettingsFacadeError::RelayProbeDns(message)) => RelayProbeOutcome::Dns { message },
        Err(app::SettingsFacadeError::RelayProbeTls(message)) => RelayProbeOutcome::Tls { message },
        Err(app::SettingsFacadeError::RelayProbeHandshake(message)) => {
            RelayProbeOutcome::Handshake { message }
        }
        Err(app::SettingsFacadeError::RelayProbeTimeout) => RelayProbeOutcome::Timeout,
        Err(app::SettingsFacadeError::RelayProbeOther(message)) => {
            RelayProbeOutcome::Other { message }
        }
        Err(app::SettingsFacadeError::RelayProbeUnavailable) => {
            return Err(EngineError::new(
                PROBE_RELAY_UNAVAILABLE_CODE,
                EngineErrorCategory::Unavailable,
                false,
            ));
        }
        Err(_) => return Err(internal_error(PROBE_RELAY_UNAVAILABLE_CODE)),
    };
    Ok(OperationResult::RelayProbed(outcome))
}

fn map_settings(settings: app::SettingsView) -> SettingsSummary {
    SettingsSummary {
        schema_version: settings.schema_version,
        general: GeneralSettingsSummary {
            auto_start: settings.general.auto_start,
            startup_mode: map_startup_mode(settings.general.startup_mode),
            restore_last_entry_on_startup: settings.general.restore_last_entry_on_startup,
            auto_check_update: settings.general.auto_check_update,
            auto_download_update: settings.general.auto_download_update,
            theme: map_theme(settings.general.theme),
            theme_color: settings.general.theme_color,
            theme_color_light: settings.general.theme_color_light,
            theme_color_dark: settings.general.theme_color_dark,
            theme_overrides_light: settings.general.theme_overrides_light,
            theme_overrides_dark: settings.general.theme_overrides_dark,
            language: settings.general.language,
            device_name: settings.general.device_name,
            update_channel: settings.general.update_channel.map(map_update_channel),
            telemetry_enabled: settings.general.telemetry_enabled,
            usage_analytics_enabled: settings.general.usage_analytics_enabled,
            debug_mode: settings.general.debug_mode,
        },
        sync: SyncSettingsSummary {
            auto_sync: settings.sync.auto_sync,
            sync_frequency: map_sync_frequency(settings.sync.sync_frequency),
            content_types: map_content_types(settings.sync.content_types),
            sync_on_restore: settings.sync.sync_on_restore,
        },
        retention_policy: RetentionPolicySummary {
            enabled: settings.retention_policy.enabled,
            rules: settings
                .retention_policy
                .rules
                .into_iter()
                .map(map_retention_rule)
                .collect(),
            skip_pinned: settings.retention_policy.skip_pinned,
            evaluation: map_rule_evaluation(settings.retention_policy.evaluation),
        },
        security: SecuritySettingsSummary {
            encryption_enabled: settings.security.encryption_enabled,
            passphrase_configured: settings.security.passphrase_configured,
            auto_unlock_enabled: settings.security.auto_unlock_enabled,
        },
        pairing: PairingSettingsSummary {
            step_timeout_secs: settings.pairing.step_timeout.as_secs(),
            user_verification_timeout_secs: settings.pairing.user_verification_timeout.as_secs(),
            session_timeout_secs: settings.pairing.session_timeout.as_secs(),
            max_retries: settings.pairing.max_retries,
            protocol_version: settings.pairing.protocol_version,
        },
        keyboard_shortcuts: settings
            .keyboard_shortcuts
            .into_iter()
            .map(|(name, shortcut)| (name, map_shortcut(shortcut)))
            .collect(),
        file_sync: FileSyncSettingsSummary {
            file_sync_enabled: settings.file_sync.file_sync_enabled,
            small_file_threshold: settings.file_sync.small_file_threshold,
            max_file_size: settings.file_sync.max_file_size,
            file_cache_quota_per_device: settings.file_sync.file_cache_quota_per_device,
            file_retention_hours: settings.file_sync.file_retention_hours,
            file_auto_cleanup: settings.file_sync.file_auto_cleanup,
            max_file_set_total_bytes: settings.file_sync.max_file_set_total_bytes,
            max_file_set_member_count: settings.file_sync.max_file_set_member_count,
            auto_save_dir: settings.file_sync.auto_save_dir,
        },
        network: NetworkSettingsSummary {
            allow_relay_fallback: settings.network.allow_relay_fallback,
            allow_overlay_network_addrs: settings.network.allow_overlay_network_addrs,
            custom_relay_urls: settings.network.custom_relay_urls,
            congestion_controller: map_congestion_controller(
                settings.network.congestion_controller,
            ),
        },
        quick_panel: QuickPanelSettingsSummary {
            enabled: settings.quick_panel.enabled,
            position: map_quick_panel_position(settings.quick_panel.position),
            double_tap_modifier: map_double_tap_modifier(settings.quick_panel.double_tap_modifier),
        },
    }
}

fn map_patch(patch: SettingsPatch) -> Result<app::SettingsPatch, String> {
    Ok(app::SettingsPatch {
        general: patch.general.map(|value| app::GeneralSettingsPatch {
            auto_start: value.auto_start,
            startup_mode: value.startup_mode.map(unmap_startup_mode),
            restore_last_entry_on_startup: value.restore_last_entry_on_startup,
            auto_check_update: value.auto_check_update,
            auto_download_update: value.auto_download_update,
            theme: value.theme.map(unmap_theme),
            theme_color: value.theme_color,
            theme_color_light: value.theme_color_light,
            theme_color_dark: value.theme_color_dark,
            theme_overrides_light: value.theme_overrides_light,
            theme_overrides_dark: value.theme_overrides_dark,
            language: value.language,
            device_name: value.device_name,
            update_channel: value
                .update_channel
                .map(|channel| channel.map(unmap_update_channel)),
            telemetry_enabled: value.telemetry_enabled,
            usage_analytics_enabled: value.usage_analytics_enabled,
            debug_mode: value.debug_mode,
        }),
        sync: patch.sync.map(|value| app::SyncSettingsPatch {
            auto_sync: value.auto_sync,
            sync_frequency: value.sync_frequency.map(unmap_sync_frequency),
            content_types: value.content_types.map(unmap_content_types_patch),
            sync_on_restore: value.sync_on_restore,
        }),
        retention_policy: patch
            .retention_policy
            .map(|value| -> Result<_, String> {
                Ok(app::RetentionPolicyPatch {
                    enabled: value.enabled,
                    rules: value
                        .rules
                        .map(|rules| {
                            rules
                                .into_iter()
                                .map(unmap_retention_rule)
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .transpose()?,
                    skip_pinned: value.skip_pinned,
                    evaluation: value.evaluation.map(unmap_rule_evaluation),
                })
            })
            .transpose()?,
        security: patch.security.map(|value| app::SecuritySettingsPatch {
            encryption_enabled: value.encryption_enabled,
            auto_unlock_enabled: value.auto_unlock_enabled,
        }),
        pairing: patch.pairing.map(|value| app::PairingSettingsPatch {
            step_timeout: value.step_timeout_secs.map(Duration::from_secs),
            user_verification_timeout: value
                .user_verification_timeout_secs
                .map(Duration::from_secs),
            session_timeout: value.session_timeout_secs.map(Duration::from_secs),
            max_retries: value.max_retries,
        }),
        keyboard_shortcuts: patch.keyboard_shortcuts.map(|shortcuts| {
            shortcuts
                .into_iter()
                .map(|(name, shortcut)| (name, shortcut.map(unmap_shortcut)))
                .collect()
        }),
        file_sync: patch.file_sync.map(|value| app::FileSyncSettingsPatch {
            file_sync_enabled: value.file_sync_enabled,
            small_file_threshold: value.small_file_threshold,
            max_file_size: value.max_file_size,
            file_cache_quota_per_device: value.file_cache_quota_per_device,
            file_retention_hours: value.file_retention_hours,
            file_auto_cleanup: value.file_auto_cleanup,
            max_file_set_total_bytes: value.max_file_set_total_bytes,
            max_file_set_member_count: value.max_file_set_member_count,
            auto_save_dir: value.auto_save_dir,
        }),
        network: patch.network.map(|value| app::NetworkSettingsPatch {
            allow_relay_fallback: value.allow_relay_fallback,
            allow_overlay_network_addrs: value.allow_overlay_network_addrs,
            custom_relay_urls: value.custom_relay_urls,
            congestion_controller: value.congestion_controller.map(unmap_congestion_controller),
        }),
        quick_panel: patch.quick_panel.map(|value| app::QuickPanelSettingsPatch {
            enabled: value.enabled,
            position: value.position.map(unmap_quick_panel_position),
            double_tap_modifier: value.double_tap_modifier.map(unmap_double_tap_modifier),
        }),
    })
}

fn map_theme(value: app::ThemeView) -> ThemeSummary {
    match value {
        app::ThemeView::Light => ThemeSummary::Light,
        app::ThemeView::Dark => ThemeSummary::Dark,
        app::ThemeView::System => ThemeSummary::System,
    }
}

fn unmap_theme(value: ThemeSummary) -> app::ThemeView {
    match value {
        ThemeSummary::Light => app::ThemeView::Light,
        ThemeSummary::Dark => app::ThemeView::Dark,
        ThemeSummary::System => app::ThemeView::System,
    }
}

fn map_startup_mode(value: app::StartupModeView) -> StartupModeSummary {
    match value {
        app::StartupModeView::Normal => StartupModeSummary::Normal,
        app::StartupModeView::Silent => StartupModeSummary::Silent,
        app::StartupModeView::Lightweight => StartupModeSummary::Lightweight,
    }
}

fn unmap_startup_mode(value: StartupModeSummary) -> app::StartupModeView {
    match value {
        StartupModeSummary::Normal => app::StartupModeView::Normal,
        StartupModeSummary::Silent => app::StartupModeView::Silent,
        StartupModeSummary::Lightweight => app::StartupModeView::Lightweight,
    }
}

fn map_update_channel(value: app::UpdateChannelView) -> UpdateChannelSummary {
    match value {
        app::UpdateChannelView::Stable => UpdateChannelSummary::Stable,
        app::UpdateChannelView::Alpha => UpdateChannelSummary::Alpha,
        app::UpdateChannelView::Beta => UpdateChannelSummary::Beta,
        app::UpdateChannelView::Rc => UpdateChannelSummary::Rc,
    }
}

fn unmap_update_channel(value: UpdateChannelSummary) -> app::UpdateChannelView {
    match value {
        UpdateChannelSummary::Stable => app::UpdateChannelView::Stable,
        UpdateChannelSummary::Alpha => app::UpdateChannelView::Alpha,
        UpdateChannelSummary::Beta => app::UpdateChannelView::Beta,
        UpdateChannelSummary::Rc => app::UpdateChannelView::Rc,
    }
}

fn map_sync_frequency(value: app::SyncFrequencyView) -> SyncFrequencySummary {
    match value {
        app::SyncFrequencyView::Realtime => SyncFrequencySummary::Realtime,
        app::SyncFrequencyView::Interval => SyncFrequencySummary::Interval,
    }
}

fn unmap_sync_frequency(value: SyncFrequencySummary) -> app::SyncFrequencyView {
    match value {
        SyncFrequencySummary::Realtime => app::SyncFrequencyView::Realtime,
        SyncFrequencySummary::Interval => app::SyncFrequencyView::Interval,
    }
}

fn map_rule_evaluation(value: app::RuleEvaluationView) -> RuleEvaluationSummary {
    match value {
        app::RuleEvaluationView::AnyMatch => RuleEvaluationSummary::AnyMatch,
        app::RuleEvaluationView::AllMatch => RuleEvaluationSummary::AllMatch,
    }
}

fn unmap_rule_evaluation(value: RuleEvaluationSummary) -> app::RuleEvaluationView {
    match value {
        RuleEvaluationSummary::AnyMatch => app::RuleEvaluationView::AnyMatch,
        RuleEvaluationSummary::AllMatch => app::RuleEvaluationView::AllMatch,
    }
}

fn map_content_types(value: app::ContentTypesView) -> SettingsContentTypes {
    SettingsContentTypes {
        text: value.text,
        image: value.image,
        link: value.link,
        file: value.file,
        code_snippet: value.code_snippet,
        rich_text: value.rich_text,
    }
}

fn unmap_content_types(value: SettingsContentTypes) -> app::ContentTypesView {
    app::ContentTypesView {
        text: value.text,
        image: value.image,
        link: value.link,
        file: value.file,
        code_snippet: value.code_snippet,
        rich_text: value.rich_text,
    }
}

fn unmap_content_types_patch(value: SettingsContentTypesPatch) -> app::ContentTypesPatch {
    app::ContentTypesPatch {
        text: value.text,
        image: value.image,
        link: value.link,
        file: value.file,
        code_snippet: value.code_snippet,
        rich_text: value.rich_text,
    }
}

fn map_retention_rule(value: app::RetentionRuleView) -> RetentionRuleSummary {
    match value {
        app::RetentionRuleView::ByAge { max_age } => RetentionRuleSummary::ByAge {
            max_age_secs: max_age.as_secs(),
        },
        app::RetentionRuleView::ByCount { max_items } => RetentionRuleSummary::ByCount {
            max_items: max_items as u64,
        },
        app::RetentionRuleView::ByContentType {
            content_type,
            max_age,
        } => RetentionRuleSummary::ByContentType {
            content_type: map_content_types(content_type),
            max_age_secs: max_age.as_secs(),
        },
        app::RetentionRuleView::ByTotalSize { max_bytes } => {
            RetentionRuleSummary::ByTotalSize { max_bytes }
        }
        app::RetentionRuleView::Sensitive { max_age } => RetentionRuleSummary::Sensitive {
            max_age_secs: max_age.as_secs(),
        },
    }
}

fn unmap_retention_rule(value: RetentionRulePatch) -> Result<app::RetentionRulePatchValue, String> {
    Ok(match value {
        RetentionRulePatch::ByAge { max_age_secs } => app::RetentionRulePatchValue::ByAge {
            max_age: Duration::from_secs(max_age_secs),
        },
        RetentionRulePatch::ByCount { max_items } => app::RetentionRulePatchValue::ByCount {
            max_items: usize::try_from(max_items)
                .map_err(|_| "retention count exceeds this platform's limit".to_string())?,
        },
        RetentionRulePatch::ByContentType {
            content_type,
            max_age_secs,
        } => app::RetentionRulePatchValue::ByContentType {
            content_type: unmap_content_types(content_type),
            max_age: Duration::from_secs(max_age_secs),
        },
        RetentionRulePatch::ByTotalSize { max_bytes } => {
            app::RetentionRulePatchValue::ByTotalSize { max_bytes }
        }
        RetentionRulePatch::Sensitive { max_age_secs } => app::RetentionRulePatchValue::Sensitive {
            max_age: Duration::from_secs(max_age_secs),
        },
    })
}

fn map_shortcut(value: ShortcutKey) -> ShortcutKeySummary {
    match value {
        ShortcutKey::Single(value) => ShortcutKeySummary::Single(value),
        ShortcutKey::Multiple(values) => ShortcutKeySummary::Multiple(values),
    }
}

fn unmap_shortcut(value: ShortcutKeySummary) -> ShortcutKey {
    match value {
        ShortcutKeySummary::Single(value) => ShortcutKey::Single(value),
        ShortcutKeySummary::Multiple(values) => ShortcutKey::Multiple(values),
    }
}

fn map_congestion_controller(value: app::CongestionControllerView) -> CongestionControllerSummary {
    match value {
        app::CongestionControllerView::Cubic => CongestionControllerSummary::Cubic,
        app::CongestionControllerView::Bbr3 => CongestionControllerSummary::Bbr3,
    }
}

fn unmap_congestion_controller(
    value: CongestionControllerSummary,
) -> app::CongestionControllerView {
    match value {
        CongestionControllerSummary::Cubic => app::CongestionControllerView::Cubic,
        CongestionControllerSummary::Bbr3 => app::CongestionControllerView::Bbr3,
    }
}

fn map_quick_panel_position(value: app::QuickPanelPositionView) -> QuickPanelPositionSummary {
    match value {
        app::QuickPanelPositionView::Center => QuickPanelPositionSummary::Center,
        app::QuickPanelPositionView::FollowCursor => QuickPanelPositionSummary::FollowCursor,
    }
}

fn unmap_quick_panel_position(value: QuickPanelPositionSummary) -> app::QuickPanelPositionView {
    match value {
        QuickPanelPositionSummary::Center => app::QuickPanelPositionView::Center,
        QuickPanelPositionSummary::FollowCursor => app::QuickPanelPositionView::FollowCursor,
    }
}

fn map_double_tap_modifier(
    value: app::QuickPanelDoubleTapModifierView,
) -> QuickPanelDoubleTapModifierSummary {
    match value {
        app::QuickPanelDoubleTapModifierView::Disabled => {
            QuickPanelDoubleTapModifierSummary::Disabled
        }
        app::QuickPanelDoubleTapModifierView::Alt => QuickPanelDoubleTapModifierSummary::Alt,
        app::QuickPanelDoubleTapModifierView::Control => {
            QuickPanelDoubleTapModifierSummary::Control
        }
        app::QuickPanelDoubleTapModifierView::Meta => QuickPanelDoubleTapModifierSummary::Meta,
    }
}

fn unmap_double_tap_modifier(
    value: QuickPanelDoubleTapModifierSummary,
) -> app::QuickPanelDoubleTapModifierView {
    match value {
        QuickPanelDoubleTapModifierSummary::Disabled => {
            app::QuickPanelDoubleTapModifierView::Disabled
        }
        QuickPanelDoubleTapModifierSummary::Alt => app::QuickPanelDoubleTapModifierView::Alt,
        QuickPanelDoubleTapModifierSummary::Control => {
            app::QuickPanelDoubleTapModifierView::Control
        }
        QuickPanelDoubleTapModifierSummary::Meta => app::QuickPanelDoubleTapModifierView::Meta,
    }
}

fn internal_error(code: u32) -> EngineError {
    EngineError::new(code, EngineErrorCategory::Internal, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_patch_preserves_clear_and_unchanged_states() {
        let patch = SettingsPatch {
            general: Some(crate::GeneralSettingsPatch {
                device_name: Some(None),
                language: None,
                ..Default::default()
            }),
            ..Default::default()
        };

        let mapped = map_patch(patch).expect("valid patch");
        let general = mapped.general.expect("general patch");
        assert_eq!(general.device_name, Some(None));
        assert_eq!(general.language, None);
    }

    #[test]
    fn oversized_retention_count_is_a_business_rejection() {
        if usize::BITS >= 64 {
            return;
        }
        let patch = SettingsPatch {
            retention_policy: Some(crate::RetentionPolicyPatch {
                rules: Some(vec![RetentionRulePatch::ByCount {
                    max_items: u64::MAX,
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(map_patch(patch).is_err());
    }

    #[test]
    fn shortcut_debug_output_redacts_key_values() {
        let shortcut = ShortcutKeySummary::Multiple(vec!["Private+Key".to_string()]);
        assert!(!format!("{shortcut:?}").contains("Private+Key"));
    }
}
