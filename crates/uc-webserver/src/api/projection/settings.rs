//! Settings boundary projections: engine summaries ↔ settings DTOs.

use std::time::Duration;

mod app_settings {
    pub use uc_engine::{
        CongestionControllerSummary as CongestionControllerView, FileSyncSettingsPatch,
        GeneralSettingsPatch, NetworkSettingsPatch, PairingSettingsPatch, RetentionPolicyPatch,
        RetentionRulePatch as RetentionRulePatchValue, RetentionRuleSummary as RetentionRuleView,
        RuleEvaluationSummary as RuleEvaluationView, SecuritySettingsPatch,
        SettingsContentTypes as ContentTypesView, SettingsContentTypesPatch as ContentTypesPatch,
        SettingsPatch, SettingsSummary as SettingsView, SyncFrequencySummary as SyncFrequencyView,
        SyncSettingsPatch,
    };
}

use super::{IntoApiDto, IntoDomain};
use crate::api::dto::settings::{
    CongestionControllerDto, ContentTypesDto, ContentTypesPatchDto, FileSyncSettingsDto,
    GeneralSettingsDto, NetworkSettingsDto, PairingSettingsDto, PeerAddressDto,
    QuickPanelDoubleTapModifierDto, QuickPanelPositionDto, QuickPanelSettingsDto,
    RetentionPolicyDto, RetentionRuleDto, RuleEvaluationDto, SecuritySettingsDto, SettingsDto,
    SettingsPatchDto, ShortcutKeyDto, StartupModeDto, SyncFrequencyDto, SyncSettingsDto, ThemeDto,
    TrayIconDto, UpdateChannelDto, WindowSizeDto,
};
use uc_desktop_settings::{
    DesktopSettings, DesktopSettingsPatch, DoubleTapModifier,
    GeneralSettingsPatch as DesktopGeneralSettingsPatch, QuickPanelPosition,
    QuickPanelSettingsPatch as DesktopQuickPanelSettingsPatch, ShortcutBinding, StartupMode, Theme,
    TrayIcon, UpdateChannel, WindowSize,
};

impl IntoDomain<uc_engine::PeerAddressSetting> for PeerAddressDto {
    fn into_domain(self) -> uc_engine::PeerAddressSetting {
        uc_engine::PeerAddressSetting {
            ip: self.ip,
            port: self.port,
        }
    }
}

impl IntoApiDto<PeerAddressDto> for uc_engine::PeerAddressSetting {
    fn into_api_dto(self) -> PeerAddressDto {
        PeerAddressDto {
            ip: self.ip,
            port: self.port,
        }
    }
}

impl IntoDomain<app_settings::SettingsPatch> for SettingsPatchDto {
    fn into_domain(self) -> app_settings::SettingsPatch {
        app_settings::SettingsPatch {
            // Only Engine-owned general fields. Product fields go to the
            // product store (ADR-013 section 8.3) and `telemetryEnabled` to its
            // own file; the Engine never receives them, so there is no dual write.
            general: self
                .general
                .map(|general| app_settings::GeneralSettingsPatch {
                    device_name: general.device_name,
                    ..Default::default()
                })
                .filter(|general| *general != app_settings::GeneralSettingsPatch::default()),
            sync: self.sync.map(|sync| app_settings::SyncSettingsPatch {
                sync_enabled: sync.sync_enabled,
                auto_sync_enabled: sync.auto_sync_enabled,
                sync_frequency: sync.sync_frequency.map(IntoDomain::into_domain),
                content_types: sync.content_types.map(IntoDomain::into_domain),
                sync_on_restore: sync.sync_on_restore,
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
                    step_timeout_secs: pairing.step_timeout.map(|value| value.as_secs()),
                    user_verification_timeout_secs: pairing
                        .user_verification_timeout
                        .map(|value| value.as_secs()),
                    session_timeout_secs: pairing.session_timeout.map(|value| value.as_secs()),
                    max_retries: pairing.max_retries,
                }),
            file_sync: self
                .file_sync
                .map(|file_sync| app_settings::FileSyncSettingsPatch {
                    file_sync_enabled: file_sync.file_sync_enabled,
                    small_file_threshold: file_sync.small_file_threshold,
                    max_file_size: file_sync.max_file_size,
                    file_cache_quota_per_device: file_sync.file_cache_quota_per_device,
                    file_retention_hours: file_sync.file_retention_hours,
                    file_auto_cleanup: file_sync.file_auto_cleanup,
                    auto_save_dir: file_sync.auto_save_dir,
                    // File-set cap overrides are diagnostic-only for now and
                    // are not part of the public daemon settings contract.
                    max_file_set_total_bytes: None,
                    max_file_set_member_count: None,
                }),
            network: self
                .network
                .map(|network| app_settings::NetworkSettingsPatch {
                    bind_port: network.bind_port,
                    peer_addresses: network.peer_addresses.map(|changes| {
                        changes
                            .into_iter()
                            .map(|(device, address)| (device, address.map(IntoDomain::into_domain)))
                            .collect()
                    }),
                    allow_relay_fallback: network.allow_relay_fallback,
                    allow_overlay_network_addrs: network.allow_overlay_network_addrs,
                    custom_relay_urls: network.custom_relay_urls,
                    congestion_controller: network.congestion_controller.map(|controller| {
                        match controller {
                            CongestionControllerDto::Cubic => {
                                app_settings::CongestionControllerView::Cubic
                            }
                            CongestionControllerDto::Bbr3 => {
                                app_settings::CongestionControllerView::Bbr3
                            }
                        }
                    }),
                }),
        }
    }
}

