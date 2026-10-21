//! Daemon-level checks of the Desktop product settings store (ADR-013).
//!
//! Each test runs a real `uniclipd` on an isolated profile, seeds
//! `desktop-settings.json` before start where needed, and talks to
//! `GET/PUT /settings`. The library-level failure modes are covered by
//! `crates/uc-desktop-settings/tests/store.rs`; these tests prove the daemon
//! constructs the store before serving, injects it, and never lets the Engine
//! see product fields.
//!
//! Set `UC_E2E_ARTIFACT_DIR` to keep a JSON record per test (inputs, file
//! hashes before and after each step, and the daemon's settings log lines).
//!
//! Run with:
//! `cargo build -p uc-daemon -p uc-dev-cli`
//! `cargo test --manifest-path tests/e2e/Cargo.toml --test desktop_settings -- --ignored`

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uc_e2e_tests::{get_session_token, NodeBinarySet, TestDaemon, TestProfile};

const FILE_NAME: &str = "desktop-settings.json";

/// `import.sourceFingerprint` written when there is no legacy `settings.json`:
/// the BLAKE3 of the normalized all-default field set 1 (ADR-013 section 8.2).
const EMPTY_LEGACY_FINGERPRINT: &str = "b6a9ea7e03eee439662d6acbe610f4994508f64e0cbe3880e02721744b72fee9";

struct Context {
    daemon: TestDaemon,
    client: reqwest::Client,
    session: String,
    record: Vec<Value>,
    name: &'static str,
}

impl Context {
    async fn start(name: &'static str, seed: Option<&[u8]>) -> Self {
        let profile = TestProfile::new(name);
        std::fs::create_dir_all(profile.data_dir()).unwrap();
        if let Some(bytes) = seed {
            std::fs::write(profile.data_dir().join(FILE_NAME), bytes).unwrap();
        }
        let daemon = TestDaemon::start_preserving_with(profile, &NodeBinarySet::current(), None)
            .await
            .expect("daemon start");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let session = get_session_token(&daemon, &client).await;
        let mut context = Self {
            daemon,
            client,
            session,
            record: Vec::new(),
            name,
        };
        context.note(
            "started",
            json!({ "seed": seed.map(|bytes| String::from_utf8_lossy(bytes).into_owned()) }),
        );
        context
    }

    fn product_path(&self) -> PathBuf {
        self.daemon.profile.data_dir().join(FILE_NAME)
    }

    fn engine_path(&self) -> PathBuf {
        self.daemon.profile.data_dir().join("settings.json")
    }

