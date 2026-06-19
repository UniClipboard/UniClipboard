//! 回归测试：`sync.syncOnRestore` 的 wire ↔ DTO ↔ View ↔ settings.json round-trip。
//!
//! `sync_on_restore` 是 issue #1017 新增的 per-feature 开关。它要穿过 8 层
//! （core model → daemon-contract DTO → webserver projection ×2 → app
//! SettingsView → app SettingsPatch + apply 分支 → TS view → TS patch-builder）
//! 才能从 PATCH 完整 round-trip 回 GET。其中 app 层的 patch-apply 分支
//! （`apply_settings_patch` 里 `sync` 段）历来是「字段被解析却没被 apply」的
//! 静默丢点 —— 本测试锁定 PATCH 进去的值能被 GET 读回，防止任何一层把它丢掉。
//!
//! ## fixture 范围（沿用 `settings_retention_smoke.rs` 模式）
//!
//! 不组装完整 axum Router + AppFacade，改为：
//!   - PUT 模拟：`serde_json::from_str::<SettingsPatchDto>` →
//!     `SettingsPatchDto::into_domain` → `SettingsFacade::update`
//!   - GET 模拟：`SettingsFacade::get` → `SettingsView::into_api_dto`
//!   - 持久化：`Mutex<Settings>` in-memory `SettingsPort`

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use uc_application::facade::settings::SettingsFacade;
use uc_core::ports::SettingsPort;
use uc_core::settings::model::Settings;
use uc_daemon_contract::api::dto::settings::SettingsPatchDto;
use uc_webserver::api::dto::settings::SettingsDto;
use uc_webserver::api::projection::{IntoApiDto, IntoDomain};

struct InMemorySettings {
    settings: Mutex<Settings>,
}

#[async_trait]
impl SettingsPort for InMemorySettings {
    async fn load(&self) -> anyhow::Result<Settings> {
        Ok(self.settings.lock().unwrap().clone())
    }

    async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        *self.settings.lock().unwrap() = settings.clone();
        Ok(())
    }
}

fn build_facade() -> SettingsFacade {
    let port: Arc<dyn SettingsPort> = Arc::new(InMemorySettings {
        settings: Mutex::new(Settings::default()),
    });
    SettingsFacade::new(port)
}

async fn simulate_put(facade: &SettingsFacade, body_json: &str) {
    let payload: SettingsPatchDto = serde_json::from_str(body_json).expect("parse PUT body");
    facade
        .update(payload.into_domain())
        .await
        .expect("settings update");
}

async fn simulate_get(facade: &SettingsFacade) -> Value {
    let view = facade.get().await.expect("settings get");
    let dto: SettingsDto = view.into_api_dto();
    serde_json::to_value(&dto).expect("serialize get")
}

/// Default is `false` (opt-in): a fresh GET must report `syncOnRestore: false`.
#[tokio::test]
async fn sync_on_restore_defaults_to_false_on_wire() {
    let facade = build_facade();
    let get = simulate_get(&facade).await;
    assert_eq!(
        get["sync"]["syncOnRestore"],
        Value::Bool(false),
        "sync_on_restore must default to false (opt-in)"
    );
}

/// The core passthrough test: PATCH `syncOnRestore: true` then GET it back.
/// Exercises daemon-contract DTO → app SettingsPatch → apply branch → view →
/// DTO. If any layer drops the field (notably the app-layer apply branch),
/// the GET reads back `false` and this fails.
#[tokio::test]
async fn sync_on_restore_patch_round_trips() {
    let facade = build_facade();

    let put_body = json!({ "sync": { "syncOnRestore": true } }).to_string();
    simulate_put(&facade, &put_body).await;

    let get = simulate_get(&facade).await;
    assert_eq!(
        get["sync"]["syncOnRestore"],
        Value::Bool(true),
        "syncOnRestore PATCH must round-trip back through GET (no silent drop in the apply branch)"
    );
}

/// A `sync` patch that omits `syncOnRestore` must not clobber the stored value:
/// once turned on, an unrelated `sync` field change (e.g. `autoSync`) leaves it on.
#[tokio::test]
async fn omitting_sync_on_restore_preserves_stored_value() {
    let facade = build_facade();

    // Turn it on.
    simulate_put(
        &facade,
        &json!({ "sync": { "syncOnRestore": true } }).to_string(),
    )
    .await;
    // Patch a sibling field without mentioning syncOnRestore.
    simulate_put(
        &facade,
        &json!({ "sync": { "autoSync": false } }).to_string(),
    )
    .await;

    let get = simulate_get(&facade).await;
    assert_eq!(
        get["sync"]["syncOnRestore"],
        Value::Bool(true),
        "an unrelated sync patch must not reset syncOnRestore (None = leave unchanged)"
    );
    assert_eq!(get["sync"]["autoSync"], Value::Bool(false));
}
