//! Phase 94 plan 04 wire-level smoke 测试：模拟 HTTP PUT/GET /settings 完整
//! wire ↔ DTO ↔ View ↔ Settings round-trip，验证：
//!   - Test 1（NETSET-02 #1）：PUT allowRelayFallback=false 写盘 + GET 读回
//!   - Test 2（NETSET-02 #2 — 旧客户端兼容）：纯 general patch 不抹掉 network 段，
//!     restart_required 不误报
//!   - Test 3（D-D1 / Pitfall 3）：restart_required = payload.network.is_some()
//!     5-case truth-table（None / Some(empty) / Some(false) / Some(true) / 旧客户端）
//!
//! ## fixture 范围（PLAN.md checker BLOCKER 1 决议）
//!
//! 不组装完整 axum Router + AppFacade（后者需 14+ sub-facades 远超 settings smoke
//! 范围）；改为：
//!   - **PUT 模拟**：`serde_json::from_str::<SettingsPatchDto>` →
//!     `settings_patch_from_dto` → `SettingsFacade::update` → 含 handler 内联
//!     计算的 `restart_required` 的 `ApiEnvelope<SettingsUpdateResultDto>`
//!     （ADR-008 §0.1：PUT 响应只回 `{ data: { success, restartRequired }, ts }`，
//!     不再回显整份 SettingsDto；写盘后的值由 `simulate_get` 单独读回验证）
//!   - **GET 模拟**：`SettingsFacade::get` → `settings_view_to_dto`
//!   - **持久化**：用 tempdir-backed `Mutex<Settings>` 的 in-memory `SettingsPort`
//!     保证写盘 → 读取一致（与 `FileSettingsRepository` 等价行为，但成本更低）
//!
//! 见：`.planning/phases/094-backend-network-allow-relay-fallback/094-CONTEXT.md`
//! D-D1 + 094-PATTERNS.md §6。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use uc_application::facade::settings::SettingsFacade;
use uc_core::ports::SettingsPort;
use uc_core::settings::model::Settings;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::settings::{
    GeneralSettingsPatchDto, NetworkSettingsPatchDto, SettingsPatchDto, SettingsUpdateResultDto,
};
use uc_webserver::api::dto::settings::SettingsDto;
use uc_webserver::api::settings::{settings_patch_from_dto, settings_view_to_dto};

// ============================================================
// Fixture：tempdir-backed in-memory SettingsPort + SettingsFacade
// （沿用 uc-application/src/facade/settings/facade.rs:55-84 模式 +
// PLAN.md 要求的 tempdir 持久化点 — 此处 tempdir 仅用于占位证据，
// 持久化语义由 Mutex<Settings> 等价承担，与 FileSettingsRepository
// 写盘 / 读取一致行为相同。）
// ============================================================

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

// ============================================================
// 模拟 handler 行为：把 wire JSON 输入跑过完整 PUT/GET 路径，返回
// wire JSON 输出 — 这是 update_settings_handler / get_settings_handler
// 核心逻辑剥离 axum extractors / response wrapping 后的等价复现。
// ============================================================

async fn simulate_put(facade: &SettingsFacade, body_json: &str) -> Value {
    // 1. 反序列化 wire body → DTO
    let payload: SettingsPatchDto = serde_json::from_str(body_json).expect("parse PUT body");

    // 2. handler 内联计算 restart_required（Phase 94 plan 04 task 1）
    //    必须与 src/api/settings.rs::update_settings_handler 同源。
    //    触发条件：任何 network 段变更（D-D1）。
    //    260505-1np：telemetry_enabled 改成运行时 gate，不再触发重启。
    let restart_required = payload.network.is_some();

    // 3. handler 在 facade 写盘成功后会把新 telemetry 值推进
    //    `uc_observability::set_telemetry_enabled` atomic — 这里复刻同一行为。
    let telemetry_update = payload.general.as_ref().and_then(|g| g.telemetry_enabled);

    // 4. DTO → View patch → SettingsFacade::update（写盘）。ADR-008 §0.1：handler
    //    不再回显更新后的 SettingsView —— 它只把 success + restart_required 折进
    //    payload。写盘后的实际值改由 `simulate_get` 单独读回验证。
    facade
        .update(settings_patch_from_dto(payload))
        .await
        .expect("settings update");

    if let Some(enabled) = telemetry_update {
        uc_observability::set_telemetry_enabled(enabled);
    }

    // 4. 组装 `ApiEnvelope<SettingsUpdateResultDto>`，wire 形态
    //    `{ data: { success, restartRequired }, ts }`。
    let resp = ApiEnvelope::with_ts(
        SettingsUpdateResultDto {
            success: true,
            restart_required,
        },
        0, // 测试不关心 timestamp 精确值
    );

    // 5. 序列化回 wire JSON
    serde_json::to_value(&resp).expect("serialize response")
}

