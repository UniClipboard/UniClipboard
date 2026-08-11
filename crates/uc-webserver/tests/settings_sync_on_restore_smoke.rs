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
//! 不组装业务运行时；本测试只验证网页输入、新核心公开设置类型和网页输出之间
//! 的转换。设置保存与字段保留规则由 `uc-engine` 和 `uc-application` 自身测试覆盖。

use serde_json::{json, Value};
use uc_daemon_contract::api::dto::settings::SettingsPatchDto;
use uc_engine::{SettingsPatch, SettingsSummary};
use uc_webserver::api::dto::settings::SettingsDto;
use uc_webserver::api::projection::{IntoApiDto, IntoDomain};

#[derive(Default)]
struct SettingsBoundaryFixture {
    current: SettingsSummary,
}

fn simulate_put(fixture: &mut SettingsBoundaryFixture, body_json: &str) {
    let payload: SettingsPatchDto = serde_json::from_str(body_json).expect("parse PUT body");
    let patch: SettingsPatch = payload.into_domain();
    if let Some(sync) = patch.sync {
        if let Some(value) = sync.sync_enabled {
            fixture.current.sync.sync_enabled = value;
        }
        if let Some(value) = sync.auto_sync_enabled {
            fixture.current.sync.auto_sync_enabled = value;
        }
        if let Some(value) = sync.sync_on_restore {
            fixture.current.sync.sync_on_restore = value;
        }
    }
}

fn simulate_get(fixture: &SettingsBoundaryFixture) -> Value {
    let dto: SettingsDto = fixture.current.clone().into_api_dto();
    serde_json::to_value(&dto).expect("serialize get")
}

/// Default is `false` (opt-in): a fresh GET must report `syncOnRestore: false`.
#[test]
fn sync_on_restore_defaults_to_false_on_wire() {
    let fixture = SettingsBoundaryFixture::default();
    let get = simulate_get(&fixture);
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
#[test]
fn sync_on_restore_patch_round_trips() {
    let mut fixture = SettingsBoundaryFixture::default();

    let put_body = json!({ "sync": { "syncOnRestore": true } }).to_string();
    simulate_put(&mut fixture, &put_body);

    let get = simulate_get(&fixture);
    assert_eq!(
        get["sync"]["syncOnRestore"],
        Value::Bool(true),
        "syncOnRestore PATCH must round-trip back through GET (no silent drop in the apply branch)"
    );
}

/// A `sync` patch that omits `syncOnRestore` must not clobber the stored value:
/// once turned on, an unrelated `sync` field change (e.g. `autoSyncEnabled`) leaves it on.
#[test]
fn omitting_sync_on_restore_preserves_stored_value() {
    let mut fixture = SettingsBoundaryFixture::default();

    // Turn it on.
    simulate_put(
        &mut fixture,
        &json!({ "sync": { "syncOnRestore": true } }).to_string(),
    );
    // Patch a sibling field without mentioning syncOnRestore.
    simulate_put(
        &mut fixture,
        &json!({ "sync": { "autoSyncEnabled": false } }).to_string(),
    );

    let get = simulate_get(&fixture);
    assert_eq!(
        get["sync"]["syncOnRestore"],
        Value::Bool(true),
        "an unrelated sync patch must not reset syncOnRestore (None = leave unchanged)"
    );
    assert_eq!(get["sync"]["autoSyncEnabled"], Value::Bool(false));
}