/// The aggregate `/settings` view: Engine settings composed with the Desktop
/// product settings (ADR-013). Paths on the wire are unchanged.
impl IntoApiDto<SettingsDto> for (app_settings::SettingsView, &DesktopSettings) {
    fn into_api_dto(self) -> SettingsDto {
        let (view, desktop) = self;
        SettingsDto {
            schema_version: view.schema_version,
            general: GeneralSettingsDto {
                auto_start: desktop.general.auto_start,
                startup_mode: desktop.general.startup_mode.into_api_dto(),
                background_startup: desktop.general.background_startup,
                restore_last_entry_on_startup: desktop.general.restore_last_entry_on_startup,
                auto_check_update: desktop.general.auto_check_update,
                auto_download_update: desktop.general.auto_download_update,
                theme: desktop.general.theme.into_api_dto(),
                // Folded into the split presets by the import (ADR-013 section 8.3).
                theme_color: None,
                theme_color_light: desktop.general.theme_color_light.clone(),
                theme_color_dark: desktop.general.theme_color_dark.clone(),
                theme_overrides_light: desktop.general.theme_overrides_light.clone(),
                theme_overrides_dark: desktop.general.theme_overrides_dark.clone(),
                language: desktop.general.language.clone(),
                device_name: view.general.device_name,
                update_channel: desktop.general.update_channel.map(IntoApiDto::into_api_dto),
                telemetry_enabled: uc_observability::is_telemetry_enabled(),
                usage_analytics_enabled: desktop.general.usage_analytics_enabled,
                debug_mode: desktop.general.debug_mode,
                tray_icon: desktop.general.tray_icon.into_api_dto(),
                main_window_size: desktop.general.main_window_size.into_api_dto(),
                main_window_maximized: desktop.general.main_window_maximized,
            },
            sync: SyncSettingsDto {
                sync_enabled: view.sync.sync_enabled,
                auto_sync_enabled: view.sync.auto_sync_enabled,
                sync_frequency: view.sync.sync_frequency.into_api_dto(),
                content_types: view.sync.content_types.into_api_dto(),
                sync_on_restore: view.sync.sync_on_restore,
            },
            retention_policy: RetentionPolicyDto {
                enabled: view.retention_policy.enabled,
                rules: view
                    .retention_policy
                    .rules
                    .into_iter()
                    .map(IntoApiDto::into_api_dto)
                    .collect(),
                skip_pinned: view.retention_policy.skip_pinned,
                evaluation: view.retention_policy.evaluation.into_api_dto(),
            },
            security: SecuritySettingsDto {
                encryption_enabled: view.security.encryption_enabled,
                passphrase_configured: view.security.passphrase_configured,
                auto_unlock_enabled: view.security.auto_unlock_enabled,
            },
            pairing: PairingSettingsDto {
                step_timeout: Duration::from_secs(view.pairing.step_timeout_secs),
                user_verification_timeout: Duration::from_secs(
                    view.pairing.user_verification_timeout_secs,
                ),
                session_timeout: Duration::from_secs(view.pairing.session_timeout_secs),
                max_retries: view.pairing.max_retries,
                protocol_version: view.pairing.protocol_version,
            },
            keyboard_shortcuts: desktop
                .keyboard_shortcuts
                .iter()
                .map(|(id, binding)| (id.clone(), binding.clone().into_api_dto()))
                .collect(),
            file_sync: FileSyncSettingsDto {
                file_sync_enabled: view.file_sync.file_sync_enabled,
                small_file_threshold: view.file_sync.small_file_threshold,
                max_file_size: view.file_sync.max_file_size,
                file_cache_quota_per_device: view.file_sync.file_cache_quota_per_device,
                file_retention_hours: view.file_sync.file_retention_hours,
                file_auto_cleanup: view.file_sync.file_auto_cleanup,
                auto_save_dir: view.file_sync.auto_save_dir.clone(),
            },
            network: NetworkSettingsDto {
                bind_port: view.network.bind_port,
                peer_addresses: view
                    .network
                    .peer_addresses
                    .into_iter()
                    .map(|(device, address)| (device, address.into_api_dto()))
                    .collect(),
                allow_relay_fallback: view.network.allow_relay_fallback,
                allow_overlay_network_addrs: view.network.allow_overlay_network_addrs,
                custom_relay_urls: view.network.custom_relay_urls,
                congestion_controller: match view.network.congestion_controller {
                    app_settings::CongestionControllerView::Cubic => CongestionControllerDto::Cubic,
                    app_settings::CongestionControllerView::Bbr3 => CongestionControllerDto::Bbr3,
                },
            },
            quick_panel: QuickPanelSettingsDto {
                enabled: desktop.quick_panel.enabled,
                position: desktop.quick_panel.position.into_api_dto(),
                double_tap_modifier: desktop.quick_panel.double_tap_modifier.into_api_dto(),
            },
        }
    }
}