    fn product_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.product_path()).ok()
    }

    fn engine_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.engine_path()).ok()
    }

    fn note(&mut self, step: &str, detail: Value) {
        self.record.push(json!({
            "step": step,
            "productSha256": hash(self.product_bytes().as_deref()),
            "engineSha256": hash(self.engine_bytes().as_deref()),
            "detail": detail,
        }));
    }

    async fn tray_icon(&self) -> Value {
        let response = self
            .client
            .get(format!("{}/settings", self.daemon.base_url()))
            .header("Authorization", format!("Session {}", self.session))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        body["data"]["general"]["trayIcon"].clone()
    }

    async fn put(&self, body: Value) -> reqwest::StatusCode {
        self.client
            .put(format!("{}/settings", self.daemon.base_url()))
            .header("Authorization", format!("Session {}", self.session))
            .json(&body)
            .send()
            .await
            .unwrap()
            .status()
    }

    async fn restart(&mut self) {
        self.daemon.restart_preserving().await.expect("restart");
        self.session = get_session_token(&self.daemon, &self.client).await;
    }

    /// Settings-related lines from the daemon's process log and log files.
    fn settings_log_lines(&self) -> Vec<String> {
        let mut sources = vec![self.daemon.profile.process_log_path()];
        if let Ok(entries) = std::fs::read_dir(self.daemon.profile.log_dir()) {
            sources.extend(entries.flatten().map(|entry| entry.path()));
        }
        sources
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .flat_map(|text| {
                text.lines()
                    .filter(|line| line.contains("desktop_settings"))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn keep_artifact(&self) {
        let Ok(dir) = std::env::var("UC_E2E_ARTIFACT_DIR") else {
            return;
        };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let artifact = json!({
            "test": self.name,
            "profile": self.daemon.profile.name,
            "steps": self.record,
            "finalProductFile": self.product_bytes().map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            "settingsLog": self.settings_log_lines(),
        });
        std::fs::write(
            dir.join(format!("{}.json", self.name)),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
    }
}

fn hash(bytes: Option<&[u8]>) -> Value {
    match bytes {
        Some(bytes) => json!(format!("{:x}", Sha256::digest(bytes))),
        None => Value::Null,
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[tokio::test]
#[ignore]
async fn first_start_without_a_file_uses_the_default_and_records_only_the_import_marker() {
    let mut context = Context::start("desktop-settings-fresh", None).await;

    assert_eq!(context.tray_icon().await, json!("glyph"));
    // Since field set 1 (ADR-013 section 8.2) the import records its marker
    // when there is no legacy file; the store load itself still writes nothing.
    assert_eq!(
        read_json(&context.product_path()),
        json!({
            "version": 1,
            "import": { "fieldSet": 1, "sourceFingerprint": EMPTY_LEGACY_FINGERPRINT }
        })
    );
    context.note("read default", json!({ "trayIcon": "glyph" }));

    context.note("completed", Value::Null);
    context.keep_artifact();
}

#[tokio::test]
#[ignore]
async fn corrupt_file_does_not_stop_startup_and_stays_until_an_explicit_write() {
    let corrupt = b"{\"general\": {\"trayIcon\": \"cat\"";
    let mut context = Context::start("desktop-settings-corrupt", Some(corrupt)).await;

    assert_eq!(context.tray_icon().await, json!("glyph"));
    assert_eq!(context.product_bytes().as_deref(), Some(&corrupt[..]));
    context.note("read with corrupt file", json!({ "trayIcon": "glyph" }));

    context.restart().await;
    assert_eq!(context.product_bytes().as_deref(), Some(&corrupt[..]));
    context.note("restart kept corrupt file", Value::Null);

    assert_eq!(
        context
            .put(json!({ "general": { "trayIcon": "cat" } }))
            .await,
        200
    );
    assert_eq!(context.tray_icon().await, json!("cat"));
    assert_eq!(
        read_json(&context.product_path()),
        json!({
            "version": 1,
            "general": { "trayIcon": "cat" },
            "import": { "userWritten": ["general.trayIcon"] }
        })
    );
    context.note("explicit write replaced corrupt file", Value::Null);
    assert!(context
        .settings_log_lines()
        .iter()
        .any(|line| line.contains("desktop_settings_read") && line.contains("malformed_json")));

    context.note("completed", Value::Null);
    context.keep_artifact();
}

#[tokio::test]
#[ignore]
async fn invalid_and_unknown_values_are_kept_and_explicit_writes_persist() {
    let seed = br#"{"version":1,"import":{"fieldSet":0,"futureImport":true},"general":{"trayIcon":"neon","futureGeneral":{"x":1}},"futureSection":{"theme":"aurora"}}"#;
    let mut context = Context::start("desktop-settings-lenient", Some(seed)).await;

    // Invalid stored value: effective default, warning names the field but not
    // the value. Since field set 1 the import (no legacy file here) advances the
    // marker in place; the invalid and unknown values stay (ADR-013 section 8.2).
    assert_eq!(context.tray_icon().await, json!("glyph"));
    assert_eq!(
        read_json(&context.product_path()),
        json!({
            "version": 1,
            "import": { "fieldSet": 1, "futureImport": true, "sourceFingerprint": EMPTY_LEGACY_FINGERPRINT },
            "general": { "trayIcon": "neon", "futureGeneral": { "x": 1 } },
            "futureSection": { "theme": "aurora" }
        })
    );
    let seeded = context.product_bytes();
    let warnings: Vec<String> = context
        .settings_log_lines()
        .into_iter()
        .filter(|line| line.contains("desktop_settings_invalid_field"))
        .collect();
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("general.trayIcon")),
        "{warnings:?}"
    );
    assert!(warnings.iter().all(|line| !line.contains("neon")));
    context.note("read invalid value", json!({ "trayIcon": "glyph" }));

    // An Engine-only write first, so the Engine file exists for comparison.
    assert_eq!(
        context
            .put(json!({ "sync": { "syncOnRestore": true } }))
            .await,
        200
    );
    let engine_before = context.engine_bytes();
    assert!(engine_before.is_some(), "Engine settings file missing");
    assert_eq!(context.product_bytes(), seeded);
    context.note("engine-only write", Value::Null);

    // An invalid write is refused and changes nothing.
    let status = context
        .put(json!({ "general": { "trayIcon": "bogus" } }))
        .await;
    assert!(status.is_client_error(), "{status}");
    assert_eq!(context.product_bytes(), seeded);
    assert_eq!(context.engine_bytes(), engine_before);
    context.note(
        "invalid write refused",
        json!({ "status": status.as_u16() }),
    );

    // A product-only write reaches the product file and never the Engine file.
    assert_eq!(
        context
            .put(json!({ "general": { "trayIcon": "cat" } }))
            .await,
        200
    );
    assert_eq!(context.tray_icon().await, json!("cat"));
    assert_eq!(
        context.engine_bytes(),
        engine_before,
        "Engine settings changed"
    );
    let expected = json!({
        "version": 1,
        "import": {
            "fieldSet": 1,
            "futureImport": true,
            "sourceFingerprint": EMPTY_LEGACY_FINGERPRINT,
            "userWritten": ["general.trayIcon"]
        },
        "general": { "trayIcon": "cat", "futureGeneral": { "x": 1 } },
        "futureSection": { "theme": "aurora" }
    });
    assert_eq!(read_json(&context.product_path()), expected);
    context.note("explicit write", json!({ "trayIcon": "cat" }));

    // Same value: the file is not rewritten.
    let written = context.product_bytes();
    let modified = std::fs::metadata(context.product_path())
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        context
            .put(json!({ "general": { "trayIcon": "cat" } }))
            .await,
        200
    );
    assert_eq!(context.product_bytes(), written);
    assert_eq!(
        std::fs::metadata(context.product_path())
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    context.note("same-value write", Value::Null);

    // Restart (the process is killed): the value and unknown keys persist.
    context.restart().await;
    assert_eq!(context.tray_icon().await, json!("cat"));
    assert_eq!(read_json(&context.product_path()), expected);
    context.note("after restart", json!({ "trayIcon": "cat" }));

    context.note("completed", Value::Null);
    context.keep_artifact();
}
