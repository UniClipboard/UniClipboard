use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use super::model::*;

impl Default for UpdateChannel {
    /// Returns the default `UpdateChannel`, which is `Stable`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::UpdateChannel;
    ///
    /// let channel = UpdateChannel::default();
    /// assert_eq!(channel, UpdateChannel::Stable);
    /// ```
    fn default() -> Self {
        UpdateChannel::Stable
    }
}

impl Default for GeneralSettings {
    /// Returns the default `GeneralSettings` used when no user preferences are configured.
    ///
    /// The defaults are:
    /// - `auto_start`: false
    /// - `silent_start`: false
    /// - `auto_check_update`: true
    /// - `auto_download_update`: false (opt-in — pre-fetch consumes bandwidth)
    /// - `theme`: `Theme::System`
    /// - `theme_color`: `None`
    /// - `device_name`: `None`
    /// - `language`: `None`
    /// - `update_channel`: `None` (auto-detect from version)
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{GeneralSettings, Theme};
    ///
    /// let settings = GeneralSettings::default();
    /// assert_eq!(settings.auto_start, false);
    /// assert_eq!(settings.silent_start, false);
    /// assert_eq!(settings.auto_check_update, true);
    /// assert_eq!(settings.auto_download_update, false);
    /// assert_eq!(settings.theme, Theme::System);
    /// assert!(settings.theme_color.is_none());
    /// assert!(settings.device_name.is_none());
    /// assert!(settings.language.is_none());
    /// assert!(settings.update_channel.is_none());
    /// ```
    fn default() -> Self {
        Self {
            auto_start: false,
            silent_start: false,
            auto_check_update: true,
            auto_download_update: false,
            theme: Theme::System,
            theme_color: None,
            theme_color_light: None,
            theme_color_dark: None,
            theme_overrides_light: BTreeMap::new(),
            theme_overrides_dark: BTreeMap::new(),
            device_name: None,
            language: None,
            update_channel: None,
            telemetry_enabled: true,
            usage_analytics_enabled: true,
        }
    }
}

impl Default for ContentTypes {
    /// Returns default `ContentTypes` with all fields set to `true`.
    ///
    /// New devices sync everything by default. Users can then disable
    /// specific content types per device.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::ContentTypes;
    ///
    /// let ct = ContentTypes::default();
    /// assert!(ct.text);
    /// assert!(ct.image);
    /// assert!(ct.link);
    /// assert!(ct.file);
    /// assert!(ct.code_snippet);
    /// assert!(ct.rich_text);
    /// ```
    fn default() -> Self {
        Self {
            text: true,
            image: true,
            link: true,
            file: true,
            code_snippet: true,
            rich_text: true,
        }
    }
}

impl Default for SyncSettings {
    /// Creates a `SyncSettings` populated with sensible defaults.
    ///
    /// The defaults enable automatic syncing, use realtime sync frequency, and include the
    /// default content types.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{SyncSettings, SyncFrequency};
    ///
    /// let s = SyncSettings::default();
    /// assert!(s.auto_sync);
    /// assert_eq!(s.sync_frequency, SyncFrequency::Realtime);
    /// ```
    fn default() -> Self {
        Self {
            auto_sync: true,
            sync_frequency: SyncFrequency::Realtime,
            content_types: ContentTypes::default(),
        }
    }
}