impl IntoApiDto<TrayIconDto> for TrayIcon {
    fn into_api_dto(self) -> TrayIconDto {
        match self {
            Self::Glyph => TrayIconDto::Glyph,
            Self::Cat => TrayIconDto::Cat,
        }
    }
}

impl IntoDomain<TrayIcon> for TrayIconDto {
    fn into_domain(self) -> TrayIcon {
        match self {
            Self::Glyph => TrayIcon::Glyph,
            Self::Cat => TrayIcon::Cat,
        }
    }
}

impl IntoDomain<DoubleTapModifier> for QuickPanelDoubleTapModifierDto {
    fn into_domain(self) -> DoubleTapModifier {
        match self {
            Self::Disabled => DoubleTapModifier::Disabled,
            Self::Alt => DoubleTapModifier::Alt,
            Self::Control => DoubleTapModifier::Control,
            Self::Meta => DoubleTapModifier::Meta,
        }
    }
}

impl IntoApiDto<QuickPanelDoubleTapModifierDto> for DoubleTapModifier {
    fn into_api_dto(self) -> QuickPanelDoubleTapModifierDto {
        match self {
            Self::Disabled => QuickPanelDoubleTapModifierDto::Disabled,
            Self::Alt => QuickPanelDoubleTapModifierDto::Alt,
            Self::Control => QuickPanelDoubleTapModifierDto::Control,
            Self::Meta => QuickPanelDoubleTapModifierDto::Meta,
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
            RetentionRuleDto::ByAge { max_age } => app_settings::RetentionRulePatchValue::ByAge {
                max_age_secs: max_age.as_secs(),
            },
            RetentionRuleDto::ByCount { max_items } => {
                app_settings::RetentionRulePatchValue::ByCount {
                    max_items: max_items as u64,
                }
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
                max_age_secs: max_age.as_secs(),
            },
            RetentionRuleDto::ByTotalSize { max_bytes } => {
                app_settings::RetentionRulePatchValue::ByTotalSize { max_bytes }
            }
            RetentionRuleDto::Sensitive { max_age } => {
                app_settings::RetentionRulePatchValue::Sensitive {
                    max_age_secs: max_age.as_secs(),
                }
            }
        }
    }
}

