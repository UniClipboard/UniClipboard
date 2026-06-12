//! Settings boundary projections: `SettingsFacade` views ↔ settings DTOs.

use uc_application::facade::settings as app_settings;

use super::{IntoApiDto, IntoDomain};
use crate::api::dto::settings::{
    ContentTypesDto, ContentTypesPatchDto, FileSyncSettingsDto, GeneralSettingsDto,
    KeyboardShortcutsPatchDto, NetworkSettingsDto, PairingSettingsDto, QuickPanelPositionDto,
    QuickPanelSettingsDto, RetentionPolicyDto, RetentionRuleDto, RuleEvaluationDto,
    SecuritySettingsDto, SettingsDto, SettingsPatchDto, ShortcutKeyDto, SyncFrequencyDto,
    SyncSettingsDto, ThemeDto, UpdateChannelDto,
};

impl IntoDomain<app_settings::SettingsPatch> for SettingsPatchDto {
    fn into_domain(self) -> app_settings::SettingsPatch {
        app_settings::SettingsPatch {
            general: self
                .general
                .map(|general| app_settings::GeneralSettingsPatch {
                    auto_start: general.auto_start,
                    silent_start: general.silent_start,
                    auto_check_update: general.auto_check_update,
                    auto_download_update: general.auto_download_update,
                    theme: general.theme.map(IntoDomain::into_domain),
                    theme_color: general.theme_color,
                    theme_color_light: general.theme_color_light,
                    theme_color_dark: general.theme_color_dark,
                    theme_overrides_light: general.theme_overrides_light,
                    theme_overrides_dark: general.theme_overrides_dark,
                    language: general.language,
                    device_name: general.device_name,
                    update_channel: general
                        .update_channel
                        .map(|channel| channel.map(IntoDomain::into_domain)),
                    telemetry_enabled: general.telemetry_enabled,
                    usage_analytics_enabled: general.usage_analytics_enabled,
                }),
            sync: self.sync.map(|sync| app_settings::SyncSettingsPatch {
                auto_sync: sync.auto_sync,
                sync_frequency: sync.sync_frequency.map(IntoDomain::into_domain),
                content_types: sync.content_types.map(IntoDomain::into_domain),
            }),
            retention_policy: self.retention_policy.map(|retention_policy| {
                app_settings::RetentionPolicyPatch {
                    enabled: retention_policy.enabled,
                    rules: retention_policy
                        .rules
                        .map(|rules| rules.into_iter().map(IntoDomain::into_domain).collect()),
                    skip_pinned: retention_policy.skip_pinned,
                    evaluation: retention_policy.evaluation.map(IntoDomain::into_domain),
                }
            }),
            security: self
                .security
                .map(|security| app_settings::SecuritySettingsPatch {
                    encryption_enabled: security.encryption_enabled,
                    auto_unlock_enabled: security.auto_unlock_enabled,
                }),
            pairing: self
                .pairing
                .map(|pairing| app_settings::PairingSettingsPatch {
                    step_timeout: pairing.step_timeout,
                    user_verification_timeout: pairing.user_verification_timeout,
                    session_timeout: pairing.session_timeout,
                    max_retries: pairing.max_retries,
                }),
            keyboard_shortcuts: self.keyboard_shortcuts.map(
                |KeyboardShortcutsPatchDto { shortcuts }| {
                    shortcuts
                        .into_iter()
                        .map(|(name, value)| (name, value.map(IntoDomain::into_domain)))
                        .collect()
                },
            ),
            file_sync: self
                .file_sync
                .map(|file_sync| app_settings::FileSyncSettingsPatch {
                    file_sync_enabled: file_sync.file_sync_enabled,
                    small_file_threshold: file_sync.small_file_threshold,
                    max_file_size: file_sync.max_file_size,
                    file_cache_quota_per_device: file_sync.file_cache_quota_per_device,
                    file_retention_hours: file_sync.file_retention_hours,
                    file_auto_cleanup: file_sync.file_auto_cleanup,
                }),
            network: self
                .network
                .map(|network| app_settings::NetworkSettingsPatch {
                    allow_relay_fallback: network.allow_relay_fallback,
                    allow_overlay_network_addrs: network.allow_overlay_network_addrs,
                    custom_relay_urls: network.custom_relay_urls,
                }),
            quick_panel: self.quick_panel.map(|quick_panel| {
                app_settings::QuickPanelSettingsPatch {
                    enabled: quick_panel.enabled,
                    position: quick_panel.position.map(IntoDomain::into_domain),
                }
            }),
        }
    }
}

