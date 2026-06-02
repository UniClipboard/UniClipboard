//! 回归测试：retention rules 的 wire ↔ DTO ↔ View ↔ settings.json round-trip。
//!
//! 修复 issue #606 —— `RetentionRuleDto` 之前只在枚举上声明 `rename_all = "camelCase"`，
//! 这只 rename 变体名（`ByAge` → `byAge`），不会改写 struct 变体内部字段名。
//! 结果 wire 是 `{"byAge":{"max_age":N}}`、前端发 `{"byAge":{"maxAge":N}}`，
//! Axum `Json` 提取器反序列化失败返回 422，UI 显示
//! "保存设置失败: DaemonApiError: 422 on /settings"。
//!
//! 这里锁定五件事，防止再次回归：
//!   1. **`byAge` 序列化字段是 `maxAge`**（不是 `max_age`）；
//!   2. **`byCount` 序列化字段是 `maxItems`**；
//!   3. **`byContentType` / `byTotalSize` / `sensitive` 同样走 camelCase**；
//!   4. **GET → PUT round-trip 一致**：把 GET 的 retention rules 原样 PUT 回去
//!      必须 200 OK、不丢字段、不返回 422；
//!   5. **handler 反序列化前端实际发送的 wire body**：模仿
//!      `StorageSection.setByAgeRule / setByCountRule` 构造的 patch 形态。
//!
//! ## fixture 范围（沿用 `settings_network_smoke.rs` 模式）
//!
//! 不组装完整 axum Router + AppFacade，改为：
//!   - PUT 模拟：`serde_json::from_str::<SettingsPatchDto>` →
//!     `settings_patch_from_dto` → `SettingsFacade::update` → `settings_view_to_dto`
//!   - GET 模拟：`SettingsFacade::get` → `settings_view_to_dto`
//!   - 持久化：`Mutex<Settings>` in-memory `SettingsPort`

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use uc_application::facade::settings::SettingsFacade;
use uc_core::ports::SettingsPort;
use uc_core::settings::model::Settings;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::settings::{
    RetentionRuleDto, SettingsPatchDto, SettingsUpdateResultDto,
};
use uc_webserver::api::dto::settings::SettingsDto;
use uc_webserver::api::settings::{settings_patch_from_dto, settings_view_to_dto};

// ============================================================
// Fixture
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

async fn simulate_put(facade: &SettingsFacade, body_json: &str) -> Value {
    let payload: SettingsPatchDto = serde_json::from_str(body_json).expect("parse PUT body");
    let restart_required = payload.network.is_some();
    // ADR-008 §0.1: handler writes the patch then returns only
    // `{ data: { success, restartRequired }, ts }` — the updated SettingsView is
    // no longer echoed on the wire. Written values are verified via `simulate_get`.
    facade
        .update(settings_patch_from_dto(payload))
        .await
        .expect("settings update");
    let resp = ApiEnvelope::with_ts(
        SettingsUpdateResultDto {
            success: true,
            restart_required,
        },
        0,
    );
    serde_json::to_value(&resp).expect("serialize response")
}

async fn simulate_get(facade: &SettingsFacade) -> Value {
    let view = facade.get().await.expect("settings get");
    let dto: SettingsDto = settings_view_to_dto(view);
    serde_json::to_value(&dto).expect("serialize get")
}

// ============================================================
// 1. 各变体 wire 形态锁定（防 rename_all_fields 被回退）
// ============================================================