impl IntoApiDto<RetentionRuleDto> for app_settings::RetentionRuleView {
    fn into_api_dto(self) -> RetentionRuleDto {
        match self {
            app_settings::RetentionRuleView::ByAge { max_age_secs } => RetentionRuleDto::ByAge {
                max_age: Duration::from_secs(max_age_secs),
            },
            app_settings::RetentionRuleView::ByCount { max_items } => RetentionRuleDto::ByCount {
                max_items: usize::try_from(max_items).unwrap_or(usize::MAX),
            },
            app_settings::RetentionRuleView::ByContentType {
                content_type,
                max_age_secs,
            } => RetentionRuleDto::ByContentType {
                content_type: content_type.into_api_dto(),
                max_age: Duration::from_secs(max_age_secs),
            },
            app_settings::RetentionRuleView::ByTotalSize { max_bytes } => {
                RetentionRuleDto::ByTotalSize { max_bytes }
            }
            app_settings::RetentionRuleView::Sensitive { max_age_secs } => {
                RetentionRuleDto::Sensitive {
                    max_age: Duration::from_secs(max_age_secs),
                }
            }
        }
    }
}

impl IntoDomain<Theme> for ThemeDto {
    fn into_domain(self) -> Theme {
        match self {
            ThemeDto::Light => Theme::Light,
            ThemeDto::Dark => Theme::Dark,
            ThemeDto::System => Theme::System,
        }
    }
}

impl IntoApiDto<ThemeDto> for Theme {
    fn into_api_dto(self) -> ThemeDto {
        match self {
            Theme::Light => ThemeDto::Light,
            Theme::Dark => ThemeDto::Dark,
            Theme::System => ThemeDto::System,
        }
    }
}

impl IntoDomain<StartupMode> for StartupModeDto {
    fn into_domain(self) -> StartupMode {
        match self {
            StartupModeDto::Normal => StartupMode::Normal,
            StartupModeDto::Silent => StartupMode::Silent,
            StartupModeDto::Lightweight => StartupMode::Lightweight,
        }
    }
}

impl IntoApiDto<StartupModeDto> for StartupMode {
    fn into_api_dto(self) -> StartupModeDto {
        match self {
            StartupMode::Normal => StartupModeDto::Normal,
            StartupMode::Silent => StartupModeDto::Silent,
            StartupMode::Lightweight => StartupModeDto::Lightweight,
        }
    }
}

impl IntoDomain<QuickPanelPosition> for QuickPanelPositionDto {
    fn into_domain(self) -> QuickPanelPosition {
        match self {
            QuickPanelPositionDto::Center => QuickPanelPosition::Center,
            QuickPanelPositionDto::FollowCursor => QuickPanelPosition::FollowCursor,
        }
    }
}

impl IntoApiDto<QuickPanelPositionDto> for QuickPanelPosition {
    fn into_api_dto(self) -> QuickPanelPositionDto {
        match self {
            QuickPanelPosition::Center => QuickPanelPositionDto::Center,
            QuickPanelPosition::FollowCursor => QuickPanelPositionDto::FollowCursor,
        }
    }
}

impl IntoDomain<UpdateChannel> for UpdateChannelDto {
    fn into_domain(self) -> UpdateChannel {
        match self {
            UpdateChannelDto::Stable => UpdateChannel::Stable,
            UpdateChannelDto::Alpha => UpdateChannel::Alpha,
            UpdateChannelDto::Beta => UpdateChannel::Beta,
            UpdateChannelDto::Rc => UpdateChannel::Rc,
        }
    }
}

