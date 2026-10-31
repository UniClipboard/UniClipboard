//! Field set 1 migration and owner-split writes, against real daemons
//! (ADR-013 section 8, failure modes F38–F63, acceptance B1–B4, B7, B8, B11).
//!
//! Every test runs real `uniclipd` processes on isolated `e2e-*` profiles. The
//! import runs inside the daemon's bootstrap, so these tests also prove its
//! ordering: the importer logs the BLAKE3 hash of the `settings.json` bytes it
//! read, and each test compares it with the hash of the file it placed in the
//! profile before the start.
//!
//! Sources of the legacy `settings.json`:
//! - `v1_2_0_*`: written by a real v1.2.0 daemon (built from tag `v1.2.0`,
//!   `4438ff72bbf97fe9b2bdc23a0cf1ea67631b26fc`) through its own
//!   `PUT /settings`. Set `UC_E2E_V1_2_0_DAEMON` to that `uniclipd`.
//! - `desktop_0_19_4_*`: the `settings.json` of the Engine upgrade-matrix
//!   fixture produced by the released 0.19.4 CLI (default values only). Set
//!   `UC_E2E_DESKTOP_0_19_4_SETTINGS` to that file.
//! - `schema_2_*`: a synthetic file in the schema-2 shape that v0.17.x wrote
//!   (`silent_start`, no `startup_mode`). It is not a real old profile.
//!
//! Set `UC_E2E_ARTIFACT_DIR` to keep a JSON record per test, and
//! `UC_E2E_RUST_LOG=info` so the import log lines reach the log file.
//!
//! Run with:
//! `cargo build -p uc-daemon -p uc-dev-cli`
//! `cargo test --manifest-path tests/e2e/Cargo.toml --test settings_migration -- --ignored --test-threads=1`

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uc_e2e_tests::{get_session_token, NodeBinarySet, TestDaemon, TestProfile};

const PRODUCT: &str = "desktop-settings.json";
const LEGACY: &str = "settings.json";
const TELEMETRY: &str = "desktop-telemetry.json";

fn v1_2_0() -> NodeBinarySet {
    let daemon = std::env::var("UC_E2E_V1_2_0_DAEMON")
        .expect("UC_E2E_V1_2_0_DAEMON must point at a uniclipd built from tag v1.2.0");
    NodeBinarySet::fixed("1.2.0", "/nonexistent/uniclip", daemon)
}

struct Run {
    name: &'static str,
    daemon: TestDaemon,
    client: reqwest::Client,
    session: String,
    record: Vec<Value>,
}

impl Run {
    /// Seed files into a fresh profile, then start `binaries` on it.
    async fn start(name: &'static str, binaries: &NodeBinarySet, seed: &[(&str, &[u8])]) -> Self {
        let profile = TestProfile::new(name);
        std::fs::create_dir_all(profile.data_dir()).unwrap();
        for (file, bytes) in seed {
            std::fs::write(profile.data_dir().join(file), bytes).unwrap();
        }
        let seeded = seed
            .iter()
            .map(|(file, bytes)| json!({ "file": file, "sha256": sha256(bytes), "blake3": blake3_hex(bytes), "content": String::from_utf8_lossy(bytes) }))
            .collect::<Vec<_>>();
        let daemon = TestDaemon::start_preserving_with(profile, binaries, None)
            .await
            .unwrap_or_else(|error| panic!("{name}: daemon start failed: {error}"));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap();
        let session = get_session_token(&daemon, &client).await;
        let mut run = Self {
            name,
            daemon,
            client,
            session,
            record: Vec::new(),
        };
        run.note(
            "started",
            json!({ "binary": binaries.version, "seed": seeded }),
        );
        run
    }

    fn path(&self, file: &str) -> PathBuf {
        self.daemon.profile.data_dir().join(file)
    }