impl Default for RetentionPolicy {
    /// Creates a `RetentionPolicy` populated with sensible defaults.
    ///
    /// The default policy is enabled, skips pinned items, evaluates rules using `AnyMatch`,
    /// and includes two rules: keep items younger than 30 days and keep up to 500 most recent items.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use uc_core::settings::model::{RetentionPolicy, RuleEvaluation, RetentionRule};
    ///
    /// let p = RetentionPolicy::default();
    /// assert!(p.enabled);
    /// assert!(p.skip_pinned);
    /// assert_eq!(p.evaluation, RuleEvaluation::AnyMatch);
    /// assert!(matches!(p.rules.get(0), Some(RetentionRule::ByAge { .. })));
    /// assert!(matches!(p.rules.get(1), Some(RetentionRule::ByCount { .. })));
    /// if let Some(RetentionRule::ByAge { max_age }) = p.rules.get(0) {
    ///     assert_eq!(*max_age, Duration::from_secs(60 * 60 * 24 * 30));
    /// }
    /// if let Some(RetentionRule::ByCount { max_items }) = p.rules.get(1) {
    ///     assert_eq!(*max_items, 500);
    /// }
    /// ```
    fn default() -> Self {
        Self {
            enabled: true,
            skip_pinned: true,
            evaluation: RuleEvaluation::AnyMatch,
            rules: vec![
                RetentionRule::ByAge {
                    max_age: Duration::from_secs(60 * 60 * 24 * 30), // 30 days
                },
                RetentionRule::ByCount { max_items: 500 },
            ],
        }
    }
}

impl Default for SecuritySettings {
    /// Creates default security settings with encryption disabled and no passphrase configured.
    ///
    /// The default has `encryption_enabled = false`, `passphrase_configured = false`,
    /// and `auto_unlock_enabled = false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::SecuritySettings;
    ///
    /// let s = SecuritySettings::default();
    /// assert!(!s.encryption_enabled);
    /// assert!(!s.passphrase_configured);
    /// assert!(!s.auto_unlock_enabled);
    /// ```
    fn default() -> Self {
        Self {
            encryption_enabled: false,
            passphrase_configured: false,
            auto_unlock_enabled: false,
        }
    }
}

impl Default for PairingSettings {
    /// Creates default pairing settings for handshake timers and retry behavior.
    ///
    /// Defaults are:
    /// - `step_timeout`: 30 seconds
    /// - `user_verification_timeout`: 120 seconds
    /// - `session_timeout`: 300 seconds
    /// - `max_retries`: 3
    /// - `protocol_version`: "1.0.0"
    fn default() -> Self {
        Self {
            step_timeout: Duration::from_secs(30),
            user_verification_timeout: Duration::from_secs(120),
            session_timeout: Duration::from_secs(300),
            max_retries: 3,
            protocol_version: "1.0.0".to_string(),
        }
    }
}

impl Default for FileSyncSettings {
    /// Returns default `FileSyncSettings` enabling file sync with sensible limits.
    ///
    /// Defaults:
    /// - `file_sync_enabled`: true
    /// - `small_file_threshold`: 10 MB (inline transfer threshold)
    /// - `max_file_size`: 5 GB
    /// - `file_cache_quota_per_device`: 500 MB
    /// - `file_retention_hours`: 24
    /// - `file_auto_cleanup`: true
    fn default() -> Self {
        Self {
            file_sync_enabled: true,
            small_file_threshold: 10 * 1024 * 1024, // 10 MB
            max_file_size: 5 * 1024 * 1024 * 1024,  // 5 GB
            file_cache_quota_per_device: 500 * 1024 * 1024, // 500 MB
            file_retention_hours: 24,
            file_auto_cleanup: true,
        }
    }
}

impl Default for NetworkSettings {
    /// Returns default `NetworkSettings` allowing iroh to fall back to public
    /// relays when direct connectivity fails (existing v0.6.x behavior).
    ///
    /// Defaults:
    /// - `allow_relay_fallback`: true
    /// - `allow_overlay_network_addrs`: false
    ///
    // 默认 true = 允许 fallback。
    // 改成 false 会让所有跨网段老用户突然离线，属于 breaking change。
    // 修改默认值前请先 grep `LAN-only Mode` 文档与 changelog。
    fn default() -> Self {
        Self {
            allow_relay_fallback: true,
            allow_overlay_network_addrs: false,
        }
    }
}