/// `RetentionRuleDto::ByAge` 的 wire 形态必须是 `{"byAge":{"maxAge":N}}`，
/// 而不是 `{"byAge":{"max_age":N}}` —— 后者是 issue #606 出错的旧形态。
#[test]
fn by_age_wire_is_camel_case() {
    let rule = RetentionRuleDto::ByAge {
        max_age: std::time::Duration::from_secs(2_592_000),
    };
    let wire = serde_json::to_value(&rule).expect("serialize ByAge");
    assert_eq!(
        wire,
        json!({"byAge": {"maxAge": 2_592_000}}),
        "wire MUST be camelCase inside variant (issue #606 regression guard)"
    );

    // 反向：camelCase 输入必须能解析；snake_case 输入必须失败（避免后端
    // 偷偷接受了 snake_case 让 mismatched 前端蒙混过关）。
    let parsed: RetentionRuleDto = serde_json::from_value(json!({"byAge": {"maxAge": 86400}}))
        .expect("must accept camelCase wire");
    match parsed {
        RetentionRuleDto::ByAge { max_age } => assert_eq!(max_age.as_secs(), 86400),
        other => panic!("unexpected variant: {:?}", other),
    }

    assert!(
        serde_json::from_value::<RetentionRuleDto>(json!({"byAge": {"max_age": 86400}})).is_err(),
        "snake_case field name on wire must be rejected — that's the bug-shape"
    );
}

/// `byCount` 的 wire 形态必须是 `{"byCount":{"maxItems":N}}`。
#[test]
fn by_count_wire_is_camel_case() {
    let rule = RetentionRuleDto::ByCount { max_items: 500 };
    let wire = serde_json::to_value(&rule).expect("serialize ByCount");
    assert_eq!(wire, json!({"byCount": {"maxItems": 500}}));

    let parsed: RetentionRuleDto = serde_json::from_value(json!({"byCount": {"maxItems": 1000}}))
        .expect("must accept camelCase wire");
    match parsed {
        RetentionRuleDto::ByCount { max_items } => assert_eq!(max_items, 1000),
        other => panic!("unexpected variant: {:?}", other),
    }
}

/// `byContentType` 的 wire 形态必须是 `{"byContentType":{"contentType":{...}, "maxAge":N}}`。
#[test]
fn by_content_type_wire_is_camel_case() {
    use uc_daemon_contract::api::dto::settings::ContentTypesDto;

    let rule = RetentionRuleDto::ByContentType {
        content_type: ContentTypesDto {
            text: true,
            image: false,
            link: false,
            file: false,
            code_snippet: false,
            rich_text: false,
        },
        max_age: std::time::Duration::from_secs(86_400),
    };
    let wire = serde_json::to_value(&rule).expect("serialize ByContentType");
    let expected = json!({
        "byContentType": {
            "contentType": {
                "text": true,
                "image": false,
                "link": false,
                "file": false,
                "codeSnippet": false,
                "richText": false,
            },
            "maxAge": 86_400,
        }
    });
    assert_eq!(wire, expected);
}

/// `byTotalSize` / `sensitive` 一同锁定 —— 单字段变体也走 camelCase。
#[test]
fn by_total_size_and_sensitive_wire_camel_case() {
    let by_size = RetentionRuleDto::ByTotalSize {
        max_bytes: 1_073_741_824,
    };
    assert_eq!(
        serde_json::to_value(&by_size).unwrap(),
        json!({"byTotalSize": {"maxBytes": 1_073_741_824u64}})
    );

    let sensitive = RetentionRuleDto::Sensitive {
        max_age: std::time::Duration::from_secs(3600),
    };
    assert_eq!(
        serde_json::to_value(&sensitive).unwrap(),
        json!({"sensitive": {"maxAge": 3600}})
    );
}

// ============================================================
// 2. PUT /settings 端到端 —— 模仿前端 `StorageSection` 实际发送的 patch
// ============================================================