    fn bytes(&self, file: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path(file)).ok()
    }

    fn product(&self) -> Value {
        serde_json::from_slice(&self.bytes(PRODUCT).expect("product file")).unwrap()
    }

    fn note(&mut self, step: &str, detail: Value) {
        let product = self.bytes(PRODUCT);
        let legacy = self.bytes(LEGACY);
        self.record.push(json!({
            "step": step,
            "productSha256": product.as_deref().map(sha256),
            "legacySha256": legacy.as_deref().map(sha256),
            "productFile": product.as_deref().map(String::from_utf8_lossy),
            "legacyFile": legacy.as_deref().map(String::from_utf8_lossy),
            "detail": detail,
        }));
    }

    async fn settings(&self) -> Value {
        let response = self
            .client
            .get(format!("{}/settings", self.daemon.base_url()))
            .header("Authorization", format!("Session {}", self.session))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{}: GET /settings", self.name);
        let body: Value = response.json().await.unwrap();
        body["data"].clone()
    }

    async fn put_at(&mut self, route: &str, step: &str, body: Value) -> (u16, Value) {
        let response = self
            .client
            .put(format!("{}{route}", self.daemon.base_url()))
            .header("Authorization", format!("Session {}", self.session))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let reply: Value = response.json().await.unwrap_or(Value::Null);
        self.note(step, json!({ "route": route, "request": body, "status": status, "response": reply }));
        (status, reply)
    }

    async fn put(&mut self, step: &str, body: Value) -> (u16, Value) {
        self.put_at("/settings", step, body).await
    }

    async fn put_raw(&mut self, step: &str, body: &str) -> u16 {
        let status = self
            .client
            .put(format!("{}/settings", self.daemon.base_url()))
            .header("Authorization", format!("Session {}", self.session))
            .header("Content-Type", "application/json")
            .body(body.to_owned())
            .send()
            .await
            .unwrap()
            .status()
            .as_u16();
        self.note(step, json!({ "rawRequest": body, "status": status }));
        status
    }

    /// Stop cleanly and start `binaries` on the same profile.
    async fn switch_to(&mut self, binaries: &NodeBinarySet, step: &str) {
        self.daemon.stop_gracefully().await.unwrap();
        self.note(&format!("{step}: stopped"), Value::Null);
        self.daemon
            .restart_preserving_with(binaries, None)
            .await
            .unwrap_or_else(|error| panic!("{}: restart failed: {error}", self.name));
        self.session = get_session_token(&self.daemon, &self.client).await;
        self.note(&format!("{step}: started {}", binaries.version), Value::Null);
    }

    async fn restart(&mut self, step: &str) {
        self.switch_to(&NodeBinarySet::current(), step).await;
    }

    /// Settings-related log lines of every daemon run on this profile.
    fn log_lines(&self) -> Vec<String> {
        let mut sources = vec![self.daemon.profile.process_log_path()];
        if let Ok(entries) = std::fs::read_dir(self.daemon.profile.log_dir()) {
            sources.extend(entries.flatten().map(|entry| entry.path()));
        }
        let keys = [
            "desktop_settings",
            "legacy_settings",
            "product settings",
            "Legacy settings",
            "analytics gate",
            "settings_partially_applied",
            "Product settings",
        ];
        sources
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .flat_map(|text| {
                text.lines()
                    .filter(|line| keys.iter().any(|key| line.contains(key)))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn all_log_text(&self) -> String {
        let mut sources = vec![self.daemon.profile.process_log_path()];
        if let Ok(entries) = std::fs::read_dir(self.daemon.profile.log_dir()) {
            sources.extend(entries.flatten().map(|entry| entry.path()));
        }
        sources
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .collect()
    }

    fn import_lines(&self, needle: &str) -> Vec<String> {
        self.log_lines()
            .into_iter()
            .filter(|line| line.contains(needle))
            .collect()
    }

    /// Matching lines of the JSON log files only; the process log repeats each event.
    fn count_events(&self, needle: &str) -> usize {
        self.import_lines(needle)
            .iter()
            .filter(|line| line.starts_with('{'))
            .count()
    }

    fn keep(&self) {
        let Ok(dir) = std::env::var("UC_E2E_ARTIFACT_DIR") else {
            return;
        };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let artifact = json!({
            "test": self.name,
            "profile": self.daemon.profile.name,
            "steps": self.record,
            "settingsLog": self.log_lines(),
        });
        std::fs::write(
            dir.join(format!("{}.json", self.name)),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// The importer logged the hash of exactly these bytes (F38, F40).
fn assert_import_read(run: &Run, legacy: &[u8]) {
    let hash = blake3_hex(legacy);
    let lines = run.import_lines("legacy_settings_blake3");
    assert!(
        lines.iter().any(|line| line.contains(&hash)),
        "{}: no import log with the seeded settings.json hash {hash}: {lines:#?}",
        run.name
    );
}

/// The non-default values a v1.2.0 user can hold, written through v1.2.0's own API.
fn v1_2_0_values() -> Value {
    json!({
        "general": {
            "autoStart": true,
            "startupMode": "silent",
            "restoreLastEntryOnStartup": true,
            "autoCheckUpdate": false,
            "autoDownloadUpdate": false,
            "updateChannel": "beta",
            "theme": "dark",
            "themeColor": "catppuccin",
            "themeOverridesLight": { "primary": "oklch(0.6193 0.2029 312.742)" },
            "themeOverridesDark": { "background": "#112233" },
            "language": "ja-JP",
            "usageAnalyticsEnabled": false,
            "deviceName": "MigrationFixture"
        },
        "keyboardShortcuts": { "shortcuts": {
            "global.toggleQuickPanel": "meta+shift+k",
            "nav.toggleSidebar": ["ctrl+b", "alt+f24"],
            "quickPanel.windowIncrease": "ctrl+ä",
            "quickPanel.windowDecrease": "ctrl+f25"
        } },
        "quickPanel": { "enabled": false, "position": "follow_cursor", "doubleTapModifier": "alt" }
    })
}

fn assert_v1_2_0_values(settings: &Value) {
    let general = &settings["general"];
    for (key, expected) in [
        ("autoStart", json!(true)),
        ("startupMode", json!("normal")),
        ("backgroundStartup", json!(true)),
        ("restoreLastEntryOnStartup", json!(true)),
        ("autoCheckUpdate", json!(false)),
        ("autoDownloadUpdate", json!(false)),
        ("updateChannel", json!("beta")),
        ("theme", json!("dark")),
        ("themeColor", Value::Null),
        ("themeColorLight", json!("catppuccin")),
        ("themeColorDark", json!("catppuccin")),
        (
            "themeOverridesLight",
            json!({ "primary": "oklch(0.6193 0.2029 312.742)" }),
        ),
        ("themeOverridesDark", json!({ "background": "#112233" })),
        ("language", json!("ja-JP")),
        ("usageAnalyticsEnabled", json!(false)),
        ("deviceName", json!("MigrationFixture")),
    ] {
        assert_eq!(general[key], expected, "general.{key}");
    }
    // Out-of-set overrides are not effective: the built-in binding applies.
    assert_eq!(
        settings["keyboardShortcuts"],
        json!({
            "global.toggleQuickPanel": "meta+shift+k",
            "nav.toggleSidebar": ["ctrl+b", "alt+f24"]
        })
    );
    assert_eq!(
        settings["quickPanel"],
        json!({ "enabled": false, "position": "follow_cursor", "doubleTapModifier": "alt" })
    );
}

/// B1, B1b, B11, F38, F40, F45, F48 (shortcut tokens), F49 (theme fallback), F61, F63.
#[tokio::test]
#[ignore]
async fn v1_2_0_profile_imports_every_value_once_and_keeps_out_of_set_shortcuts_raw() {
    let mut run = Run::start("migration-v120", &v1_2_0(), &[]).await;
    let (status, _) = run.put("v1.2.0 writes non-default values", v1_2_0_values()).await;
    assert_eq!(status, 200);
    run.daemon.stop_gracefully().await.unwrap();
    let legacy = run.bytes(LEGACY).expect("v1.2.0 wrote settings.json");
    assert!(run.bytes(PRODUCT).is_none(), "v1.2.0 has no product file");
    run.note("v1.2.0 stopped", json!({ "legacyBlake3": blake3_hex(&legacy) }));

    run.switch_to(&NodeBinarySet::current(), "upgrade to current").await;
    assert_import_read(&run, &legacy);
    let settings = run.settings().await;
    assert_v1_2_0_values(&settings);
    run.note("current GET after import", settings.clone());

    let product = run.product();
    assert_eq!(product["import"]["fieldSet"], 2);
    assert!(product["import"]["sourceFingerprint"].is_string());
    // Raw values of out-of-set shortcuts stay in the product file.
    assert_eq!(
        product["keyboardShortcuts"]["quickPanel.windowIncrease"],
        json!("ctrl+ä")
    );
    assert_eq!(
        product["keyboardShortcuts"]["quickPanel.windowDecrease"],
        json!("ctrl+f25")
    );
    let invalid = run.import_lines("desktop_settings_import_invalid_field");
    assert!(
        invalid
            .iter()
            .any(|line| line.contains("keyboardShortcuts.quickPanel.windowIncrease")
                && line.contains("keyboardShortcuts.quickPanel.windowDecrease")),
        "{invalid:#?}"
    );

    // B1b: Engine rewrote settings.json after load; later starts do not import again.
    let product_after_import = run.bytes(PRODUCT).unwrap();
    for start in ["second start", "third start"] {
        run.restart(start).await;
        assert_eq!(run.bytes(PRODUCT).unwrap(), product_after_import, "{start}");
        assert_v1_2_0_values(&run.settings().await);
    }
    assert_eq!(
        run.count_events("Imported product settings from the legacy Engine settings"),
        1
    );

    // F63: no setting value reaches the log.
    let log = run.all_log_text();
    for value in ["catppuccin", "ctrl+ä", "ctrl+f25", "meta+shift+k", "oklch(0.6193"] {
        assert!(!log.contains(value), "log leaks {value}");
    }
    run.note("completed", Value::Null);
    run.keep();
}

/// B4, F46, F47: downgrade to v1.2.0, edit there, upgrade again.
#[tokio::test]
#[ignore]
async fn v1_2_0_edits_after_a_downgrade_are_imported_except_paths_written_under_p1() {
    let mut run = Run::start("migration-downgrade", &v1_2_0(), &[]).await;
    assert_eq!(run.put("v1.2.0 initial values", v1_2_0_values()).await.0, 200);
    run.switch_to(&NodeBinarySet::current(), "upgrade").await;
    assert_v1_2_0_values(&run.settings().await);

    // A P1 write: general.theme enters userWritten.
    assert_eq!(
        run.put("P1 writes theme", json!({ "general": { "theme": "light" } }))
            .await
            .0,
        200
    );
    let legacy_before_downgrade = run.bytes(LEGACY).unwrap();

    run.switch_to(&v1_2_0(), "downgrade").await;
    // Without an Engine write, the old binary still reads its frozen values.
    // The Desktop-owned P1 write remains authoritative when upgrading again.
    assert_eq!(run.settings().await["general"]["theme"], json!("dark"));
    assert_eq!(
        run.put(
            "v1.2.0 edits language and theme",
            json!({ "general": { "language": "pt-BR", "theme": "system" } })
        )
        .await
        .0,
        200
    );
    run.daemon.stop_gracefully().await.unwrap();
    let legacy_after_downgrade = run.bytes(LEGACY).unwrap();
    assert_ne!(legacy_before_downgrade, legacy_after_downgrade);

    run.switch_to(&NodeBinarySet::current(), "upgrade again").await;
    assert_import_read(&run, &legacy_after_downgrade);
    let settings = run.settings().await;
    assert_eq!(settings["general"]["language"], json!("pt-BR"));
    assert_eq!(settings["general"]["theme"], json!("light"));
    let repeated = run.import_lines("repeated=true");
    let repeated_json = run.import_lines("\"repeated\":true");
    assert!(
        repeated
            .iter()
            .chain(repeated_json.iter())
            .any(|line| line.contains("general.language")),
        "{:#?}",
        run.log_lines()
    );
    let kept = run.import_lines("desktop_settings_import_kept_user_value");
    assert!(
        kept.iter().any(|line| line.contains("general.theme")),
        "{kept:#?}"
    );
    run.note("completed", settings);
    run.keep();
}

/// B1 on the released 0.19.4 settings.json (defaults only), F50.
#[tokio::test]
#[ignore]
async fn desktop_0_19_4_settings_import_with_engine_defaults() {
    let path = std::env::var("UC_E2E_DESKTOP_0_19_4_SETTINGS").expect(
        "UC_E2E_DESKTOP_0_19_4_SETTINGS must point at the Engine fixture desktop-0.19.4/profile/settings.json",
    );
    let legacy = std::fs::read(path).unwrap();
    let mut run = Run::start(
        "migration-0194",
        &NodeBinarySet::current(),
        &[(LEGACY, &legacy)],
    )
    .await;
    assert_import_read(&run, &legacy);
    let settings = run.settings().await;
    assert_engine_defaults(&settings);
    assert_eq!(settings["general"]["deviceName"], json!("LegacyFixture"));
    let product = run.product();
    assert_eq!(product["import"]["fieldSet"], 2);
    assert_eq!(product["general"]["startupMode"], json!("normal"));

    // B1b on a real Engine rewrite: an Engine-only write re-serializes the
    // historical file (for example the `auto_sync` alias); the next start must
    // not import again.
    assert_eq!(
        run.put("engine-only write", json!({ "sync": { "syncOnRestore": true } }))
            .await
            .0,
        200
    );
    let rewritten = run.bytes(LEGACY).unwrap();
    assert_ne!(rewritten, legacy, "Engine did not rewrite settings.json");
    let product_bytes = run.bytes(PRODUCT).unwrap();
    run.restart("start after the Engine rewrote the 0.19.4 file").await;
    // Product fields were erased by Engine; absence must not trigger an import.
    assert_eq!(run.bytes(PRODUCT).unwrap(), product_bytes);
    assert_eq!(
        run.count_events("Imported product settings from the legacy Engine settings"),
        1
    );
    run.note("completed", settings);
    run.keep();
}

fn assert_engine_defaults(settings: &Value) {
    let general = &settings["general"];
    for (key, expected) in [
        ("autoStart", json!(false)),
        ("startupMode", json!("normal")),
        ("restoreLastEntryOnStartup", json!(false)),
        ("autoCheckUpdate", json!(true)),
        ("autoDownloadUpdate", json!(true)),
        ("updateChannel", Value::Null),
        ("theme", json!("system")),
        ("themeColor", Value::Null),
        ("themeColorLight", Value::Null),
        ("themeColorDark", Value::Null),
        ("themeOverridesLight", json!({})),
        ("themeOverridesDark", json!({})),
        ("language", Value::Null),
        ("usageAnalyticsEnabled", json!(true)),
        ("trayIcon", json!("glyph")),
    ] {
        assert_eq!(general[key], expected, "general.{key}");
    }
    assert_eq!(settings["keyboardShortcuts"], json!({}));
    assert_eq!(
        settings["quickPanel"],
        json!({ "enabled": true, "position": "center", "doubleTapModifier": "disabled" })
    );
}

/// F43, F50: no legacy file. The marker advances without values.
#[tokio::test]
#[ignore]
async fn missing_legacy_file_advances_the_marker_and_uses_engine_defaults() {
    let mut run = Run::start("migration-fresh", &NodeBinarySet::current(), &[]).await;
    assert_engine_defaults(&run.settings().await);
    let product = run.product();
    assert_eq!(product["import"]["fieldSet"], 2);
    assert!(product["import"]["sourceFingerprint"].is_string());
    // The Engine writes its defaults later; the next start must not import again.
    assert_eq!(
        run.put("engine-only write", json!({ "sync": { "syncOnRestore": true } }))
            .await
            .0,
        200
    );
    assert!(run.bytes(LEGACY).is_some());
    let product_bytes = run.bytes(PRODUCT).unwrap();
    run.restart("start after the Engine wrote its defaults").await;
    assert_eq!(run.bytes(PRODUCT).unwrap(), product_bytes);
    assert_eq!(
        run.count_events("Imported product settings from the legacy Engine settings"),
        0
    );
    assert!(!run
        .import_lines("Product settings import marker advanced without legacy values")
        .is_empty());
    run.note("completed", product);
    run.keep();
}

/// F49, F45: schema-2 shape (synthetic). Legacy startup precedence and theme
/// fallback; Engine's v2→v3 rewrite does not trigger a second import.
#[tokio::test]
#[ignore]
async fn schema_2_legacy_rules_apply_and_the_engine_migration_does_not_reimport() {
    let legacy = serde_json::to_vec_pretty(&json!({
        "schema_version": 2,
        "general": {
            "silent_start": true,
            "lightweight_start": true,
            "theme_color": "rose",
            "theme_color_light": "blue",
            "theme_color_dark": null
        }
    }))
    .unwrap();
    let mut run = Run::start(
        "migration-schema2",
        &NodeBinarySet::current(),
        &[(LEGACY, &legacy)],
    )
    .await;
    assert_import_read(&run, &legacy);
    let settings = run.settings().await;
    assert_eq!(settings["general"]["startupMode"], json!("lightweight"));
    assert_eq!(settings["general"]["backgroundStartup"], json!(true));
    assert_eq!(settings["general"]["themeColorLight"], json!("blue"));
    assert_eq!(settings["general"]["themeColorDark"], json!("rose"));
    let product = run.bytes(PRODUCT).unwrap();

    run.restart("after the Engine v2→v3 rewrite").await;
    let rewritten = run.bytes(LEGACY).unwrap();
    assert_ne!(rewritten, legacy, "Engine did not rewrite settings.json");
    let rewritten_json: Value = serde_json::from_slice(&rewritten).unwrap();
    assert_eq!(rewritten_json["schema_version"], 3);
    assert_eq!(rewritten_json["general"]["startup_mode"], Value::Null);
    assert_eq!(run.settings().await["general"]["startupMode"], json!("lightweight"));
    assert_eq!(run.bytes(PRODUCT).unwrap(), product);
    assert_eq!(
        run.count_events("Imported product settings from the legacy Engine settings"),
        1
    );

    // Silent alone.
    let legacy = serde_json::to_vec(&json!({
        "schema_version": 2,
        "general": { "silent_start": true }
    }))
    .unwrap();
    let silent = Run::start(
        "migration-schema2-silent",
        &NodeBinarySet::current(),
        &[(LEGACY, &legacy)],
    )
    .await;
    let silent_settings = silent.settings().await;
    assert_eq!(silent_settings["general"]["startupMode"], json!("normal"));
    assert_eq!(silent_settings["general"]["backgroundStartup"], json!(true));
    run.note("completed", settings);
    run.keep();
    silent.keep();
}

/// B1a, F41: a P0 file with trayIcon does not block the import and keeps trayIcon.
#[tokio::test]
#[ignore]
async fn p0_product_file_with_tray_icon_still_imports_field_set_1() {
    let product = br#"{"version":1,"import":{"userWritten":["general.trayIcon"]},"general":{"trayIcon":"cat"}}"#;
    let legacy = serde_json::to_vec(&json!({
        "schema_version": 3,
        "general": { "theme": "dark", "language": "zh-TW", "tray_icon": "glyph" },
        "quick_panel": { "position": "follow_cursor" }
    }))
    .unwrap();
    let mut run = Run::start(
        "migration-p0",
        &NodeBinarySet::current(),
        &[(PRODUCT, product), (LEGACY, &legacy)],
    )
    .await;
    let settings = run.settings().await;
    assert_eq!(settings["general"]["trayIcon"], json!("cat"));
    assert_eq!(settings["general"]["theme"], json!("dark"));
    assert_eq!(settings["general"]["language"], json!("zh-TW"));
    assert_eq!(settings["quickPanel"]["position"], json!("follow_cursor"));
    let file = run.product();
    assert_eq!(file["import"]["fieldSet"], 2);
    assert_eq!(file["import"]["userWritten"], json!(["general.trayIcon"]));
    run.note("completed", settings);
    run.keep();
}

/// B3, F48: invalid enum, invalid grammar and unknown keys in the legacy file.
#[tokio::test]
#[ignore]
async fn invalid_legacy_values_are_kept_raw_with_defaults_effective() {
    // Shortcut and override values are free-form for the Engine, so the
    // Engine still loads this file; only product grammars reject them.
    let legacy = serde_json::to_vec_pretty(&json!({
        "schema_version": 3,
        "general": {
            "theme_color_light": "Not A Preset",
            "theme_overrides_dark": { "accent": "#ffffff" },
            "language": "zz zz",
            "future_field": 1
        },
        "keyboard_shortcuts": { "global.toggleQuickPanel": "page up", "Bad Id": "ctrl+k" },
        "future_section": { "x": true }
    }))
    .unwrap();
    let mut run = Run::start(
        "migration-invalid",
        &NodeBinarySet::current(),
        &[(LEGACY, &legacy)],
    )
    .await;
    assert_import_read(&run, &legacy);
    let settings = run.settings().await;
    assert_eq!(settings["general"]["themeColorLight"], Value::Null);
    assert_eq!(settings["general"]["themeOverridesDark"], json!({}));
    assert_eq!(settings["general"]["language"], Value::Null);
    assert_eq!(settings["keyboardShortcuts"], json!({}));
    let product = run.product();
    assert_eq!(product["general"]["themeColorLight"], json!("Not A Preset"));
    assert_eq!(product["general"]["language"], json!("zz zz"));
    assert_eq!(
        product["keyboardShortcuts"]["global.toggleQuickPanel"],
        json!("page up")
    );
    let warnings = run.import_lines("desktop_settings_import_invalid_field");
    for path in [
        "general.themeColorLight",
        "general.themeOverridesDark",
        "general.language",
        "keyboardShortcuts.global.toggleQuickPanel",
        "keyboardShortcuts.Bad Id",
    ] {
        assert!(
            warnings.iter().any(|line| line.contains(path)),
            "{path}: {warnings:#?}"
        );
    }
    let log = run.all_log_text();
    assert!(!log.contains("Not A Preset") && !log.contains("zz zz"));
    run.note("completed", settings);
    run.keep();
}

/// B3 for an Engine-typed enum. Pass condition (design section 9): the session
/// starts. With the pinned Engine this fails: the Engine parses the frozen
/// product enum strictly and has no host hook for its settings load, so it
/// cannot pass before the Engine drops the field (tasks 7 and 10). The test
/// keeps the artifact and asserts the real pass condition, so it fails until then.
#[tokio::test]
#[ignore]
async fn invalid_engine_typed_enum_in_the_legacy_file_is_recorded() {
    // Control: the same input with a valid enum value. If it starts and the
    // `neon` input does not, the enum value is the only difference.
    let control = serde_json::to_vec_pretty(&json!({
        "schema_version": 3,
        "general": { "theme": "dark", "language": "ja-JP" }
    }))
    .unwrap();
    let control_run = Run::start(
        "migration-neon-control",
        &NodeBinarySet::current(),
        &[(LEGACY, &control)],
    )
    .await;
    assert_eq!(control_run.settings().await["general"]["theme"], json!("dark"));
    control_run.keep();
    drop(control_run);

    let legacy = serde_json::to_vec_pretty(&json!({
        "schema_version": 3,
        "general": { "theme": "neon", "language": "ja-JP" }
    }))
    .unwrap();
    let profile = TestProfile::new("migration-neon");
    std::fs::create_dir_all(profile.data_dir()).unwrap();
    std::fs::write(profile.data_dir().join(LEGACY), &legacy).unwrap();
    let mut daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        |_| {},
    )
    .unwrap();
    let healthy = daemon.wait_for_conn(Duration::from_secs(30)).await.is_ok()
        && daemon.wait_healthy(Duration::from_secs(30)).await.is_ok();
    let product: Value = serde_json::from_slice(
        &std::fs::read(daemon.profile.data_dir().join(PRODUCT)).expect("import wrote the file"),
    )
    .unwrap();
    assert_eq!(product["general"]["theme"], json!("neon"));
    assert_eq!(product["general"]["language"], json!("ja-JP"));
    let legacy_after = std::fs::read(daemon.profile.data_dir().join(LEGACY)).unwrap();
    let mut record = json!({
        "legacyInput": String::from_utf8_lossy(&legacy),
        "controlInput": String::from_utf8_lossy(&control),
        "controlStarted": true,
        "legacyBlake3": blake3_hex(&legacy),
        "daemonHealthy": healthy,
        "daemonRunning": daemon.is_running(),
        "legacyUnchanged": legacy_after == legacy,
        "productFile": product,
    });
    if healthy {
        let client = reqwest::Client::new();
        let session = get_session_token(&daemon, &client).await;
        let settings: Value = client
            .get(format!("{}/settings", daemon.base_url()))
            .header("Authorization", format!("Session {session}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        record["getSettings"] = settings["data"].clone();
        record["getSettingsStatus"] = json!(settings.get("code").cloned());
    }
    record["processLogTail"] = json!(daemon
        .diagnostic_log()
        .lines()
        .rev()
        .take(40)
        .collect::<Vec<_>>());
    let mut cause = Vec::new();
    if let Ok(entries) = std::fs::read_dir(daemon.profile.log_dir()) {
        for entry in entries.flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            cause.extend(
                text.lines()
                    .filter(|line| {
                        line.contains("\"level\":\"ERROR\"") || line.contains("unknown variant")
                    })
                    .map(str::to_owned),
            );
        }
    }
    record["errorLines"] = json!(cause);
    if let Ok(dir) = std::env::var("UC_E2E_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join("migration-neon.json"),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
    }
    daemon.kill();
    assert!(healthy, "B3: the Engine session did not start; see migration-neon.json");
}

/// F42: unreadable legacy JSON does not advance the marker; a repaired file imports.
#[tokio::test]
#[ignore]
async fn invalid_legacy_json_does_not_advance_the_marker() {
    let broken = b"{\"general\": {\"theme\": \"dark\"";
    let profile = TestProfile::new("migration-broken");
    std::fs::create_dir_all(profile.data_dir()).unwrap();
    std::fs::write(profile.data_dir().join(LEGACY), broken).unwrap();
    let mut daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        |_| {},
    )
    .unwrap();
    let healthy = daemon.wait_for_conn(Duration::from_secs(30)).await.is_ok()
        && daemon.wait_healthy(Duration::from_secs(30)).await.is_ok();
    let data = daemon.profile.data_dir().clone();
    let product_after_broken = std::fs::read(data.join(PRODUCT)).ok();
    let legacy_after_broken = std::fs::read(data.join(LEGACY)).ok();
    let marker = product_after_broken
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .and_then(|product| product["import"]["fieldSet"].as_u64());
    assert_eq!(marker, None, "marker advanced on invalid JSON");
    daemon.kill();

    let repaired = serde_json::to_vec(&json!({ "general": { "theme": "dark" } })).unwrap();
    std::fs::write(data.join(LEGACY), &repaired).unwrap();
    daemon
        .restart_preserving_with(&NodeBinarySet::current(), None)
        .await
        .unwrap();
    let product: Value = serde_json::from_slice(&std::fs::read(data.join(PRODUCT)).unwrap()).unwrap();
    assert_eq!(product["import"]["fieldSet"], 2);
    assert_eq!(product["general"]["theme"], json!("dark"));
    let deferred = std::fs::read_to_string(daemon.profile.process_log_path()).unwrap_or_default()
        + &std::fs::read_dir(daemon.profile.log_dir())
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
                    .collect::<String>()
            })
            .unwrap_or_default();
    assert!(deferred.contains("legacy_settings_unreadable") && deferred.contains("malformed_json"));
    if let Ok(dir) = std::env::var("UC_E2E_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join("migration-broken.json"),
            serde_json::to_vec_pretty(&json!({
                "brokenInput": String::from_utf8_lossy(broken),
                "firstStartHealthy": healthy,
                "productAfterBroken": product_after_broken.as_deref().map(String::from_utf8_lossy),
                "legacyAfterBroken": legacy_after_broken.as_deref().map(String::from_utf8_lossy),
                "repairedInput": String::from_utf8_lossy(&repaired),
                "productAfterRepair": product,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    daemon.kill();
}

/// B2, F44: the import cannot complete (the product path is a directory, so the
/// importer finds the product file unreadable and defers), then succeeds. Also a
/// kill right after the import was persisted. The persist-failure branch itself
/// cannot be forced here: any path that makes the rename fail also makes the
/// read fail first.
#[tokio::test]
#[ignore]
async fn a_failed_or_interrupted_import_repeats_to_the_same_result() {
    let legacy = serde_json::to_vec(&json!({
        "schema_version": 3,
        "general": { "theme": "dark", "auto_check_update": false },
        "keyboard_shortcuts": { "global.toggleQuickPanel": "ctrl+shift+space" }
    }))
    .unwrap();
    let profile = TestProfile::new("migration-crash");
    let data = profile.data_dir().clone();
    std::fs::create_dir_all(data.join(PRODUCT).join("blocker")).unwrap();
    std::fs::write(data.join(LEGACY), &legacy).unwrap();
    let mut daemon = TestDaemon::spawn_preserving_configured_with(
        profile, &NodeBinarySet::current(), None, |_| {},
    ).unwrap();
    assert!(daemon.wait_healthy(Duration::from_secs(30)).await.is_err());
    assert!(!data.join("daemon.conn").exists());
    assert_eq!(std::fs::read(data.join(LEGACY)).unwrap(), legacy);
    let mut log = std::fs::read_to_string(daemon.profile.process_log_path()).unwrap_or_default();
    for entry in std::fs::read_dir(daemon.profile.log_dir()).unwrap().flatten() {
        log.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
    }
    assert!(log.contains("desktop_settings_import_deferred") && log.contains("unreadable"));
    daemon.kill();
    let legacy_after_failed_import = std::fs::read(data.join(LEGACY)).unwrap();

    std::fs::remove_dir_all(data.join(PRODUCT)).unwrap();
    daemon
        .respawn_preserving_configured_with(|_| {})
        .unwrap();
    // Kill as soon as the import has persisted, before the Engine finishes starting.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !data.join(PRODUCT).is_file() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    daemon.kill();
    let after_kill: Value =
        serde_json::from_slice(&std::fs::read(data.join(PRODUCT)).unwrap()).unwrap();
    assert_eq!(after_kill["import"]["fieldSet"], 2);

    daemon
        .restart_preserving_with(&NodeBinarySet::current(), None)
        .await
        .unwrap();
    let after_restart: Value =
        serde_json::from_slice(&std::fs::read(data.join(PRODUCT)).unwrap()).unwrap();
    assert_eq!(after_restart, after_kill, "the start after the kill imported again");
    assert_eq!(after_restart["general"]["theme"], json!("dark"));
    assert_eq!(after_restart["general"]["autoCheckUpdate"], json!(false));
    assert_eq!(
        after_restart["keyboardShortcuts"]["global.toggleQuickPanel"],
        json!("ctrl+shift+space")
    );
    if let Ok(dir) = std::env::var("UC_E2E_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join("migration-crash.json"),
            serde_json::to_vec_pretty(&json!({
                "legacyInput": String::from_utf8_lossy(&legacy),
                "legacyBlake3": blake3_hex(&legacy),
                "legacyAfterFailedImport": String::from_utf8_lossy(&legacy_after_failed_import),
                "productAfterKill": after_kill,
                "productAfterRestart": after_restart,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    daemon.kill();
}

/// B1c (simulated): a config import replaces settings.json; the next start
/// restores paths not in userWritten and keeps written paths.
#[tokio::test]
#[ignore]
async fn replaced_legacy_file_restores_unwritten_paths_only() {
    let first = serde_json::to_vec(&json!({
        "schema_version": 3,
        "general": { "theme": "dark", "language": "en-US" }
    }))
    .unwrap();
    let mut run = Run::start(
        "migration-restore",
        &NodeBinarySet::current(),
        &[(LEGACY, &first)],
    )
    .await;
    assert_eq!(
        run.put("P1 writes language", json!({ "general": { "language": "ru-RU" } }))
            .await
            .0,
        200
    );
    run.daemon.stop_gracefully().await.unwrap();
    let bundle = serde_json::to_vec(&json!({
        "schema_version": 3,
        "general": { "theme": "light", "language": "zh-CN" },
        "quick_panel": { "enabled": false }
    }))
    .unwrap();
    std::fs::write(run.path(LEGACY), &bundle).unwrap();
    run.note("settings.json replaced as a config import does", Value::Null);
    run.daemon
        .restart_preserving_with(&NodeBinarySet::current(), None)
        .await
        .unwrap();
    run.session = get_session_token(&run.daemon, &run.client).await;
    assert_import_read(&run, &bundle);
    let settings = run.settings().await;
    assert_eq!(settings["general"]["theme"], json!("light"));
    assert_eq!(settings["quickPanel"]["enabled"], json!(false));
    assert_eq!(settings["general"]["language"], json!("ru-RU"));
    assert!(run
        .import_lines("desktop_settings_import_kept_user_value")
        .iter()
        .any(|line| line.contains("general.language")));
    run.note("completed", settings);
    run.keep();
}

/// F51, F52, F53, F54, F58, F61, F62: owner split and validation.
#[tokio::test]
#[ignore]
async fn product_writes_never_reach_the_engine_and_invalid_input_is_400() {
    let mut run = Run::start("split-owner", &NodeBinarySet::current(), &[]).await;
    // Create the Engine file with an Engine-only write.
    assert_eq!(
        run.put("engine-only write", json!({ "sync": { "syncOnRestore": true } }))
            .await
            .0,
        200
    );
    let legacy = run.bytes(LEGACY).expect("Engine file");

    let product_values = json!({
        "general": {
            "autoStart": true,
            "startupMode": "lightweight",
            "restoreLastEntryOnStartup": true,
            "autoCheckUpdate": false,
            "autoDownloadUpdate": false,
            "updateChannel": "rc",
            "theme": "light",
            "themeColorLight": "zz-unique-preset-9",
            "themeColorDark": "notebook",
            "themeOverridesLight": { "border": "#abcdef" },
            "themeOverridesDark": { "foreground": "oklch(1 0 0)" },
            "language": "pt-BR",
            "usageAnalyticsEnabled": false,
            "trayIcon": "cat"
        },
        "keyboardShortcuts": { "shortcuts": { "global.toggleQuickPanel": "super+ctrl+pageup f12" } },
        "quickPanel": { "enabled": false, "position": "follow_cursor", "doubleTapModifier": "meta" }
    });
    assert_eq!(run.put("product-only write", product_values.clone()).await.0, 200);
    assert_eq!(run.bytes(LEGACY).unwrap(), legacy, "a product write changed settings.json");
    let settings = run.settings().await;
    for (key, value) in product_values["general"].as_object().unwrap() {
        assert_eq!(&settings["general"][key], value, "general.{key}");
    }
    assert_eq!(
        settings["keyboardShortcuts"],
        json!({ "global.toggleQuickPanel": "super+ctrl+pageup f12" })
    );
    assert_eq!(settings["quickPanel"], product_values["quickPanel"]);
    // The frozen Engine copy is untouched and not shown (F52).
    let legacy_json: Value = serde_json::from_slice(&legacy).unwrap();
    assert_eq!(legacy_json["general"]["language"], Value::Null);

    // F53, F61: invalid values in a mixed patch; nothing is written.
    let product_before = run.bytes(PRODUCT).unwrap();
    for (step, body) in [
        ("f25 token", json!({ "sync": { "syncEnabled": false }, "keyboardShortcuts": { "shortcuts": { "global.toggleQuickPanel": "ctrl+f25" } } })),
        ("non-ASCII token", json!({ "sync": { "syncEnabled": false }, "keyboardShortcuts": { "shortcuts": { "global.toggleQuickPanel": "ctrl+ä" } } })),
        ("two-word key", json!({ "sync": { "syncEnabled": false }, "keyboardShortcuts": { "shortcuts": { "global.toggleQuickPanel": "ctrl+page up" } } })),
        ("preset grammar", json!({ "sync": { "syncEnabled": false }, "general": { "themeColorDark": "Bad Preset" } })),
        ("override token", json!({ "sync": { "syncEnabled": false }, "general": { "themeOverridesLight": { "accent": "#000000" } } })),
        ("language grammar", json!({ "sync": { "syncEnabled": false }, "general": { "language": "en US" } })),
    ] {
        let (status, reply) = run.put(step, body).await;
        assert_eq!(status, 400, "{step}: {reply}");
        assert_eq!(run.bytes(LEGACY).unwrap(), legacy, "{step}");
        assert_eq!(run.bytes(PRODUCT).unwrap(), product_before, "{step}");
    }
    // F54: malformed bodies are 400, also for Engine fields.
    assert_eq!(run.put_raw("unknown enum", r#"{"general":{"theme":"neon"}}"#).await, 400);
    assert_eq!(
        run.put_raw("engine field shape", r#"{"retentionPolicy":{"rules":[{"byAge":{"max_age":1}}]}}"#)
            .await,
        400
    );
    assert_eq!(run.put_raw("not json", "{").await, 400);
    // The relay save is an Engine operation: a product field there is 400.
    let (status, _) = run
        .put_at(
            "/settings/relay",
            "relay save with a product field",
            json!({ "settings": { "general": { "theme": "dark" } }, "url": "https://relay.example.com", "credential": { "action": "keep" } }),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(run.bytes(LEGACY).unwrap(), legacy);
    assert_eq!(run.settings().await["general"]["theme"], json!("light"));

    // A legacy themeColor in a PUT fills split presets that are null only.
    assert_eq!(
        run.put("clear-free product write", json!({ "general": { "themeColor": "amber" } }))
            .await
            .0,
        200
    );
    let settings = run.settings().await;
    assert_eq!(settings["general"]["themeColorLight"], json!("zz-unique-preset-9"));
    assert_eq!(settings["general"]["themeColor"], Value::Null);

    // F63: unique values never reach the log.
    let log = run.all_log_text();
    assert!(!log.contains("zz-unique-preset-9"));
    assert!(!log.contains("super+ctrl+pageup"));
    run.note("completed", Value::Null);
    run.keep();
}

/// B8, F55, F56, F57: partial failures with accurate owner lists.
#[tokio::test]
#[ignore]
async fn partial_failures_report_exactly_which_owners_were_written() {
    let mut run = Run::start("split-partial", &NodeBinarySet::current(), &[]).await;
    assert_eq!(
        run.put("seed product file", json!({ "general": { "theme": "dark" } }))
            .await
            .0,
        200
    );
    let telemetry_before = run.settings().await["general"]["telemetryEnabled"].clone();

    // Product write fails after the Engine part: product file replaced by a directory.
    let product_path = run.path(PRODUCT);
    std::fs::remove_file(&product_path).unwrap();
    std::fs::create_dir_all(product_path.join("blocker")).unwrap();
    let (status, reply) = run
        .put(
            "engine ok, product fails, telemetry skipped",
            json!({ "sync": { "syncEnabled": false }, "general": { "theme": "light", "telemetryEnabled": false } }),
        )
        .await;
    assert_eq!(status, 500, "{reply}");
    assert_eq!(reply["code"], "settings_partially_applied", "{reply}");
    assert_eq!(
        reply["details"],
        json!({ "applied": ["engine"], "failed": ["desktop"], "skipped": ["telemetry"] })
    );
    let settings = run.settings().await;
    assert_eq!(settings["sync"]["syncEnabled"], json!(false), "Engine part stays written");
    assert_eq!(settings["general"]["theme"], json!("dark"), "product memory unchanged");
    assert_eq!(settings["general"]["telemetryEnabled"], telemetry_before);

    // Nothing applied: product-only patch fails as a plain internal error.
    let (status, reply) = run
        .put("product only fails", json!({ "general": { "theme": "light" } }))
        .await;
    assert_eq!((status, reply["code"].clone()), (500, json!("internal_error")), "{reply}");

    // Telemetry fails after Engine and product succeeded.
    std::fs::remove_dir_all(&product_path).unwrap();
    let telemetry_path = run.path(TELEMETRY);
    let _ = std::fs::remove_file(&telemetry_path);
    std::fs::create_dir_all(telemetry_path.join("blocker")).unwrap();
    let (status, reply) = run
        .put(
            "engine and product ok, telemetry fails",
            json!({ "sync": { "syncEnabled": true }, "general": { "theme": "light", "telemetryEnabled": false } }),
        )
        .await;
    assert_eq!(status, 500, "{reply}");
    assert_eq!(
        reply["details"],
        json!({ "applied": ["engine", "desktop"], "failed": ["telemetry"], "skipped": [] })
    );
    let settings = run.settings().await;
    assert_eq!(settings["sync"]["syncEnabled"], json!(true));
    assert_eq!(settings["general"]["theme"], json!("light"));
    assert_eq!(run.product()["general"]["theme"], json!("light"));
    std::fs::remove_dir_all(&telemetry_path).unwrap();
    run.note("completed", Value::Null);
    run.keep();
}

/// B7 (backend), F59, F60: concurrent writes to different owners; analytics
/// consent survives a restart from the product value.
#[tokio::test]
#[ignore]
async fn concurrent_owner_writes_keep_every_field_and_consent_survives_restart() {
    let mut run = Run::start("split-concurrent", &NodeBinarySet::current(), &[]).await;
    let base = run.daemon.base_url();
    let auth = format!("Session {}", run.session);
    let bodies = [
        json!({ "sync": { "syncEnabled": false } }),
        json!({ "general": { "autoDownloadUpdate": false } }),
        json!({ "general": { "theme": "dark" } }),
        json!({ "general": { "usageAnalyticsEnabled": false } }),
    ];
    let mut tasks = Vec::new();
    for body in bodies.clone() {
        let client = run.client.clone();
        let (base, auth) = (base.clone(), auth.clone());
        tasks.push(tokio::spawn(async move {
            client
                .put(format!("{base}/settings"))
                .header("Authorization", auth)
                .json(&body)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), 200);
    }
    run.note("concurrent writes", json!({ "bodies": bodies }));
    let check = |settings: &Value| {
        assert_eq!(settings["sync"]["syncEnabled"], json!(false));
        assert_eq!(settings["general"]["autoDownloadUpdate"], json!(false));
        assert_eq!(settings["general"]["theme"], json!("dark"));
        assert_eq!(settings["general"]["usageAnalyticsEnabled"], json!(false));
    };
    check(&run.settings().await);
    run.restart("restart").await;
    check(&run.settings().await);
    let gate = run.import_lines("analytics gate set from product settings");
    assert!(
        gate.iter().any(|line| line.contains("false")),
        "{gate:#?}"
    );
    run.note("completed", Value::Null);
    run.keep();
}