impl Default for MobileSyncSettings {
    /// 默认全部关闭 / 未选定。开启移动端同步暴露 LAN 监听端口,必须由用户
    /// 在设置页显式开启 + 重启 daemon。详见
    /// `.context/mobile-sync/SPEC.md` §5 + §14.10。
    fn default() -> Self {
        Self {
            enabled: false,
            lan_listen_enabled: false,
            lan_advertise_ip: None,
            lan_port: None,
        }
    }
}

impl Default for Settings {
    /// Constructs a Settings instance populated with the current schema version and sensible nested defaults.
    ///
    /// The created `Settings` uses `CURRENT_SCHEMA_VERSION` for `schema_version` and the `Default` implementations
    /// of the nested settings types for `general`, `sync`, `retention_policy`, `security`, and `pairing`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{Settings, CURRENT_SCHEMA_VERSION};
    ///
    /// let settings = Settings::default();
    /// assert_eq!(settings.schema_version, CURRENT_SCHEMA_VERSION);
    /// // Nested defaults are available:
    /// let _ = settings.general;
    /// let _ = settings.sync;
    /// let _ = settings.retention_policy;
    /// let _ = settings.security;
    /// let _ = settings.pairing;
    /// ```
    ///
    /// # Returns
    ///
    /// `Settings` initialized with `CURRENT_SCHEMA_VERSION` and default values for `general`, `sync`, `retention_policy`, `security`, and `pairing`.
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            general: GeneralSettings::default(),
            sync: SyncSettings::default(),
            retention_policy: RetentionPolicy::default(),
            security: SecuritySettings::default(),
            pairing: PairingSettings::default(),
            keyboard_shortcuts: HashMap::new(),
            file_sync: FileSyncSettings::default(),
            network: NetworkSettings::default(),
            mobile_sync: MobileSyncSettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::model::{
        ContentTypes, FileSyncSettings, NetworkSettings, Settings, SyncFrequency, Theme,
        CURRENT_SCHEMA_VERSION,
    };

    /// Pitfall 2 防御：默认值必须为 true（允许 fallback），保护老用户
    /// 跨网段同步行为。改 false = breaking change。
    #[test]
    fn network_settings_default_allows_relay_fallback() {
        let n = NetworkSettings::default();
        assert!(
            n.allow_relay_fallback,
            "NetworkSettings::default().allow_relay_fallback MUST be true (Pitfall 2)"
        );
    }

