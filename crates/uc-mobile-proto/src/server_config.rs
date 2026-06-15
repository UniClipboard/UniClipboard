//! Server profiles + persisted list (`server_config_list` blob) — spec §5.1/§5.2/§5.5.
//!
//! Port of the `Codable` shapes in uc-ios `Shared/Models/ServerConfig.swift`.
//! This module owns ONLY the persisted byte shape and the one-shot legacy
//! migrations; the §5.3 network-ordering math ([`crate::net_class`]) and SSID
//! normalization already live in `net_class`, so a profile's `urls` are stored
//! verbatim here and re-ordered at call time elsewhere.
//!
//! Byte-compat notes (BYTE-CRITICAL — these decode existing native blobs and
//! produce blobs an old native reader must still parse):
//! - [`ServerConfig`] encodes BOTH `url` (== `urls[0]`) and `urls`, mirroring
//!   the §4 wire payload, so a pre-multi-URL reader still works.
//! - `urls` wins on decode when present and non-empty; otherwise the legacy
//!   single `url` becomes a one-element list. Neither present → error.
//! - The dropped pre-unification keys (`autoSwitchWifiNames` /
//!   `autoSwitchStrategy` on a config, `manualOverrideConfigId` on the list)
//!   are decoded-and-ignored, never re-encoded.

use serde::ser::SerializeMap;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

/// One server profile (spec §5.1): one credential pair reachable at one or more
/// candidate base URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// Stable profile id (UUID from the pairing QR, or a migration-minted one).
    pub id: String,
    /// Optional display name; falls back to `urls[0]` in the UI.
    pub name: Option<String>,
    /// Ordered candidate base URLs. Never empty for a valid config.
    pub urls: Vec<String>,
    /// HTTP Basic Auth username.
    pub username: String,
    /// HTTP Basic Auth password.
    pub password: String,
}

impl ServerConfig {
    /// Back-compat accessor == `urls[0]` (empty string only when `urls` is
    /// somehow empty). Mirrors Swift `ServerConfig.url`.
    pub fn url(&self) -> &str {
        self.urls.first().map(String::as_str).unwrap_or("")
    }
}

/// Decode mirror — every field optional so the `urls`-or-legacy-`url` fallback
/// and the dropped pre-unification keys are handled in one place. Unknown keys
/// (incl. `autoSwitchWifiNames` / `autoSwitchStrategy`) are ignored by serde.
#[derive(Deserialize)]
struct ServerConfigRaw {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    urls: Option<Vec<String>>,
    username: String,
    password: String,
}

impl<'de> Deserialize<'de> for ServerConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = ServerConfigRaw::deserialize(deserializer)?;
        // `urls` is the source of truth when present and non-empty; otherwise
        // fall back to the legacy single `url`. At least one must be present.
        let urls = match raw.urls {
            Some(u) if !u.is_empty() => u,
            _ => match raw.url {
                Some(u) => vec![u],
                None => {
                    return Err(de::Error::custom(
                        "ServerConfig requires non-empty `urls` or a legacy `url`",
                    ))
                }
            },
        };
        Ok(ServerConfig {
            id: raw.id,
            name: raw.name,
            urls,
            username: raw.username,
            password: raw.password,
        })
    }
}

impl Serialize for ServerConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Emit BOTH `url` (== urls[0]) and `urls`; `name` only when present
        // (Swift `encodeIfPresent`). Key order mirrors Swift's hand-written
        // encoder for human-readable diffs (decoders are order-insensitive).
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &self.id)?;
        if let Some(name) = &self.name {
            map.serialize_entry("name", name)?;
        }
        map.serialize_entry("url", self.url())?;
        map.serialize_entry("urls", &self.urls)?;
        map.serialize_entry("username", &self.username)?;
        map.serialize_entry("password", &self.password)?;
        map.end()
    }
}

/// Persisted multi-server collection (spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerConfigList {
    /// All known profiles, in publisher order.
    pub configs: Vec<ServerConfig>,
    /// The user's last explicit pick; falls back to `configs[0]` when stale.
    pub active_config_id: Option<String>,
}

impl ServerConfigList {
    /// §5.2 — stale `active_config_id` falls back to `configs[0]`; `None` iff
    /// `configs` is empty. Mirrors Swift `ServerConfigList.activeConfig`.
    pub fn active_config(&self) -> Option<&ServerConfig> {
        if self.configs.is_empty() {
            return None;
        }
        if let Some(id) = &self.active_config_id {
            if let Some(hit) = self.configs.iter().find(|c| &c.id == id) {
                return Some(hit);
            }
        }
        self.configs.first()
    }
}