/// 复现 issue #606 触发路径：前端改"历史保留时间"调 `updateRetentionPolicy({rules: [...] })`，
/// 最终 PUT body 是 `{"retentionPolicy":{"rules":[{"byAge":{"maxAge":...}}, ...], ...}}`。
/// 修复前这一步 Axum 会 422 拒掉；修复后必须 200 OK 并写盘 round-trip 成功。
#[tokio::test]
async fn put_retention_rules_camelcase_round_trips() {
    let facade = build_facade();

    // 前端 `setByAgeRule(rules, 60)` + `setByCountRule(rules, 1000)` 拼出的 patch。
    let put_body = json!({
        "retentionPolicy": {
            "enabled": true,
            "rules": [
                { "byAge": { "maxAge": 60 * 86_400 } },
                { "byCount": { "maxItems": 1000 } },
            ],
            "skipPinned": true,
            "evaluation": "anyMatch",
        }
    })
    .to_string();

    let put_resp = simulate_put(&facade, &put_body).await;
    // ADR-008 §0.1: PUT wire is `{ data: { success, restartRequired }, ts }`.
    assert_eq!(
        put_resp["data"]["success"],
        Value::Bool(true),
        "PUT /settings must succeed with camelCase retention rules (issue #606)"
    );

    // PUT 响应不再回显 SettingsDto（ADR-008 §0.1）；写入的两条规则改由 GET 读回
    // 验证 —— 既确认写盘成功，也锁定 wire 字段名仍是 camelCase（issue #606 回归点）。
    let get_resp = simulate_get(&facade).await;
    let get_rules = get_resp["retentionPolicy"]["rules"]
        .as_array()
        .expect("rules array on GET");
    assert_eq!(get_rules.len(), 2);
    assert_eq!(get_rules[0]["byAge"]["maxAge"], json!(60 * 86_400));
    assert_eq!(get_rules[1]["byCount"]["maxItems"], json!(1000));
}

/// 把 GET 拿到的 retention rules 原封不动 PUT 回去（前端 `SettingContext.saveSetting`
/// 的典型路径：先 GET 整份 settings，patch 单个字段，再把整份 PUT 上去）。
/// 必须 200 OK —— 这是用户在 UI 上拨一下"自动清理"开关就触发的最常见路径。
#[tokio::test]
async fn get_then_put_full_retention_section_succeeds() {
    let facade = build_facade();

    // 先种入一份带规则的 settings（模拟用户已存在的配置）。
    let seed = json!({
        "retentionPolicy": {
            "enabled": true,
            "rules": [
                { "byAge": { "maxAge": 14 * 86_400 } },
                { "byCount": { "maxItems": 200 } },
            ],
            "skipPinned": true,
            "evaluation": "anyMatch",
        }
    });
    let _ = simulate_put(&facade, &seed.to_string()).await;

    // GET 取出整份 retentionPolicy。
    let snapshot = simulate_get(&facade).await;
    let retention = &snapshot["retentionPolicy"];
    assert!(
        retention.is_object(),
        "retentionPolicy must be present on GET"
    );

    // 把 GET 拿到的 retentionPolicy 原样塞进 PUT body —— 这正是 SettingContext
    // 在用户拨"自动清理"开关时构造 patch 的方式。
    let full_put = json!({ "retentionPolicy": retention }).to_string();
    let resp = simulate_put(&facade, &full_put).await;
    // ADR-008 §0.1: PUT wire is `{ data: { success, restartRequired }, ts }`.
    assert_eq!(
        resp["data"]["success"],
        Value::Bool(true),
        "round-trip GET → PUT of the full retentionPolicy must succeed"
    );

    // PUT 响应不再回显 SettingsDto；round-trip 后的 rules 改由 GET 读回验证。
    let after = simulate_get(&facade).await;
    assert_eq!(
        after["retentionPolicy"]["rules"][0]["byAge"]["maxAge"],
        json!(14 * 86_400)
    );
    assert_eq!(
        after["retentionPolicy"]["rules"][1]["byCount"]["maxItems"],
        json!(200)
    );
}

/// 防御回归：用 issue #606 出错时的旧 wire 形态（snake_case 内部字段）PUT 必须失败。
/// 这条用例确保如果有人误删 `rename_all_fields` 退回旧行为，CI 会立刻报错。
#[test]
fn snake_case_inside_variant_is_rejected_at_wire() {
    // 旧形态 wire（修复前 daemon 实际发出的 GET 数据形态、也是被 422 拒掉的形态）
    let buggy_body = r#"{
        "retentionPolicy": {
            "rules": [{"byAge": {"max_age": 86400}}]
        }
    }"#;

    let result: Result<SettingsPatchDto, _> = serde_json::from_str(buggy_body);
    assert!(
        result.is_err(),
        "snake_case field inside variant must NOT deserialize — that was the bug shape (issue #606)"
    );
}