impl IntoApiDto<SettingsDto> for app_settings::SettingsView {
    fn into_api_dto(self) -> SettingsDto {
        SettingsDto {
            schema_version: self.schema_version,
            general: GeneralSettingsDto {
                auto_start: self.general.auto_start,
                silent_start: self.general.silent_start,
                auto_check_update: self.general.auto_check_update,
                auto_download_update: self.general.auto_download_update,
                theme: self.general.theme.into_api_dto(),
                theme_color: self.general.theme_color,
                theme_color_light: self.general.theme_color_light,
                theme_color_dark: self.general.theme_color_dark,
                theme_overrides_light: self.general.theme_overrides_light,
                theme_overrides_dark: self.general.theme_overrides_dark,
                language: self.general.language,
                device_name: self.general.device_name,
                update_channel: self.general.update_channel.map(IntoApiDto::into_api_dto),
                telemetry_enabled: self.general.telemetry_enabled,
                usage_analytics_enabled: self.general.usage_analytics_enabled,
            },
            sync: SyncSettingsDto {
                auto_sync: self.sync.auto_sync,
                sync_frequency: self.sync.sync_frequency.into_api_dto(),
                content_types: self.sync.content_types.into_api_dto(),
            },
            retention_policy: RetentionPolicyDto {
                enabled: self.retention_policy.enabled,
                rules: self
                    .retention_policy
                    .rules
                    .into_iter()
                    .map(IntoApiDto::into_api_dto)
                    .collect(),
                skip_pinned: self.retention_policy.skip_pinned,
                evaluation: self.retention_policy.evaluation.into_api_dto(),
            },
            security: SecuritySettingsDto {
                encryption_enabled: self.security.encryption_enabled,
                passphrase_configured: self.security.passphrase_configured,
                auto_unlock_enabled: self.security.auto_unlock_enabled,
            },
            pairing: PairingSettingsDto {
                step_timeout: self.pairing.step_timeout,
                user_verification_timeout: self.pairing.user_verification_timeout,
                session_timeout: self.pairing.session_timeout,
                max_retries: self.pairing.max_retries,
                protocol_version: self.pairing.protocol_version,
            },
            keyboard_shortcuts: self
                .keyboard_shortcuts
                .into_iter()
                .map(|(name, shortcut)| (name, shortcut.into_api_dto()))
                .collect(),
            file_sync: FileSyncSettingsDto {
                file_sync_enabled: self.file_sync.file_sync_enabled,
                small_file_threshold: self.file_sync.small_file_threshold,
                max_file_size: self.file_sync.max_file_size,
                file_cache_quota_per_device: self.file_sync.file_cache_quota_per_device,
                file_retention_hours: self.file_sync.file_retention_hours,
                file_auto_cleanup: self.file_sync.file_auto_cleanup,
            },
            network: NetworkSettingsDto {
                allow_relay_fallback: self.network.allow_relay_fallback,
                allow_overlay_network_addrs: self.network.allow_overlay_network_addrs,
                custom_relay_urls: self.network.custom_relay_urls,
            },
            quick_panel: QuickPanelSettingsDto {
                enabled: self.quick_panel.enabled,
                position: self.quick_panel.position.into_api_dto(),
            },
        }
    }
}

impl IntoDomain<app_settings::ContentTypesPatch> for ContentTypesPatchDto {
    fn into_domain(self) -> app_settings::ContentTypesPatch {
        app_settings::ContentTypesPatch {
            text: self.text,
            image: self.image,
            link: self.link,
            file: self.file,
            code_snippet: self.code_snippet,
            rich_text: self.rich_text,
        }
    }
}

impl IntoApiDto<ContentTypesDto> for app_settings::ContentTypesView {
    fn into_api_dto(self) -> ContentTypesDto {
        ContentTypesDto {
            text: self.text,
            image: self.image,
            link: self.link,
            file: self.file,
            code_snippet: self.code_snippet,
            rich_text: self.rich_text,
        }
    }
}