impl IntoApiDto<UpdateChannelDto> for UpdateChannel {
    fn into_api_dto(self) -> UpdateChannelDto {
        match self {
            UpdateChannel::Stable => UpdateChannelDto::Stable,
            UpdateChannel::Alpha => UpdateChannelDto::Alpha,
            UpdateChannel::Beta => UpdateChannelDto::Beta,
            UpdateChannel::Rc => UpdateChannelDto::Rc,
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

impl IntoDomain<ShortcutBinding> for ShortcutKeyDto {
    fn into_domain(self) -> ShortcutBinding {
        match self {
            ShortcutKeyDto::Single(v) => ShortcutBinding::Single(v),
            ShortcutKeyDto::Multiple(v) => ShortcutBinding::Multiple(v),
        }
    }
}

impl IntoApiDto<ShortcutKeyDto> for ShortcutBinding {
    fn into_api_dto(self) -> ShortcutKeyDto {
        match self {
            ShortcutBinding::Single(v) => ShortcutKeyDto::Single(v),
            ShortcutBinding::Multiple(v) => ShortcutKeyDto::Multiple(v),
        }
    }
}

impl IntoApiDto<WindowSizeDto> for WindowSize {
    fn into_api_dto(self) -> WindowSizeDto {
        WindowSizeDto {
            width: self.width,
            height: self.height,
        }
    }
}

impl IntoDomain<WindowSize> for WindowSizeDto {
    fn into_domain(self) -> WindowSize {
        WindowSize {
            width: self.width,
            height: self.height,
        }
    }
}

/// The product part of a `PUT /settings` patch (ADR-013 section 8.3). Values
/// are validated by the store's grammars before any write.
pub fn desktop_patch(patch: &SettingsPatchDto) -> DesktopSettingsPatch {
    let general = patch.general.as_ref();
    DesktopSettingsPatch {
        general: general.map_or_else(DesktopGeneralSettingsPatch::default, |general| {
            DesktopGeneralSettingsPatch {
                tray_icon: general.tray_icon.map(IntoDomain::into_domain),
                main_window_size: general.main_window_size.map(IntoDomain::into_domain),
                main_window_maximized: general.main_window_maximized,
                auto_start: general.auto_start,
                startup_mode: general.startup_mode.clone().map(IntoDomain::into_domain),
                background_startup: general.background_startup,
                restore_last_entry_on_startup: general.restore_last_entry_on_startup,
                auto_check_update: general.auto_check_update,
                auto_download_update: general.auto_download_update,
                update_channel: general
                    .update_channel
                    .clone()
                    .map(|channel| channel.map(IntoDomain::into_domain)),
                theme: general.theme.clone().map(IntoDomain::into_domain),
                theme_color: general.theme_color.clone().flatten(),
                theme_color_light: general.theme_color_light.clone(),
                theme_color_dark: general.theme_color_dark.clone(),
                theme_overrides_light: general.theme_overrides_light.clone(),
                theme_overrides_dark: general.theme_overrides_dark.clone(),
                language: general.language.clone(),
                usage_analytics_enabled: general.usage_analytics_enabled,
                debug_mode: general.debug_mode,
            }
        }),
        keyboard_shortcuts: patch
            .keyboard_shortcuts
            .as_ref()
            .map(|shortcuts| {
                shortcuts
                    .shortcuts
                    .iter()
                    .map(|(id, value)| (id.clone(), value.clone().map(IntoDomain::into_domain)))
                    .collect()
            })
            .unwrap_or_default(),
        quick_panel: patch.quick_panel.as_ref().map_or_else(
            DesktopQuickPanelSettingsPatch::default,
            |quick_panel| DesktopQuickPanelSettingsPatch {
                enabled: quick_panel.enabled,
                position: quick_panel.position.map(IntoDomain::into_domain),
                double_tap_modifier: quick_panel.double_tap_modifier.map(IntoDomain::into_domain),
            },
        ),
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;

    #[test]
    fn peer_address_patch_projects_only_supplied_devices_and_null_removals() {
        let dto: SettingsPatchDto = serde_json::from_value(serde_json::json!({
            "network": {"peerAddresses": {
                "device-a": {"ip": "10.10.0.2", "port": 51820}, "device-b": null
            }}
        }))
        .unwrap();
        let patch: app_settings::SettingsPatch = dto.into_domain();
        let changes = patch.network.unwrap().peer_addresses.unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes["device-a"].as_ref().unwrap().ip, "10.10.0.2");
        assert_eq!(changes["device-a"].as_ref().unwrap().port, 51820);
        assert_eq!(changes["device-b"], None);
        let absent: SettingsPatchDto =
            serde_json::from_value(serde_json::json!({"network": {}})).unwrap();
        let patch: app_settings::SettingsPatch = absent.into_domain();
        assert!(patch.network.unwrap().peer_addresses.is_none());
    }

    #[test]
    fn peer_address_summary_round_trips_through_wire() {
        let mut summary = app_settings::SettingsView::default();
        summary.network.peer_addresses.insert(
            "device-a".into(),
            uc_engine::PeerAddressSetting {
                ip: "fd00::2".into(),
                port: 51820,
            },
        );
        let dto: SettingsDto = (summary, &DesktopSettings::default()).into_api_dto();
        let wire = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            wire["network"]["peerAddresses"]["device-a"],
            serde_json::json!({"ip": "fd00::2", "port": 51820})
        );
        assert!(wire["network"].get("peer_addresses").is_none());
        let decoded: SettingsDto = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded.network.peer_addresses, dto.network.peer_addresses);
    }

    #[test]
    fn bind_port_patch_preserves_absent_random_and_fixed_values() {
        for (wire, expected) in [
            (serde_json::json!({}), None),
            (serde_json::json!({"bindPort": 0}), Some(0)),
            (serde_json::json!({"bindPort": 51820}), Some(51820)),
            (serde_json::json!({"bindPort": 65535}), Some(65535)),
        ] {
            let dto: SettingsPatchDto =
                serde_json::from_value(serde_json::json!({"network": wire})).unwrap();
            let patch: app_settings::SettingsPatch = dto.into_domain();
            assert_eq!(patch.network.unwrap().bind_port, expected);
        }
    }

    #[test]
    fn bind_port_summary_round_trips_through_camel_case_wire() {
        for port in [None, Some(1), Some(51820), Some(65535)] {
            let mut summary = app_settings::SettingsView::default();
            summary.network.bind_port = port;
            let dto: SettingsDto = (summary, &DesktopSettings::default()).into_api_dto();
            let wire = serde_json::to_value(dto).unwrap();
            assert_eq!(wire["network"]["bindPort"], serde_json::json!(port));
            assert!(wire["network"].get("bind_port").is_none());
            let decoded: SettingsDto = serde_json::from_value(wire).unwrap();
            assert_eq!(decoded.network.bind_port, port);
        }
    }
}

#[cfg(test)]
mod window_size_tests {
    use super::*;