    /// Pitfall 2 防御：顶层 Settings::default 把 network 装配进去，
    /// 字段值与子结构 default 保持一致。
    #[test]
    fn settings_default_includes_network_with_fallback_allowed() {
        let s = Settings::default();
        assert!(s.network.allow_relay_fallback);
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            s.schema_version, 1,
            "schema_version MUST stay 1 (no migration)"
        );
    }

    /// NETSET-02 success criterion #2：缺 `network` 段的旧 settings.json
    /// 反序列化必须回填默认值 true。
    #[test]
    fn old_settings_json_without_network_section_falls_back_to_default() {
        // 模拟 v0.6.x 时代写出的 settings.json 片段（无 network 字段）
        let json = r#"{}"#;
        let s: Settings = serde_json::from_str(json).expect("parse minimal settings");
        assert!(
            s.network.allow_relay_fallback,
            "missing network section MUST default to true"
        );
        assert_eq!(s.schema_version, 1);
    }

    /// 显式 false 的 JSON 必须保留 false 语义（确认未误取反）。
    #[test]
    fn explicit_allow_relay_fallback_false_is_preserved() {
        let json = r#"{ "network": { "allow_relay_fallback": false } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse explicit false");
        assert!(!s.network.allow_relay_fallback);
    }

    /// 反向断言：显式 true 也保留 true 语义。
    #[test]
    fn explicit_allow_relay_fallback_true_is_preserved() {
        let json = r#"{ "network": { "allow_relay_fallback": true } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse explicit true");
        assert!(s.network.allow_relay_fallback);
    }

    /// 默认值：默认过滤 overlay 网络地址，保持 v0.6.x 起的现行行为。
    #[test]
    fn network_settings_default_filters_overlay_addrs() {
        let n = NetworkSettings::default();
        assert!(
            !n.allow_overlay_network_addrs,
            "NetworkSettings::default().allow_overlay_network_addrs MUST be false"
        );
    }

    /// 老 settings.json 缺 `allow_overlay_network_addrs` 字段时回填默认 false。
    #[test]
    fn old_settings_json_without_overlay_field_falls_back_to_default() {
        let json = r#"{ "network": { "allow_relay_fallback": true } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse old network section");
        assert!(
            !s.network.allow_overlay_network_addrs,
            "missing allow_overlay_network_addrs MUST default to false"
        );
    }

    /// 显式 true 必须保留（专业用户主动开启）。
    #[test]
    fn explicit_allow_overlay_network_addrs_true_is_preserved() {
        let json = r#"{ "network": { "allow_relay_fallback": true, "allow_overlay_network_addrs": true } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse explicit overlay true");
        assert!(s.network.allow_overlay_network_addrs);
    }

    /// 显式 false 必须保留（双向覆盖）。
    #[test]
    fn explicit_allow_overlay_network_addrs_false_is_preserved() {
        let json = r#"{ "network": { "allow_relay_fallback": true, "allow_overlay_network_addrs": false } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse explicit overlay false");
        assert!(!s.network.allow_overlay_network_addrs);
    }

    // ============================================================
    // Issue #581 回归测试：旧版本 settings.json 在升级后被新版本读取时,
    // 缺失字段必须自动回退到 `Default::default()` 的业务默认值,
    // 反序列化不得失败。
    //
    // 核心场景:`file_sync` 段在 fed7ada2 之前没有 `small_file_threshold`
    // 字段,daemon 启动时反序列化整个 Settings 直接报错。
    // ============================================================

    /// Issue #581 直接复现:`file_sync` 段缺 `small_file_threshold`,
    /// 必须能反序列化并回退到默认 10 MB。
    #[test]
    fn file_sync_missing_small_file_threshold_falls_back_to_default() {
        let json = r#"{
            "file_sync": {
                "file_sync_enabled": true,
                "max_file_size": 5368709120,
                "file_cache_quota_per_device": 524288000,
                "file_retention_hours": 24,
                "file_auto_cleanup": true
            }
        }"#;
        let s: Settings = serde_json::from_str(json).expect("must not error on missing field");
        assert_eq!(s.file_sync.small_file_threshold, 10 * 1024 * 1024);
        assert!(s.file_sync.file_sync_enabled);
        assert_eq!(s.file_sync.max_file_size, 5 * 1024 * 1024 * 1024);
    }

    /// `file_sync` 段为空对象时,所有字段都回退到默认值。
    #[test]
    fn file_sync_empty_object_falls_back_to_full_default() {
        let json = r#"{ "file_sync": {} }"#;
        let s: Settings = serde_json::from_str(json).expect("empty file_sync must parse");
        let expected = FileSyncSettings::default();
        assert_eq!(s.file_sync, expected);
    }

    /// `general` 段缺 `telemetry_enabled` 字段,必须回退默认 true。
    #[test]
    fn general_missing_telemetry_enabled_falls_back_to_default() {
        let json = r#"{
            "general": {
                "auto_start": false,
                "silent_start": false,
                "auto_check_update": true,
                "theme": "system",
                "theme_color": null,
                "language": null,
                "device_name": null
            }
        }"#;
        let s: Settings = serde_json::from_str(json).expect("missing telemetry must parse");
        assert!(s.general.telemetry_enabled);
    }

    /// `general` 段缺多个字段时仍能解析,缺失字段全部回退。
    #[test]
    fn general_partial_object_fills_missing_fields() {
        let json = r#"{ "general": { "auto_start": true } }"#;
        let s: Settings = serde_json::from_str(json).expect("partial general must parse");
        assert!(s.general.auto_start);
        // 其余字段全部走默认
        assert!(!s.general.silent_start);
        assert!(s.general.auto_check_update);
        assert_eq!(s.general.theme, Theme::System);
        assert!(s.general.telemetry_enabled);
    }

    /// `sync` 段缺 `content_types` 与 `auto_sync`,均回退默认。
    #[test]
    fn sync_missing_fields_fall_back_to_default() {
        let json = r#"{ "sync": { "sync_frequency": "interval" } }"#;
        let s: Settings = serde_json::from_str(json).expect("partial sync must parse");
        assert_eq!(s.sync.sync_frequency, SyncFrequency::Interval);
        assert!(s.sync.auto_sync);
        assert_eq!(s.sync.content_types, ContentTypes::default());
    }

    /// `security` 段缺所有字段时回退默认,且未来加新字段不会再 break 启动。
    #[test]
    fn security_empty_object_falls_back_to_default() {
        let json = r#"{ "security": {} }"#;
        let s: Settings = serde_json::from_str(json).expect("empty security must parse");
        assert!(!s.security.encryption_enabled);
        assert!(!s.security.passphrase_configured);
        assert!(!s.security.auto_unlock_enabled);
    }

    /// `pairing` 段缺字段时回退到 `PairingSettings::default()` —— `serde_with`
    /// 装饰器与 struct 级 `#[serde(default)]` 协同工作。
    #[test]
    fn pairing_partial_object_fills_missing_fields() {
        let json = r#"{ "pairing": { "max_retries": 7 } }"#;
        let s: Settings = serde_json::from_str(json).expect("partial pairing must parse");
        assert_eq!(s.pairing.max_retries, 7);
        assert_eq!(s.pairing.protocol_version, "1.0.0");
        assert_eq!(s.pairing.step_timeout, std::time::Duration::from_secs(30));
    }

    /// `mobile_sync` 段缺 `lan_advertise_ip` / `lan_port` 时回退 None。
    #[test]
    fn mobile_sync_missing_optional_fields_fall_back_to_none() {
        let json = r#"{ "mobile_sync": { "enabled": true } }"#;
        let s: Settings = serde_json::from_str(json).expect("partial mobile_sync must parse");
        assert!(s.mobile_sync.enabled);
        assert!(!s.mobile_sync.lan_listen_enabled);
        assert!(s.mobile_sync.lan_advertise_ip.is_none());
        assert!(s.mobile_sync.lan_port.is_none());
    }

    /// 综合回归:模拟 v0.2 时代的 settings.json(只有 general/sync,
    /// 完全没有 file_sync / network / mobile_sync 等后续新增段),
    /// 必须能直接反序列化为完整 Settings。
    #[test]
    fn legacy_v02_settings_json_loads_with_all_defaults() {
        let json = r#"{
            "schema_version": 1,
            "general": { "auto_start": true, "theme": "dark" },
            "sync": { "auto_sync": false, "sync_frequency": "interval" }
        }"#;
        let s: Settings = serde_json::from_str(json).expect("legacy settings must parse");

        assert_eq!(s.schema_version, 1);
        assert!(s.general.auto_start);
        assert_eq!(s.general.theme, Theme::Dark);
        assert!(s.general.telemetry_enabled);
        assert!(!s.sync.auto_sync);
        assert_eq!(s.sync.content_types, ContentTypes::default());

        // 后续新增段全部走 Default
        assert_eq!(s.file_sync, FileSyncSettings::default());
        assert!(s.network.allow_relay_fallback);
        assert!(!s.network.allow_overlay_network_addrs);
        assert!(!s.mobile_sync.enabled);
    }

    /// 显式字段值不被 `#[serde(default)]` 误覆盖。
    #[test]
    fn explicit_file_sync_values_are_preserved() {
        let json = r#"{
            "file_sync": {
                "file_sync_enabled": false,
                "small_file_threshold": 1024,
                "max_file_size": 2048,
                "file_cache_quota_per_device": 4096,
                "file_retention_hours": 1,
                "file_auto_cleanup": false
            }
        }"#;
        let s: Settings = serde_json::from_str(json).expect("explicit file_sync must parse");
        assert!(!s.file_sync.file_sync_enabled);
        assert_eq!(s.file_sync.small_file_threshold, 1024);
        assert_eq!(s.file_sync.max_file_size, 2048);
        assert_eq!(s.file_sync.file_cache_quota_per_device, 4096);
        assert_eq!(s.file_sync.file_retention_hours, 1);
        assert!(!s.file_sync.file_auto_cleanup);
    }

    /// theme_color 拆分后的回退语义：旧 JSON 只有 `theme_color: "catppuccin"`,
    /// 新代码 read 出 light/dark 都应回退到 catppuccin。
    #[test]
    fn legacy_theme_color_falls_back_to_both_modes() {
        let json = r#"{ "general": { "auto_start": false, "silent_start": false, "auto_check_update": true, "theme": "system", "theme_color": "catppuccin" } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse legacy theme_color");
        assert_eq!(s.general.theme_color.as_deref(), Some("catppuccin"));
        assert!(s.general.theme_color_light.is_none());
        assert!(s.general.theme_color_dark.is_none());
        assert_eq!(s.general.effective_theme_color_light(), Some("catppuccin"));
        assert_eq!(s.general.effective_theme_color_dark(), Some("catppuccin"));
    }

    /// 新 JSON 同时写入 light/dark 时取自身值,不再回退到 legacy。
    #[test]
    fn split_theme_color_overrides_legacy_field() {
        let json = r#"{
            "general": {
                "auto_start": false,
                "silent_start": false,
                "auto_check_update": true,
                "theme": "system",
                "theme_color": "catppuccin",
                "theme_color_light": "zinc",
                "theme_color_dark": "claude"
            }
        }"#;
        let s: Settings = serde_json::from_str(json).expect("parse split theme_color");
        assert_eq!(s.general.effective_theme_color_light(), Some("zinc"));
        assert_eq!(s.general.effective_theme_color_dark(), Some("claude"));
    }

    /// 完全没配置任何 theme_color 时,回退为 None,由 UI 端引擎兜底。
    #[test]
    fn missing_all_theme_color_fields_resolves_to_none() {
        let json = r#"{ "general": { "auto_start": false, "silent_start": false, "auto_check_update": true, "theme": "system" } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse no theme_color");
        assert!(s.general.effective_theme_color_light().is_none());
        assert!(s.general.effective_theme_color_dark().is_none());
    }

    /// 老 settings.json 缺 theme_overrides_* 字段时回填为空 map。
    #[test]
    fn legacy_settings_without_theme_overrides_fields_default_empty() {
        let json = r#"{ "general": { "auto_start": false, "silent_start": false, "auto_check_update": true, "theme": "system" } }"#;
        let s: Settings = serde_json::from_str(json).expect("parse legacy");
        assert!(s.general.theme_overrides_light.is_empty());
        assert!(s.general.theme_overrides_dark.is_empty());
    }

    /// theme_overrides_* 显式带值时序列化往返不丢字段。
    #[test]
    fn theme_overrides_round_trip() {
        let json = r#"{
            "general": {
                "auto_start": false,
                "silent_start": false,
                "auto_check_update": true,
                "theme": "system",
                "theme_overrides_light": { "primary": "oklch(0.5 0.2 270)" },
                "theme_overrides_dark": {
                    "primary": "oklch(0.6 0.15 30)",
                    "background": "oklch(0.18 0.02 280)"
                }
            }
        }"#;
        let s: Settings = serde_json::from_str(json).expect("parse overrides");
        assert_eq!(
            s.general
                .theme_overrides_light
                .get("primary")
                .map(String::as_str),
            Some("oklch(0.5 0.2 270)")
        );
        assert_eq!(s.general.theme_overrides_dark.len(), 2);
        assert_eq!(
            s.general
                .theme_overrides_dark
                .get("background")
                .map(String::as_str),
            Some("oklch(0.18 0.02 280)")
        );
    }
}