#[derive(Deserialize)]
struct ServerConfigListRaw {
    #[serde(default)]
    configs: Vec<ServerConfig>,
    #[serde(default, rename = "activeConfigId")]
    active_config_id: Option<String>,
    // Decode-only: a pre-unification "pin" we promote into `activeConfigId`
    // and never re-encode (Swift `manualOverrideConfigId`).
    #[serde(default, rename = "manualOverrideConfigId")]
    manual_override_config_id: Option<String>,
}

impl<'de> Deserialize<'de> for ServerConfigList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = ServerConfigListRaw::deserialize(deserializer)?;
        // One-shot §5.2 migration: a resolvable legacy pin out-prioritized
        // `activeConfigId` in old builds — promote it, else keep the persisted
        // `activeConfigId`.
        let active_config_id = match raw.manual_override_config_id {
            Some(pin) if raw.configs.iter().any(|c| c.id == pin) => Some(pin),
            _ => raw.active_config_id,
        };
        Ok(ServerConfigList {
            configs: raw.configs,
            active_config_id,
        })
    }
}

impl Serialize for ServerConfigList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // `configs` always emitted (even empty); `activeConfigId` only when
        // present; `manualOverrideConfigId` never re-encoded.
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("configs", &self.configs)?;
        if let Some(active) = &self.active_config_id {
            map.serialize_entry("activeConfigId", active)?;
        }
        map.end()
    }
}

/// Read-only legacy single-config shape (spec §5.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyServerConfig {
    /// The single legacy base URL.
    pub url: String,
    /// HTTP Basic Auth username.
    pub username: String,
    /// HTTP Basic Auth password.
    pub password: String,
}

impl LegacyServerConfig {
    /// §5.5 — wrap into a `ServerConfigList` with the provided id and mark it
    /// active. The id is supplied by the native layer (a fresh lowercase UUID,
    /// Swift `idProvider`); the proto crate stays free of randomness so this
    /// remains a pure, deterministic transform.
    pub fn migrate(&self, new_id: String) -> ServerConfigList {
        let cfg = ServerConfig {
            id: new_id.clone(),
            name: None,
            urls: vec![self.url.clone()],
            username: self.username.clone(),
            password: self.password.clone(),
        };
        ServerConfigList {
            configs: vec![cfg],
            active_config_id: Some(new_id),
        }
    }
}

/// Outcome of [`load_servers`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerLoad {
    /// The resolved server list.
    pub list: ServerConfigList,
    /// `true` ⇒ a §5.5 legacy migration just ran: the native layer must persist
    /// `list` to `server_config_list` and remove the legacy `server_config`
    /// key (one-shot; idempotent on the next launch because the new key now
    /// exists).
    pub migrated: bool,
}

/// Resolve the server list, performing one-shot §5.5 legacy migration.
///
/// Mirrors `SettingsStore.loadServers` (snapshot in → decision out): the native
/// layer hands in the two raw blobs (each `None` when the key is absent) and a
/// freshly-minted id for the migration path, and applies the persistence
/// side-effects the result describes.
///
/// Policy:
/// - `server_config_list` present and decodable → use it (no migration).
/// - present but corrupt → empty list (Swift logs a fault; it does NOT fall
///   through to the legacy key when the new key exists).
/// - absent, legacy present and decodable → migrate, signal persist+drop.
/// - otherwise → empty list.
pub fn load_servers(
    list_blob: Option<&[u8]>,
    legacy_blob: Option<&[u8]>,
    new_id: String,
) -> ServerLoad {
    if let Some(bytes) = list_blob {
        let list = serde_json::from_slice(bytes).unwrap_or_default();
        return ServerLoad {
            list,
            migrated: false,
        };
    }
    if let Some(bytes) = legacy_blob {
        if let Ok(legacy) = serde_json::from_slice::<LegacyServerConfig>(bytes) {
            return ServerLoad {
                list: legacy.migrate(new_id),
                migrated: true,
            };
        }
    }
    ServerLoad {
        list: ServerConfigList::default(),
        migrated: false,
    }
}