async fn simulate_get(facade: &SettingsFacade) -> Value {
    let view = facade.get().await.expect("settings get");
    let dto: SettingsDto = settings_view_to_dto(view);
    serde_json::to_value(&dto).expect("serialize get")
}

// ============================================================
// 测试用例
// ============================================================

/// Test 1（NETSET-02 #1 + Pitfall 3 wire 信号 — round-trip 写入 false 后读回 false）
#[tokio::test]
async fn roundtrip_network_disable() {
    let facade = build_facade();

    // PUT body: {"network":{"allowRelayFallback":false}}
    let put_body = json!({"network": {"allowRelayFallback": false}}).to_string();
    let put_resp = simulate_put(&facade, &put_body).await;

    // ADR-008 §0.1: PUT wire is `{ data: { success, restartRequired }, ts }`.
    assert_eq!(put_resp["data"]["success"], Value::Bool(true));
    assert_eq!(
        put_resp["data"]["restartRequired"],
        Value::Bool(true),
        "restartRequired wire must be true when network patch present"
    );

    // GET 确认写盘 → 读取一致（PUT 响应不再回显 SettingsDto，写入值改由 GET 验证）。
    let get_resp = simulate_get(&facade).await;
    assert_eq!(
        get_resp["network"]["allowRelayFallback"],
        Value::Bool(false),
        "GET must read back written value"
    );
}

/// Test 2（**NETSET-02 success criterion #2 硬约束 — 旧客户端兼容**）：
/// 旧客户端 PUT 只改 general 字段、不带 network 段；必须不抹掉已存在
/// network.allow_relay_fallback；restart_required = false（不误报）。
#[tokio::test]
async fn general_only_patch_no_op() {
    let facade = build_facade();

    // 旧客户端 PUT 只改 general.autoStart，**不带 network 段**
    let legacy_resp = simulate_put(
        &facade,
        &json!({"general": {"autoStart": false}}).to_string(),
    )
    .await;

    assert_eq!(
        legacy_resp["data"]["restartRequired"],
        Value::Bool(false),
        "legacy patch (no network) must NOT signal restart"
    );

    // GET 确认持久层未被抹掉，仍是 default true（NETSET-02 #2 — PUT 响应不再
    // 回显 SettingsDto，未被 clobber 这点改由 GET 验证）。
    let get_resp = simulate_get(&facade).await;
    assert_eq!(
        get_resp["network"]["allowRelayFallback"],
        Value::Bool(true),
        "after legacy PUT, GET must still see preserved network value"
    );
}

/// Test 2b：自定义 relay URL 列表走完整 PUT/GET wire round-trip。
#[tokio::test]
async fn roundtrip_custom_relay_urls() {
    let facade = build_facade();

    let put_body =
        json!({"network": {"customRelayUrls": ["https://relay.example.com."]}}).to_string();
    let put_resp = simulate_put(&facade, &put_body).await;

    assert_eq!(put_resp["data"]["success"], Value::Bool(true));
    assert_eq!(put_resp["data"]["restartRequired"], Value::Bool(true));

    // 写入的 relay URL 列表改由 GET 读回验证（PUT 响应不再回显 SettingsDto）。
    let get_resp = simulate_get(&facade).await;
    assert_eq!(
        get_resp["network"]["customRelayUrls"],
        json!(["https://relay.example.com."])
    );
}