impl IntoDomain<app_settings::RetentionRulePatchValue> for RetentionRuleDto {
    fn into_domain(self) -> app_settings::RetentionRulePatchValue {
        match self {
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
}

impl IntoApiDto<RetentionRuleDto> for app_settings::RetentionRuleView {
    fn into_api_dto(self) -> RetentionRuleDto {
        match self {
            app_settings::RetentionRuleView::ByAge { max_age } => {
                RetentionRuleDto::ByAge { max_age }
            }
            app_settings::RetentionRuleView::ByCount { max_items } => {
                RetentionRuleDto::ByCount { max_items }
            }
            app_settings::RetentionRuleView::ByContentType {
                content_type,
                max_age,
            } => RetentionRuleDto::ByContentType {
                content_type: content_type.into_api_dto(),
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
}

impl IntoDomain<app_settings::ThemeView> for ThemeDto {
    fn into_domain(self) -> app_settings::ThemeView {
        match self {
            ThemeDto::Light => app_settings::ThemeView::Light,
            ThemeDto::Dark => app_settings::ThemeView::Dark,
            ThemeDto::System => app_settings::ThemeView::System,
        }
    }
}

impl IntoApiDto<ThemeDto> for app_settings::ThemeView {
    fn into_api_dto(self) -> ThemeDto {
        match self {
            app_settings::ThemeView::Light => ThemeDto::Light,
            app_settings::ThemeView::Dark => ThemeDto::Dark,
            app_settings::ThemeView::System => ThemeDto::System,
        }
    }
}

impl IntoDomain<app_settings::QuickPanelPositionView> for QuickPanelPositionDto {
    fn into_domain(self) -> app_settings::QuickPanelPositionView {
        match self {
            QuickPanelPositionDto::Center => app_settings::QuickPanelPositionView::Center,
            QuickPanelPositionDto::FollowCursor => {
                app_settings::QuickPanelPositionView::FollowCursor
            }
        }
    }
}

impl IntoApiDto<QuickPanelPositionDto> for app_settings::QuickPanelPositionView {
    fn into_api_dto(self) -> QuickPanelPositionDto {
        match self {
            app_settings::QuickPanelPositionView::Center => QuickPanelPositionDto::Center,
            app_settings::QuickPanelPositionView::FollowCursor => {
                QuickPanelPositionDto::FollowCursor
            }
        }
    }
}

impl IntoDomain<app_settings::UpdateChannelView> for UpdateChannelDto {
    fn into_domain(self) -> app_settings::UpdateChannelView {
        match self {
            UpdateChannelDto::Stable => app_settings::UpdateChannelView::Stable,
            UpdateChannelDto::Alpha => app_settings::UpdateChannelView::Alpha,
            UpdateChannelDto::Beta => app_settings::UpdateChannelView::Beta,
            UpdateChannelDto::Rc => app_settings::UpdateChannelView::Rc,
        }
    }
}

impl IntoApiDto<UpdateChannelDto> for app_settings::UpdateChannelView {
    fn into_api_dto(self) -> UpdateChannelDto {
        match self {
            app_settings::UpdateChannelView::Stable => UpdateChannelDto::Stable,
            app_settings::UpdateChannelView::Alpha => UpdateChannelDto::Alpha,
            app_settings::UpdateChannelView::Beta => UpdateChannelDto::Beta,
            app_settings::UpdateChannelView::Rc => UpdateChannelDto::Rc,
        }
    }
}

impl IntoDomain<app_settings::SyncFrequencyView> for SyncFrequencyDto {
    fn into_domain(self) -> app_settings::SyncFrequencyView {
        match self {
            SyncFrequencyDto::Realtime => app_settings::SyncFrequencyView::Realtime,
            SyncFrequencyDto::Interval => app_settings::SyncFrequencyView::Interval,
        }
    }
}

impl IntoApiDto<SyncFrequencyDto> for app_settings::SyncFrequencyView {
    fn into_api_dto(self) -> SyncFrequencyDto {
        match self {
            app_settings::SyncFrequencyView::Realtime => SyncFrequencyDto::Realtime,
            app_settings::SyncFrequencyView::Interval => SyncFrequencyDto::Interval,
        }
    }
}

impl IntoDomain<app_settings::RuleEvaluationView> for RuleEvaluationDto {
    fn into_domain(self) -> app_settings::RuleEvaluationView {
        match self {
            RuleEvaluationDto::AnyMatch => app_settings::RuleEvaluationView::AnyMatch,
            RuleEvaluationDto::AllMatch => app_settings::RuleEvaluationView::AllMatch,
        }
    }
}

impl IntoApiDto<RuleEvaluationDto> for app_settings::RuleEvaluationView {
    fn into_api_dto(self) -> RuleEvaluationDto {
        match self {
            app_settings::RuleEvaluationView::AnyMatch => RuleEvaluationDto::AnyMatch,
            app_settings::RuleEvaluationView::AllMatch => RuleEvaluationDto::AllMatch,
        }
    }
}

impl IntoDomain<app_settings::ShortcutKeyView> for ShortcutKeyDto {
    fn into_domain(self) -> app_settings::ShortcutKeyView {
        match self {
            ShortcutKeyDto::Single(v) => app_settings::ShortcutKeyView::Single(v),
            ShortcutKeyDto::Multiple(v) => app_settings::ShortcutKeyView::Multiple(v),
        }
    }
}

impl IntoApiDto<ShortcutKeyDto> for app_settings::ShortcutKeyView {
    fn into_api_dto(self) -> ShortcutKeyDto {
        match self {
            app_settings::ShortcutKeyView::Single(v) => ShortcutKeyDto::Single(v),
            app_settings::ShortcutKeyView::Multiple(v) => ShortcutKeyDto::Multiple(v),
        }
    }
}