/// Encode a `ServerConfigList` to its persisted JSON form.
pub fn encode_server_list(list: &ServerConfigList) -> Vec<u8> {
    serde_json::to_vec(list).unwrap_or_default()
}

/// Decode a `server_config_list` blob; corruption returns the empty list
/// (Swift `SettingsStore.loadServers` corruption policy).
pub fn decode_server_list(bytes: &[u8]) -> ServerConfigList {
    serde_json::from_slice(bytes).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_blob(list: &ServerConfigList) -> Vec<u8> {
        encode_server_list(list)
    }

    /// Swift `test_servers_saveThenLoad_isRoundTripEqual`.
    #[test]
    fn round_trip_preserves_multi_url() {
        let original = ServerConfigList {
            configs: vec![ServerConfig {
                id: "abc".to_string(),
                name: Some("NAS".to_string()),
                urls: vec![
                    "https://nas.lan/".to_string(),
                    "http://192.168.0.9:5033".to_string(),
                ],
                username: "u".to_string(),
                password: "p".to_string(),
            }],
            active_config_id: Some("abc".to_string()),
        };
        assert_eq!(decode_server_list(&list_blob(&original)), original);
    }

    /// Swift `ServerConfig.encode`: BOTH `url` and `urls` are emitted, `url`
    /// equals `urls[0]`.
    #[test]
    fn encode_emits_both_url_and_urls() {
        let list = ServerConfigList {
            configs: vec![ServerConfig {
                id: "x".to_string(),
                name: None,
                urls: vec!["https://a/".to_string(), "https://b/".to_string()],
                username: "u".to_string(),
                password: "p".to_string(),
            }],
            active_config_id: None,
        };
        let json = String::from_utf8(encode_server_list(&list)).unwrap();
        assert!(json.contains("\"url\":\"https://a/\""), "url == urls[0]");
        assert!(json.contains("\"urls\":[\"https://a/\",\"https://b/\"]"));
        assert!(!json.contains("\"name\""), "name omitted when None");
        assert!(!json.contains("activeConfigId"), "active omitted when None");
    }

    /// Swift `ServerConfig.init(from:)`: legacy single `url` becomes a
    /// one-element candidate list when `urls` is absent.
    #[test]
    fn decodes_legacy_single_url() {
        let cfg: ServerConfig = serde_json::from_str(
            r#"{"id":"i","url":"https://legacy/","username":"u","password":"p"}"#,
        )
        .unwrap();
        assert_eq!(cfg.urls, vec!["https://legacy/".to_string()]);
        assert_eq!(cfg.url(), "https://legacy/");
    }

    /// Pre-unification per-config keys are decoded-and-dropped.
    #[test]
    fn ignores_dropped_autoswitch_keys() {
        let cfg: ServerConfig = serde_json::from_str(
            r#"{"id":"i","urls":["https://a/"],"username":"u","password":"p","autoSwitchStrategy":"foo","autoSwitchWifiNames":["X"]}"#,
        )
        .unwrap();
        assert_eq!(cfg.urls, vec!["https://a/".to_string()]);
    }

    #[test]
    fn config_with_neither_url_nor_urls_errors() {
        let r = serde_json::from_str::<ServerConfig>(r#"{"id":"i","username":"u","password":"p"}"#);
        assert!(r.is_err());
        // An empty `urls` array also falls through to the legacy `url` (absent
        // here) → error.
        let r = serde_json::from_str::<ServerConfig>(
            r#"{"id":"i","urls":[],"username":"u","password":"p"}"#,
        );
        assert!(r.is_err());
    }

    /// Swift `ServerConfigList.init(from:)`: a resolvable `manualOverrideConfigId`
    /// is promoted to `activeConfigId` and never re-encoded.
    #[test]
    fn promotes_resolvable_legacy_pin() {
        let json = r#"{
            "configs":[
                {"id":"a","urls":["https://a/"],"username":"u","password":"p"},
                {"id":"b","urls":["https://b/"],"username":"u","password":"p"}
            ],
            "activeConfigId":"a",
            "manualOverrideConfigId":"b"
        }"#;
        let list: ServerConfigList = serde_json::from_str(json).unwrap();
        assert_eq!(list.active_config_id, Some("b".to_string()), "pin wins");
        // Re-encode never carries the legacy key forward.
        let re = String::from_utf8(encode_server_list(&list)).unwrap();
        assert!(!re.contains("manualOverrideConfigId"));
        assert!(re.contains("\"activeConfigId\":\"b\""));
    }

    /// An unresolvable pin (id not in `configs`) falls back to `activeConfigId`.
    #[test]
    fn unresolvable_pin_falls_back_to_active() {
        let json = r#"{
            "configs":[{"id":"a","urls":["https://a/"],"username":"u","password":"p"}],
            "activeConfigId":"a",
            "manualOverrideConfigId":"ghost"
        }"#;
        let list: ServerConfigList = serde_json::from_str(json).unwrap();
        assert_eq!(list.active_config_id, Some("a".to_string()));
    }

    /// Swift `test_loadServers_whenOnlyLegacyKeyPresent_migratesAndDropsLegacy`.
    #[test]
    fn load_servers_migrates_legacy_only() {
        let legacy = LegacyServerConfig {
            url: "https://legacy.example.com/".to_string(),
            username: "user".to_string(),
            password: "pw".to_string(),
        };
        let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
        let loaded = load_servers(None, Some(&legacy_bytes), "fresh-id".to_string());
        assert!(loaded.migrated, "native must persist + drop the legacy key");
        assert_eq!(loaded.list.configs.len(), 1);
        let cfg = &loaded.list.configs[0];
        assert_eq!(cfg.url(), legacy.url);
        assert_eq!(cfg.username, legacy.username);
        assert_eq!(cfg.password, legacy.password);
        assert_eq!(cfg.name, None);
        assert_eq!(cfg.urls, vec![legacy.url.clone()]);
        assert_eq!(loaded.list.active_config_id, Some("fresh-id".to_string()));
        assert_eq!(cfg.id, "fresh-id");
    }

    /// Swift `test_loadServers_whenBothKeysPresent_newWinsAndLegacyUntouched`.
    #[test]
    fn load_servers_new_key_wins_over_legacy() {
        let new_list = ServerConfigList {
            configs: vec![ServerConfig {
                id: "new".to_string(),
                name: None,
                urls: vec!["https://new/".to_string()],
                username: "n".to_string(),
                password: "n".to_string(),
            }],
            active_config_id: Some("new".to_string()),
        };
        let new_bytes = encode_server_list(&new_list);
        let legacy = LegacyServerConfig {
            url: "https://old/".to_string(),
            username: "o".to_string(),
            password: "o".to_string(),
        };
        let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
        let loaded = load_servers(Some(&new_bytes), Some(&legacy_bytes), "unused".to_string());
        assert!(
            !loaded.migrated,
            "new key present → no migration, legacy left alone"
        );
        assert_eq!(loaded.list, new_list);
    }

    /// Swift `test_loadServers_whenJSONIsCorrupt_returnsEmptyList` — a corrupt
    /// new key does NOT fall through to the legacy key.
    #[test]
    fn load_servers_corrupt_new_key_returns_empty() {
        let legacy = LegacyServerConfig {
            url: "https://old/".to_string(),
            username: "o".to_string(),
            password: "o".to_string(),
        };
        let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
        let loaded = load_servers(Some(b"not json"), Some(&legacy_bytes), "unused".to_string());
        assert!(!loaded.migrated);
        assert_eq!(loaded.list, ServerConfigList::default());
    }

    #[test]
    fn load_servers_all_absent_returns_empty() {
        let loaded = load_servers(None, None, "unused".to_string());
        assert!(!loaded.migrated);
        assert_eq!(loaded.list, ServerConfigList::default());
    }

    /// §5.2 active-config fallback semantics.
    #[test]
    fn active_config_falls_back_to_first() {
        let mut list = ServerConfigList {
            configs: vec![
                ServerConfig {
                    id: "a".to_string(),
                    name: None,
                    urls: vec!["https://a/".to_string()],
                    username: "u".to_string(),
                    password: "p".to_string(),
                },
                ServerConfig {
                    id: "b".to_string(),
                    name: None,
                    urls: vec!["https://b/".to_string()],
                    username: "u".to_string(),
                    password: "p".to_string(),
                },
            ],
            active_config_id: Some("ghost".to_string()),
        };
        assert_eq!(list.active_config().map(|c| c.id.as_str()), Some("a"));
        list.active_config_id = Some("b".to_string());
        assert_eq!(list.active_config().map(|c| c.id.as_str()), Some("b"));
        let empty = ServerConfigList::default();
        assert_eq!(empty.active_config(), None);
    }
}