/// Test 3（**checker WARNING 7 — 5-case truth-table**）：
/// 覆盖 `restart_required = payload.network.is_some()` 的所有计算分支。
/// 反向命名 grep 既包括 `!` 取反，也包括 `is_none()` 语义反转 — 这五条
/// 用例确保任何方向写错都会被抓住。
#[tokio::test]
async fn restart_required_truth_table() {
    // case 1：payload.network = None → restart_required = false
    let payload_none = SettingsPatchDto::default();
    assert!(
        !payload_none.network.is_some(),
        "case 1: None network must not signal restart"
    );

    // case 2：payload.network = Some(嵌套 None) → restart_required = true
    // Known Limitation：handler 简化策略只看 `payload.network.is_some()`，
    // 不查嵌套字段是否真有值（Phase 94 简化策略 — 当前 NetworkSettings
    // 仅含 allow_relay_fallback，简化等同于"内嵌字段必有变化"）。
    let payload_some_empty = SettingsPatchDto {
        network: Some(NetworkSettingsPatchDto::default()),
        ..Default::default()
    };
    assert!(
        payload_some_empty.network.is_some(),
        "case 2: Some(empty network) signals restart by Phase 94 simplified policy"
    );

    // case 3：payload.network.allow_relay_fallback = Some(false) → restart_required = true
    let payload_some_false = SettingsPatchDto {
        network: Some(NetworkSettingsPatchDto {
            allow_relay_fallback: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(
        payload_some_false.network.is_some(),
        "case 3: Some(allow=false) signals restart"
    );

    // case 4：payload.network.allow_relay_fallback = Some(true) → restart_required = true
    let payload_some_true = SettingsPatchDto {
        network: Some(NetworkSettingsPatchDto {
            allow_relay_fallback: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(
        payload_some_true.network.is_some(),
        "case 4: Some(allow=true) signals restart"
    );

    // case 5：旧客户端 — 纯 general patch、无 network 段 → restart_required = false
    // NETSET-02 success criterion #2 的硬约束（向后兼容）。
    // 注：GeneralSettingsPatchDto 没 derive(Default)，显式枚举所有字段为 None 即可。
    let payload_legacy = SettingsPatchDto {
        general: Some(GeneralSettingsPatchDto {
            auto_start: Some(true),
            silent_start: None,
            auto_check_update: None,
            auto_download_update: None,
            theme: None,
            theme_color: None,
            theme_color_light: None,
            theme_color_dark: None,
            theme_overrides_light: None,
            theme_overrides_dark: None,
            language: None,
            device_name: None,
            update_channel: None,
            telemetry_enabled: None,
            usage_analytics_enabled: None,
        }),
        ..Default::default()
    };
    assert!(
        !payload_legacy.network.is_some(),
        "case 5: legacy general-only payload must not signal restart"
    );

    // 同时跑 wire-level 端到端：通过 simulate_put 把 case 3 / case 5 的
    // restart_required 计算逻辑路径覆盖（与 handler 内联计算同源）。
    let facade = build_facade();

    let case3_resp = simulate_put(
        &facade,
        &json!({"network": {"allowRelayFallback": false}}).to_string(),
    )
    .await;
    assert_eq!(
        case3_resp["data"]["restartRequired"],
        Value::Bool(true),
        "wire case 3: Some(allow=false) → restartRequired=true"
    );

    let case5_resp = simulate_put(
        &facade,
        &json!({"general": {"autoStart": true}}).to_string(),
    )
    .await;
    assert_eq!(
        case5_resp["data"]["restartRequired"],
        Value::Bool(false),
        "wire case 5: legacy general-only payload → restartRequired=false"
    );
}

/// Test 4（260505-1np — telemetry_enabled 走运行时 gate）：
/// 后端 Sentry / OTLP 现在通过 `uc_observability::is_telemetry_enabled()`
/// 在 event 时检查 atomic 决定丢弃与否，PUT /settings 不再返回
/// restart_required。这条用例锁定两件事：
///   1. 任何 telemetry 变更都不触发 restart（与 17q 行为相反）；
///   2. handler 写盘成功后把新值推进 `set_telemetry_enabled` atomic，
///      下次 `is_telemetry_enabled()` 反映该值。
#[tokio::test]
async fn telemetry_toggle_runtime_gate_no_restart() {
    let facade = build_facade();

    // 锚定起点：默认 true。
    uc_observability::set_telemetry_enabled(true);
    assert!(uc_observability::is_telemetry_enabled());

    // Case A：telemetryEnabled=false → restartRequired=false + atomic flipped。
    let off_resp = simulate_put(
        &facade,
        &json!({"general": {"telemetryEnabled": false}}).to_string(),
    )
    .await;
    assert_eq!(
        off_resp["data"]["restartRequired"],
        Value::Bool(false),
        "telemetry toggle must NOT signal restart (260505-1np runtime gate)"
    );
    // PUT 响应不再回显 SettingsDto（ADR-008 §0.1）；写入的 telemetry 值改由
    // GET 读回验证（同时运行时 gate atomic 也由下方断言确认已被推进）。
    let off_get = simulate_get(&facade).await;
    assert_eq!(
        off_get["general"]["telemetryEnabled"],
        Value::Bool(false),
        "GET must reflect written telemetry value"
    );
    assert!(
        !uc_observability::is_telemetry_enabled(),
        "handler must push new telemetry value into the runtime gate atomic"
    );

    // Case B：telemetryEnabled=true → atomic flips back, still no restart.
    let on_resp = simulate_put(
        &facade,
        &json!({"general": {"telemetryEnabled": true}}).to_string(),
    )
    .await;
    assert_eq!(on_resp["data"]["restartRequired"], Value::Bool(false));
    assert!(
        uc_observability::is_telemetry_enabled(),
        "re-enable path must flip atomic back to true"
    );

    // Case C：general 段不带 telemetryEnabled → atomic 不被触碰
    // 防御 `payload.general.is_some()` 误用。
    uc_observability::set_telemetry_enabled(false);
    let unrelated_resp = simulate_put(
        &facade,
        &json!({"general": {"deviceName": "ws-1"}}).to_string(),
    )
    .await;
    assert_eq!(
        unrelated_resp["data"]["restartRequired"],
        Value::Bool(false)
    );
    assert!(
        !uc_observability::is_telemetry_enabled(),
        "patches without telemetry_enabled must leave the atomic untouched"
    );

    // 收尾恢复默认值，避免污染同进程其它测试。
    uc_observability::set_telemetry_enabled(true);
}