    #[test]
    fn window_size_patch_is_owned_only_by_desktop() {
        let patch: SettingsPatchDto = serde_json::from_value(serde_json::json!({
            "general": {"mainWindowSize": {"width": 1234, "height": 810}}
        }))
        .unwrap();
        let wire = serde_json::to_value(&patch).unwrap();
        assert_eq!(
            wire["general"]["mainWindowSize"],
            serde_json::json!({"width":1234,"height":810})
        );
        assert!(wire["general"].get("main_window_size").is_none());
        assert_eq!(
            WindowSizeDto::default().into_domain(),
            WindowSize::default()
        );
        let desktop = desktop_patch(&patch);
        assert_eq!(
            desktop.general.main_window_size,
            Some(WindowSize {
                width: 1234,
                height: 810
            })
        );
        let engine: app_settings::SettingsPatch = patch.into_domain();
        assert!(engine == app_settings::SettingsPatch::default());
        let wire = serde_json::to_value(
            WindowSize {
                width: 1234,
                height: 810,
            }
            .into_api_dto(),
        )
        .unwrap();
        assert_eq!(wire, serde_json::json!({"width":1234,"height":810}));
    }
}

#[cfg(test)]
mod desktop_mode_tests {
    use super::*;

    #[test]
    fn runtime_preferences_round_trip_without_changing_engine_settings() {
        let patch: SettingsPatchDto = serde_json::from_value(serde_json::json!({
            "general": {"startupMode": "lightweight", "backgroundStartup": false}
        }))
        .unwrap();
        let desktop = desktop_patch(&patch);
        assert_eq!(
            desktop.general.startup_mode,
            Some(uc_desktop_settings::StartupMode::Lightweight)
        );
        assert_eq!(desktop.general.background_startup, Some(false));
        assert!(
            patch.network.is_none(),
            "desktop modes must not request a daemon restart"
        );
        let engine: app_settings::SettingsPatch = patch.into_domain();
        assert!(engine == app_settings::SettingsPatch::default());

        let mut desktop = DesktopSettings::default();
        desktop.general.startup_mode = uc_desktop_settings::StartupMode::Lightweight;
        desktop.general.background_startup = true;
        let wire: SettingsDto = (app_settings::SettingsView::default(), &desktop).into_api_dto();
        let general = serde_json::to_value(wire.general).unwrap();
        assert_eq!(general["startupMode"], "lightweight");
        assert_eq!(general["backgroundStartup"], true);
    }
}
