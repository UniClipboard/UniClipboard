//! `PosthogSink` —— release 构建专用，把事件 POST 到 PostHog Cloud capture endpoint。
//!
//! ## 范围
//!
//! Slice 7b-1：依赖图、`PosthogSink` struct、构造器与 `AnalyticsPort` 占位 impl。
//! Slice 7b-2（本 slice）：`build_capture_body` 纯 fn + `capture` 实 wire +
//! warn 节流 + tokio::spawn fire-and-forget HTTP。
//! Slice 7b-3：`build_analytics_sink` key 注入 + 缺 key 降级。
//!
//! ## 为什么自写 reqwest 0.12 client，不用 `posthog-rs` SDK
//!
//! - `posthog-rs 0.7` 的 Cargo.toml 把 `reqwest = "0.13.2"` + `features = ["rustls"]`
//!   写死，而 reqwest 0.13 的 rustls feature 隐式选 `aws-lc-rs`（C 库 + CMake 编译）。
//!   这与 uc-cli musl 静态编译"零 C 工具链"硬约束冲突——sentry 已为此用 ureq 而非
//!   reqwest 0.13（见 `uc-bootstrap/Cargo.toml` 注释）。
//! - cargo features unification 是 workspace 级 union，`optional` / feature gate
//!   无法把 uc-cli 排除出依赖图。
//! - PostHog capture endpoint 极简（单条 POST + JSON body），自写 ~100 行成本远低
//!   于把 SDK 拖进依赖；失去 batching + retry 的代价 < 1% 事件丢失，schema doc
//!   §10 已允许。
//!
//! ## 字段冲突 invariant
//!
//! [`super::build_event_payload`] 输出顶层带 `event` / `distinct_id`，而 PostHog
//! capture 要求把这两键放外层、`properties` 对象里**不得**重复出现 `distinct_id`
//! （否则服务端会触发 distinct_id property collision 警告）。
//! [`build_capture_body`] 把这两键移出 properties，单测 `build_capture_body_*`
//! 守住此 invariant。
//!
//! ## 进程退出语义
//!
//! `capture` 内 `tokio::spawn` 出独立 task 调 `reqwest::Client::post(...).await`，
//! 调用方 zero-await。task 一旦 spawn 就走自己的网络生命周期；进程立刻退出时未
//! send 完成的 task 会被 runtime 中断——schema doc §10 已允许 < 1% 丢失。
//! 后续若发现 `app_first_open` 等 onboarding 起点丢失率高，再补 `tauri::App::on_exit`
//! 钩子做 best-effort drain。

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Map, Value};
use tracing::warn;

use super::super::context::global_event_context;
use super::super::events::Event;
use super::super::port::{AnalyticsPort, GroupIdentifyPayload, IdentifyPayload};
use super::build_event_payload;
use super::stdout::TRACE_TARGET;

/// PostHog Cloud US capture endpoint。schema doc §10 选 US ingestion region；
/// 切 EU 或 self-host 都只是替换此 URL（Slice 11+ 范围），wire 形态零改动。
pub const POSTHOG_US_CAPTURE_ENDPOINT: &str = "https://us.i.posthog.com/i/v0/e/";

/// 把事件 POST 到 PostHog Cloud 的 release sink。
///
/// 调用方应在构造前确认 [`crate::analytics_gate::is_analytics_enabled`]
/// 为 `true`——本 sink 不重复检查 gate（schema doc §6.4 约定 gate 责任在
/// 调用方，由 [`super::GatedAnalyticsSink`] 统一守住）。
///
/// `warned_missing_context` 与 `StdoutSink` 同款语义：context 缺失分支
/// 一次/sink 实例 warn，避免启动早期日志 spam。
#[derive(Debug)]
pub struct PosthogSink {
    client: reqwest::Client,
    api_key: String,
    endpoint: String,
    warned_missing_context: AtomicBool,
}

impl PosthogSink {
    /// 用默认 US ingestion endpoint 构造。生产路径调用 `new(api_key)` 即可。
    pub fn new(api_key: String) -> Self {
        Self::with_endpoint(api_key, POSTHOG_US_CAPTURE_ENDPOINT.to_string())
    }

    /// 自定义 endpoint 入口。两种用途：
    /// - 单测起 `wiremock` 本地 mock server 时把 endpoint 指过去；
    /// - 未来 self-host PostHog 实例时由 `build_analytics_sink` 注入。
    pub fn with_endpoint(api_key: String, endpoint: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            endpoint,
            warned_missing_context: AtomicBool::new(false),
        }
    }

    fn warn_missing_context_once(&self, event_name: &'static str) {
        if !self.warned_missing_context.swap(true, Ordering::Relaxed) {
            warn!(
                target: TRACE_TARGET,
                event = event_name,
                "global EventContext not yet set; dropping analytics event (logged once per sink)"
            );
        }
    }
}

impl AnalyticsPort for PosthogSink {
    fn capture(&self, event: Event) {
        let Some(ctx) = global_event_context() else {
            self.warn_missing_context_once(event.name());
            return;
        };
        let payload = build_event_payload(&event, &ctx);
        let body = build_capture_body(event.name(), payload, &self.api_key);

        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        let event_name = event.name(); // &'static str，跨线程安全
        tokio::spawn(async move {
            match client.post(&endpoint).json(&body).send().await {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => warn!(
                    target: TRACE_TARGET,
                    event = event_name,
                    status = %resp.status(),
                    "posthog capture non-2xx"
                ),
                Err(err) => warn!(
                    target: TRACE_TARGET,
                    event = event_name,
                    error = %err,
                    "posthog capture failed"
                ),
            }
        });
    }

    fn identify(&self, payload: IdentifyPayload) {
        // $identify 不依赖 EventContext —— person 合并字段全部由 payload 自带。
        // 与 capture 一致走 fire-and-forget tokio::spawn。
        let body = build_identify_body(&payload, &self.api_key);

        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move {
            match client.post(&endpoint).json(&body).send().await {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => warn!(
                    target: TRACE_TARGET,
                    event = "$identify",
                    status = %resp.status(),
                    "posthog identify non-2xx"
                ),
                Err(err) => warn!(
                    target: TRACE_TARGET,
                    event = "$identify",
                    error = %err,
                    "posthog identify failed"
                ),
            }
        });
    }

    fn group_identify(&self, payload: GroupIdentifyPayload) {
        // $groupidentify 需要 distinct_id（PostHog 用 distinct_id 把 group
        // event 归属到一个 person）。从 global EventContext 取当前 distinct_id —
        // 与 capture 同一逻辑，保证 person 合并语义。
        let Some(ctx) = global_event_context() else {
            self.warn_missing_context_once("$groupidentify");
            return;
        };
        let distinct_id = ctx.analytics_person_id.as_uuid().to_string();
        let body = build_group_identify_body(&payload, distinct_id, &self.api_key);

        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move {
            match client.post(&endpoint).json(&body).send().await {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => warn!(
                    target: TRACE_TARGET,
                    event = "$groupidentify",
                    status = %resp.status(),
                    "posthog group_identify non-2xx"
                ),
                Err(err) => warn!(
                    target: TRACE_TARGET,
                    event = "$groupidentify",
                    error = %err,
                    "posthog group_identify failed"
                ),
            }
        });
    }
}

/// `$lib` 取值：自写 client 自报身份，便于 PostHog 控制台按来源过滤。
const POSTHOG_LIB_NAME: &str = "uniclipboard-rust";

/// `$device_type` 固定为 PostHog 标准枚举的 `"Desktop"`。
///
/// PostHog 标准取值集合（Web SDK 推断 UA 时落到这三个值之一）：
/// `Mobile` / `Tablet` / `Desktop`。uniclipboard 是桌面 App——只可能跑在
/// macOS / Windows / Linux 桌面环境，固定 `Desktop` 让 PostHog 内置
/// "Device type" breakdown 直接可用，无需在 PostHog 端配置自定义映射。
///
/// 注意：iOS Shortcut / Android 客户端只是发请求过来的"对端"——uniclipboard
/// 桌面 daemon 本身仍是 desktop，对端 OS 走 event property `peer_os`（见
/// `events.rs::PairingSucceeded`），不影响本字段。
const POSTHOG_DEVICE_TYPE_DESKTOP: &str = "Desktop";

/// 把 [`build_event_payload`] 的产物转成 PostHog capture endpoint 的请求 body。
///
/// 输出 wire 形态（顶层）：
///
/// ```json
/// {
///   "api_key": "phc_xxx",
///   "event": "<event name>",
///   "distinct_id": "<anonymous_user_id>",
///   "properties": {
///     <context + event-specific 字段，不含 event / distinct_id>,
///     "$device_id": "<analytics_device_id>",
///     "$session_id": "<session_id>",
///     "$lib": "uniclipboard-rust",
///     "$lib_version": "<app_version>",
///     "$geoip_disable": true,
///     "$set": { <person property 当前快照> },
///     "$set_once": { <person 首次出现时写入的安装期不变量> }
///   },
///   "timestamp": "2026-05-09T12:34:56.789+00:00"
/// }
/// ```
///
/// `event` / `distinct_id` 必须从 properties 移出——两键由 [`build_event_payload`]
/// 在顶层平铺，PostHog 服务端会用顶层值做漏斗主键；若 properties 也保留 `distinct_id`
/// 服务端会触发 property collision 警告（dashboard 上漏斗折线会出现重复 series）。
///
/// `$`-prefix 字段是 PostHog 服务端识别的标准 property —— 解锁三类能力：
/// - `$device_id` / `$session_id`：Person ↔ Device / Session funnel & Replay。
/// - `$lib` / `$lib_version`：控制台按来源过滤客户端流量。
/// - `$set` / `$set_once`：把 EventContext 中"用户级"字段路由到 Person Properties，
///   控制台按 person 切片可用，且 person 维度不会被每事件重写覆盖。
/// - `$geoip_disable`：兜底执行 schema doc §6.1"客户端原始 IP 永不上传"隐私
///   契约——拒绝服务端按请求 IP 反推 `$geoip_country` / `$geoip_city`。
///
/// 重要：本函数**只在 PostHog 适配器内**注入这些字段。[`build_event_payload`]
/// 仍保持 vendor-neutral（schema doc §4 / §10.1 单一真相源约定）；将来切
/// self-host PostHog / 别的后端只动这里。
fn build_capture_body(event_name: &str, mut payload: Map<String, Value>, api_key: &str) -> Value {
    let distinct_id = payload
        .remove("distinct_id")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    payload.remove("event");

    inject_posthog_standard_fields(&mut payload);

    let mut body = Map::new();
    body.insert("api_key".into(), Value::String(api_key.to_string()));
    body.insert("event".into(), Value::String(event_name.to_string()));
    body.insert("distinct_id".into(), Value::String(distinct_id));
    body.insert("properties".into(), Value::Object(payload));
    body.insert(
        "timestamp".into(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    Value::Object(body)
}

/// 把 vendor-neutral 的 properties map 翻译成 PostHog 标准 wire 形态。
///
/// 字段来源全部从 `payload` 里现取（`build_event_payload` 已把 EventContext
/// 平铺过来），保持函数纯净、易测试。原 flat 字段同时保留——schema doc §4
/// 仍是 wire 契约，删字段会破坏向后兼容；新增 `$`-prefix 字段是非破坏性扩展。
fn inject_posthog_standard_fields(payload: &mut Map<String, Value>) {
    if let Some(v) = payload.get("analytics_device_id").cloned() {
        payload.insert("$device_id".into(), v);
    }
    if let Some(v) = payload.get("session_id").cloned() {
        payload.insert("$session_id".into(), v);
    }
    if let Some(v) = payload.get("app_version").cloned() {
        payload.insert("$lib_version".into(), v);
    }
    // PostHog 内置 dashboard 的"Top OS" / "OS version" / "Device type"
    // breakdown 读的是 `$os` / `$os_version` / `$device_type` 标准 property，
    // 而非自定义命名的 `os`。同时把 `os` flat 字段保留（schema doc §4 仍是
    // wire 契约），双写让 vendor-neutral 字段集不破坏。
    if let Some(v) = payload.get("os").cloned() {
        payload.insert("$os".into(), v);
    }
    if let Some(v) = payload.get("os_version").cloned() {
        payload.insert("$os_version".into(), v);
    }
    payload.insert(
        "$device_type".into(),
        Value::String(POSTHOG_DEVICE_TYPE_DESKTOP.into()),
    );
    payload.insert("$lib".into(), Value::String(POSTHOG_LIB_NAME.into()));
    payload.insert("$geoip_disable".into(), Value::Bool(true));

    payload.insert("$set".into(), Value::Object(build_set_snapshot(payload)));
    payload.insert(
        "$set_once".into(),
        Value::Object(build_set_once_initial(payload)),
    );

    // Phase 098 · v2 跨设备 person 聚合：把 space_id_hash 转成 PostHog
    // group analytics 的 `$groups`，让控制台同时按 person 与 group (Space)
    // 维度切片留存（schema doc §3.4）。Solo 状态下 `space_id_hash` 是
    // None → 不出现 `$groups` 字段，避免 PostHog 把 null 当显式清空。
    if let Some(Value::String(hash)) = payload.get("space_id_hash") {
        if !hash.is_empty() {
            let mut groups = Map::new();
            groups.insert("space".into(), Value::String(hash.clone()));
            payload.insert("$groups".into(), Value::Object(groups));
        }
    }
}

/// 当前快照写入 Person Property：每条事件覆盖，控制台按 person 切片直接可用。
///
/// 选取的字段都是"可变的当前状态"——版本、平台、locale、active_device_count 等。
/// PostHog 端 person profile 永远反映最近一条事件的快照。
fn build_set_snapshot(payload: &Map<String, Value>) -> Map<String, Value> {
    const SET_KEYS: &[&str] = &[
        "app_version",
        "app_channel",
        "os",
        "os_version",
        "arch",
        "locale",
        "timezone",
        "active_device_count",
        "space_id_hash",
    ];
    let mut out = Map::new();
    for k in SET_KEYS {
        if let Some(v) = payload.get(*k).cloned() {
            out.insert((*k).into(), v);
        }
    }
    out
}

/// 把 [`IdentifyPayload`] 翻译成 PostHog `$identify` 的请求 body。
///
/// 输出 wire 形态（顶层）：
///
/// ```json
/// {
///   "api_key": "phc_xxx",
///   "event": "$identify",
///   "distinct_id": "<new_distinct_id>",
///   "properties": {
///     "$anon_distinct_id": "<old_distinct_id>",
///     "$lib": "uniclipboard-rust",
///     "$geoip_disable": true,
///     "$set":      { ... },          // 可选：payload.set 非空时出现
///     "$set_once": { ... }           // 可选：payload.set_once 非空时出现
///   },
///   "timestamp": "..."
/// }
/// ```
///
/// 关键约束：
/// - `$anon_distinct_id` 必须放在 `properties` 内、**不在顶层**——这是 PostHog
///   alias 合并协议的硬要求。顶层 `distinct_id` 是 new；老 anonymous person
///   通过 `$anon_distinct_id` 与之合并。
/// - `$lib` / `$geoip_disable` 与 capture 保持一致——身份合并事件也算客户端
///   流量，dashboard 按 `$lib` 过滤、`$geoip_disable` 兜底执行 §6.1 IP 不上传。
/// - `$set` / `$set_once` **仅在调用方显式提供**时出现；空 map 时不进 wire，
///   避免 PostHog 把"空对象"误解为"清空 person property"。
fn build_identify_body(payload: &IdentifyPayload, api_key: &str) -> Value {
    let mut props = Map::new();
    props.insert(
        "$anon_distinct_id".into(),
        Value::String(payload.old_distinct_id.to_string()),
    );
    props.insert("$lib".into(), Value::String(POSTHOG_LIB_NAME.into()));
    props.insert("$geoip_disable".into(), Value::Bool(true));
    if !payload.set.is_empty() {
        props.insert("$set".into(), Value::Object(payload.set.clone()));
    }
    if !payload.set_once.is_empty() {
        props.insert("$set_once".into(), Value::Object(payload.set_once.clone()));
    }

    let mut body = Map::new();
    body.insert("api_key".into(), Value::String(api_key.to_string()));
    body.insert("event".into(), Value::String("$identify".into()));
    body.insert(
        "distinct_id".into(),
        Value::String(payload.new_distinct_id.to_string()),
    );
    body.insert("properties".into(), Value::Object(props));
    body.insert(
        "timestamp".into(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    Value::Object(body)
}

/// 把 [`GroupIdentifyPayload`] 翻译成 PostHog `$groupidentify` 的请求 body。
///
/// 输出 wire 形态：
///
/// ```json
/// {
///   "api_key": "phc_xxx",
///   "event": "$groupidentify",
///   "distinct_id": "<distinct_id>",
///   "properties": {
///     "$group_type": "<group_type>",
///     "$group_key":  "<group_key>",
///     "$group_set":  { ...payload.set... },
///     "$lib": "uniclipboard-rust",
///     "$geoip_disable": true
///   },
///   "timestamp": "..."
/// }
/// ```
///
/// `$group_type` / `$group_key` / `$group_set` 是 PostHog group analytics 的
/// 协议字段——服务端据此把 group property 写到指定 group。
///
/// `distinct_id` 由调用方（sink）从全局 EventContext 取，保证与 capture / identify
/// 的 person 一致——这是 PostHog 关联 group 与 person 的关键。
fn build_group_identify_body(
    payload: &GroupIdentifyPayload,
    distinct_id: String,
    api_key: &str,
) -> Value {
    let mut props = Map::new();
    props.insert(
        "$group_type".into(),
        Value::String(payload.group_type.clone()),
    );
    props.insert(
        "$group_key".into(),
        Value::String(payload.group_key.clone()),
    );
    props.insert("$group_set".into(), Value::Object(payload.set.clone()));
    props.insert("$lib".into(), Value::String(POSTHOG_LIB_NAME.into()));
    props.insert("$geoip_disable".into(), Value::Bool(true));

    let mut body = Map::new();
    body.insert("api_key".into(), Value::String(api_key.to_string()));
    body.insert("event".into(), Value::String("$groupidentify".into()));
    body.insert("distinct_id".into(), Value::String(distinct_id));
    body.insert("properties".into(), Value::Object(props));
    body.insert(
        "timestamp".into(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    Value::Object(body)
}

/// 安装期不变量写入 `$set_once`：仅 person 首次出现时写入，后续被 PostHog 忽略。
///
/// 用 `initial_*` 前缀避免与 `$set` 当前快照同名冲突——dashboard 上可同时看到
/// "首次安装的 OS" vs "当前 OS"，捕获版本迁移 / 跨平台切换信号。
fn build_set_once_initial(payload: &Map<String, Value>) -> Map<String, Value> {
    const INITIAL_KEYS: &[(&str, &str)] = &[
        ("app_version", "initial_app_version"),
        ("app_channel", "initial_app_channel"),
        ("os", "initial_os"),
        ("install_source", "initial_install_source"),
    ];
    let mut out = Map::new();
    for (src, dst) in INITIAL_KEYS {
        if let Some(v) = payload.get(*src).cloned() {
            out.insert((*dst).into(), v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::super::context::{
        build_event_context, clear_global_event_context, lock_global_event_context_for_tests,
        set_global_event_context, AnalyticsPersonId, AppChannel, EventContextInputs, InstallSource,
    };
    use super::*;

    // —— 7b-1 骨架测试 ————————————————————————————————

    #[test]
    fn new_uses_us_endpoint_by_default() {
        let sink = PosthogSink::new("phc_test".into());
        assert_eq!(sink.endpoint, POSTHOG_US_CAPTURE_ENDPOINT);
        assert_eq!(sink.api_key, "phc_test");
    }

    #[test]
    fn with_endpoint_overrides_capture_url() {
        let sink = PosthogSink::with_endpoint("phc_test".into(), "http://127.0.0.1:9999/x".into());
        assert_eq!(sink.endpoint, "http://127.0.0.1:9999/x");
    }

    #[test]
    fn implements_analytics_port_object_safe() {
        let _sink: Box<dyn AnalyticsPort> = Box::new(PosthogSink::new("phc_test".into()));
    }

    // —— 7b-2 build_capture_body 纯 fn 测试 ————————————————————————————————

    fn payload_with(
        event_name: &str,
        distinct_id: &str,
        extras: &[(&str, Value)],
    ) -> Map<String, Value> {
        let mut p = Map::new();
        p.insert("event".into(), Value::String(event_name.to_string()));
        p.insert("distinct_id".into(), Value::String(distinct_id.to_string()));
        for (k, v) in extras {
            p.insert((*k).into(), v.clone());
        }
        p
    }

    #[test]
    fn build_capture_body_top_level_fields_present() {
        let payload = payload_with(
            "app_first_open",
            "018f0000-0000-7000-8000-000000000001",
            &[("app_version", Value::String("0.7.0-alpha.7".into()))],
        );
        let body = build_capture_body("app_first_open", payload, "phc_test");
        let obj = body.as_object().expect("body is object");

        assert_eq!(obj.get("api_key").and_then(Value::as_str), Some("phc_test"));
        assert_eq!(
            obj.get("event").and_then(Value::as_str),
            Some("app_first_open")
        );
        assert_eq!(
            obj.get("distinct_id").and_then(Value::as_str),
            Some("018f0000-0000-7000-8000-000000000001")
        );
        // timestamp 是 RFC3339 字符串（不直接比值，只校结构）
        let ts = obj.get("timestamp").and_then(Value::as_str).unwrap();
        assert!(
            ts.contains('T') && (ts.ends_with("+00:00") || ts.ends_with('Z')),
            "timestamp 应为 RFC3339 UTC：{ts}"
        );
    }

    /// 字段冲突 invariant：properties 里不得保留 `event` / `distinct_id`，
    /// 否则 PostHog 服务端会把它们当成普通属性，与顶层主键发生 collision。
    #[test]
    fn build_capture_body_strips_event_and_distinct_id_from_properties() {
        let payload = payload_with(
            "sync_succeeded",
            "user-123",
            &[("transport_type", Value::String("p2p_direct".into()))],
        );
        let body = build_capture_body("sync_succeeded", payload, "phc_test");
        let props = body
            .as_object()
            .unwrap()
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties 应为 object");

        assert!(
            !props.contains_key("event"),
            "properties 不得保留 event：{props:?}"
        );
        assert!(
            !props.contains_key("distinct_id"),
            "properties 不得保留 distinct_id：{props:?}"
        );
        // 其它字段必须平铺保留（与 dashboard 字段一致）。
        assert_eq!(
            props.get("transport_type").and_then(Value::as_str),
            Some("p2p_direct")
        );
    }

    #[test]
    fn build_capture_body_falls_back_to_empty_distinct_id_when_missing() {
        // 防御性：build_event_payload 生产路径下 distinct_id 必存（含
        // anonymous_user_id），但若 ctx 字段缺漏不应让 PosthogSink panic。
        let mut p = Map::new();
        p.insert("event".into(), Value::String("app_first_open".into()));
        let body = build_capture_body("app_first_open", p, "phc_test");
        assert_eq!(body["distinct_id"], Value::String(String::new()));
    }

    #[test]
    fn build_capture_body_preserves_property_value_types() {
        let payload = payload_with(
            "first_clipboard_sync_succeeded",
            "user-1",
            &[
                ("duration_ms", Value::Number(1234.into())),
                ("is_first_run", Value::Bool(true)),
                ("active_device_count", Value::Number(2.into())),
            ],
        );
        let body = build_capture_body("first_clipboard_sync_succeeded", payload, "phc_test");
        let props = body["properties"].as_object().unwrap();

        // PostHog 端 dashboard 依赖 numeric 而非 string ——区间化字段
        // (如 active_device_count) 必须保 number 类型不退化为字符串。
        assert!(props["duration_ms"].is_number());
        assert!(props["is_first_run"].is_boolean());
        assert!(props["active_device_count"].is_number());
    }

    // —— PostHog 标准 $-prefix 字段映射 ————————————————————————————————

    /// `$device_id` / `$session_id` 必须从 vendor-neutral 字段派生——PostHog
    /// 控制台的 Person ↔ Device / Session funnel & Replay 都依赖这两个 key。
    #[test]
    fn build_capture_body_emits_posthog_device_and_session_ids() {
        let payload = payload_with(
            "app_first_open",
            "anon-1",
            &[
                (
                    "analytics_device_id",
                    Value::String("018f0000-0000-7000-8000-000000000002".into()),
                ),
                (
                    "session_id",
                    Value::String("018f0000-0000-7000-8000-000000000003".into()),
                ),
            ],
        );
        let body = build_capture_body("app_first_open", payload, "phc_test");
        let props = body["properties"]
            .as_object()
            .expect("properties is object");

        assert_eq!(
            props.get("$device_id").and_then(Value::as_str),
            Some("018f0000-0000-7000-8000-000000000002"),
            "$device_id 必须从 analytics_device_id 派生"
        );
        assert_eq!(
            props.get("$session_id").and_then(Value::as_str),
            Some("018f0000-0000-7000-8000-000000000003"),
            "$session_id 必须从 session_id 派生"
        );
        // flat 字段同时保留——schema doc §4 仍是 wire 契约。
        assert!(props.contains_key("analytics_device_id"));
        assert!(props.contains_key("session_id"));
    }

    /// `$lib` / `$lib_version` 自报客户端身份——v1 自写 HTTP client，没有
    /// PostHog SDK 自动注入，必须手填。否则控制台所有事件 `$lib` 为空，与
    /// 浏览器 SDK / 第三方 SDK 流量混在一起，按来源过滤完全失效。
    #[test]
    fn build_capture_body_emits_lib_metadata() {
        let payload = payload_with(
            "app_first_open",
            "anon-1",
            &[("app_version", Value::String("0.7.0-alpha.7".into()))],
        );
        let body = build_capture_body("app_first_open", payload, "phc_test");
        let props = body["properties"].as_object().unwrap();

        assert_eq!(
            props.get("$lib").and_then(Value::as_str),
            Some("uniclipboard-rust"),
            "$lib 应固定为 client 名"
        );
        assert_eq!(
            props.get("$lib_version").and_then(Value::as_str),
            Some("0.7.0-alpha.7"),
            "$lib_version 应等于 ctx.app_version"
        );
    }

    /// `$os` / `$os_version` 是 PostHog 内置 dashboard 的"Top OS" / "OS version"
    /// breakdown 的数据源——必须从 vendor-neutral 的 `os` / `os_version` 派生，
    /// 否则控制台这两个图表对桌面端流量永远空白。
    #[test]
    fn build_capture_body_emits_posthog_os_fields() {
        let payload = payload_with(
            "app_opened",
            "anon-1",
            &[
                ("os", Value::String("macos".into())),
                ("os_version", Value::String("15.1".into())),
            ],
        );
        let body = build_capture_body("app_opened", payload, "phc_test");
        let props = body["properties"].as_object().unwrap();

        assert_eq!(
            props.get("$os").and_then(Value::as_str),
            Some("macos"),
            "$os 必须从 ctx.os 派生"
        );
        assert_eq!(
            props.get("$os_version").and_then(Value::as_str),
            Some("15.1"),
            "$os_version 必须从 ctx.os_version 派生"
        );
        // flat 字段同时保留——schema doc §4 wire 契约不变。
        assert_eq!(props.get("os").and_then(Value::as_str), Some("macos"));
        assert_eq!(
            props.get("os_version").and_then(Value::as_str),
            Some("15.1")
        );
    }

    /// `$device_type` 固定 `"Desktop"`——uniclipboard 桌面 daemon 只可能跑在
    /// 桌面 OS。PostHog 内置"Device type"图表读这个字段做 Mobile/Tablet/Desktop
    /// 切片，缺失会让所有事件归到"Unknown"。
    #[test]
    fn build_capture_body_emits_device_type_desktop() {
        let payload = payload_with("app_opened", "anon-1", &[]);
        let body = build_capture_body("app_opened", payload, "phc_test");
        let props = body["properties"].as_object().unwrap();

        assert_eq!(
            props.get("$device_type").and_then(Value::as_str),
            Some("Desktop"),
            "$device_type 必须固定为 PostHog 标准枚举 Desktop"
        );
    }

    /// `$geoip_disable: true` 是 schema doc §6.1 隐私契约的兜底实施——拒绝
    /// 服务端按请求 IP 反推 `$geoip_country` / `$geoip_city` 落到 person property。
    /// 与 SDK 的 `disable_geoip=true` 等价。
    #[test]
    fn build_capture_body_disables_geoip_by_default() {
        let payload = payload_with("app_first_open", "anon-1", &[]);
        let body = build_capture_body("app_first_open", payload, "phc_test");
        let props = body["properties"].as_object().unwrap();

        assert_eq!(
            props.get("$geoip_disable"),
            Some(&Value::Bool(true)),
            "$geoip_disable 必须默认为 true，兜底执行 §6.1 IP 永不上传契约"
        );
    }

    /// `$set` 路由 person property 当前快照——控制台按 person 维度切片（如
    /// "macOS 用户的留存"）读的是 person property 而非 event property。
    /// 没有 $set 这类查询每次都要在 event property 上 distinct/聚合，慢且贵。
    #[test]
    fn build_capture_body_routes_person_properties_to_set() {
        let payload = payload_with(
            "sync_succeeded",
            "anon-1",
            &[
                ("app_version", Value::String("0.7.0-alpha.7".into())),
                ("app_channel", Value::String("alpha".into())),
                ("os", Value::String("macos".into())),
                ("os_version", Value::String("15.1".into())),
                ("arch", Value::String("aarch64".into())),
                ("locale", Value::String("zh-CN".into())),
                ("timezone", Value::String("+08:00".into())),
                ("active_device_count", Value::Number(2.into())),
                ("space_id_hash", Value::String("0123456789abcdef".into())),
            ],
        );
        let body = build_capture_body("sync_succeeded", payload, "phc_test");
        let props = body["properties"].as_object().unwrap();
        let set = props
            .get("$set")
            .and_then(Value::as_object)
            .expect("$set 应为 object");

        // person 快照字段必须全部进 $set。
        for k in [
            "app_version",
            "app_channel",
            "os",
            "os_version",
            "arch",
            "locale",
            "timezone",
            "active_device_count",
            "space_id_hash",
        ] {
            assert!(set.contains_key(k), "$set 缺字段 `{k}`：{set:?}");
        }
        // 数值字段类型不退化为字符串。
        assert!(set["active_device_count"].is_number());
    }

    /// `space_id_hash` 在未加入 Space 时是 `None`——不应误把 `null` 写入 $set。
    /// PostHog 把 `null` 当显式清空指令，会把已有 person property 抹掉。
    #[test]
    fn build_capture_body_set_omits_missing_optional_fields() {
        // payload 不含 space_id_hash 字段（context 中是 None）。
        let payload = payload_with(
            "app_first_open",
            "anon-1",
            &[("app_version", Value::String("0.7.0-alpha.7".into()))],
        );
        let body = build_capture_body("app_first_open", payload, "phc_test");
        let set = body["properties"]["$set"]
            .as_object()
            .expect("$set 应为 object");

        assert!(
            !set.contains_key("space_id_hash"),
            "缺失的 optional 字段不应出现在 $set（否则会被解释为 null 清空）"
        );
    }

    /// `$set_once` 路由安装期不变量——PostHog 仅在 person 首次出现时写入，
    /// 后续同名 key 被忽略。dashboard 上可同时看到"首次安装的 OS" vs
    /// "$set 的当前 OS"，捕获版本迁移 / 跨平台切换信号。
    #[test]
    fn build_capture_body_routes_install_initial_to_set_once() {
        let payload = payload_with(
            "app_first_open",
            "anon-1",
            &[
                ("app_version", Value::String("0.7.0-alpha.7".into())),
                ("app_channel", Value::String("alpha".into())),
                ("os", Value::String("macos".into())),
                ("install_source", Value::String("github".into())),
            ],
        );
        let body = build_capture_body("app_first_open", payload, "phc_test");
        let set_once = body["properties"]["$set_once"]
            .as_object()
            .expect("$set_once 应为 object");

        assert_eq!(
            set_once.get("initial_app_version").and_then(Value::as_str),
            Some("0.7.0-alpha.7")
        );
        assert_eq!(
            set_once.get("initial_app_channel").and_then(Value::as_str),
            Some("alpha")
        );
        assert_eq!(
            set_once.get("initial_os").and_then(Value::as_str),
            Some("macos")
        );
        assert_eq!(
            set_once
                .get("initial_install_source")
                .and_then(Value::as_str),
            Some("github"),
            "install_source 必须落 $set_once 而不是 $set——后续不应被覆盖"
        );
    }

    // —— 7b-2 HTTP 烟测（wiremock）————————————————————————————————

    fn install_ctx() {
        let anon = Uuid::parse_str("018f0000-0000-7000-8000-000000000001").unwrap();
        let dev = Uuid::parse_str("018f0000-0000-7000-8000-000000000002").unwrap();
        let ctx = build_event_context(EventContextInputs {
            anonymous_user_id: anon,
            analytics_device_id: dev,
            app_version: "0.7.0-alpha.7".into(),
            app_channel: AppChannel::Alpha,
            install_source: InstallSource::Unknown,
            is_first_run: true,
            active_device_count: 1,
            space_id_hash: None,
            analytics_person_id: AnalyticsPersonId::Solo(anon),
        });
        set_global_event_context(Arc::new(ctx));
    }

    /// 串行子函数：context 装好 → capture 一条 → wiremock 应收到 1 个 POST，
    /// body 顶层含 `api_key` / `event` / `distinct_id`，`properties` 不带
    /// `event` / `distinct_id`（字段冲突 invariant 在 HTTP 路径上的复测）。
    async fn case_capture_posts_to_endpoint(server: &MockServer) {
        clear_global_event_context();
        install_ctx();

        Mock::given(method("POST"))
            .and(path("/i/v0/e/"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(server)
            .await;

        let endpoint = format!("{}/i/v0/e/", server.uri());
        let sink = PosthogSink::with_endpoint("phc_test".into(), endpoint);
        sink.capture(Event::AppFirstOpen);

        // tokio::spawn 是 fire-and-forget，等一拍让 mock server 收到请求。
        // wiremock Drop 会校验 expect(1) — 这里给 spawn 出去的 task 留时间。
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let received = server.received_requests().await.expect("requests 列表");
        assert_eq!(received.len(), 1, "应收到 1 个 POST");
        let body: Value = serde_json::from_slice(&received[0].body).expect("body 是 JSON");
        let obj = body.as_object().unwrap();

        assert_eq!(obj.get("api_key").and_then(Value::as_str), Some("phc_test"));
        assert_eq!(
            obj.get("event").and_then(Value::as_str),
            Some("app_first_open")
        );
        assert_eq!(
            obj.get("distinct_id").and_then(Value::as_str),
            Some("018f0000-0000-7000-8000-000000000001")
        );

        let props = obj["properties"].as_object().unwrap();
        assert!(
            !props.contains_key("event"),
            "字段冲突 invariant：properties.event 必须移除"
        );
        assert!(
            !props.contains_key("distinct_id"),
            "字段冲突 invariant：properties.distinct_id 必须移除"
        );
        // EventContext 字段在 wire 上保留（dashboard 切片必需）。
        assert_eq!(
            props.get("app_version").and_then(Value::as_str),
            Some("0.7.0-alpha.7")
        );

        // —— PostHog 标准 $-prefix 字段端到端复测 ——
        assert_eq!(
            props.get("$device_id").and_then(Value::as_str),
            Some("018f0000-0000-7000-8000-000000000002"),
            "$device_id 必须穿越 HTTP 边界保留"
        );
        assert!(
            props.get("$session_id").and_then(Value::as_str).is_some(),
            "$session_id 必须穿越 HTTP 边界保留"
        );
        assert_eq!(
            props.get("$lib").and_then(Value::as_str),
            Some("uniclipboard-rust")
        );
        assert_eq!(
            props.get("$lib_version").and_then(Value::as_str),
            Some("0.7.0-alpha.7")
        );
        assert_eq!(props.get("$geoip_disable"), Some(&Value::Bool(true)));
        // `$os` / `$os_version` / `$device_type` 必须穿越 HTTP 边界保留，
        // PostHog 内置 OS / Device type breakdown 才能命中。
        assert!(
            props.get("$os").and_then(Value::as_str).is_some(),
            "$os 必须穿越 HTTP 边界保留"
        );
        assert!(
            props.get("$os_version").and_then(Value::as_str).is_some(),
            "$os_version 必须穿越 HTTP 边界保留"
        );
        assert_eq!(
            props.get("$device_type").and_then(Value::as_str),
            Some("Desktop"),
            "$device_type 必须穿越 HTTP 边界保留"
        );
        assert!(
            props.get("$set").and_then(Value::as_object).is_some(),
            "$set 必须穿越 HTTP 边界保留"
        );
        assert!(
            props.get("$set_once").and_then(Value::as_object).is_some(),
            "$set_once 必须穿越 HTTP 边界保留"
        );
    }

    /// context 缺失：capture 不发 HTTP，wiremock 收 0 请求；warn 节流到 1 次。
    async fn case_drops_event_when_context_missing(server: &MockServer) {
        clear_global_event_context();

        Mock::given(method("POST"))
            .and(path("/i/v0/e/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(server)
            .await;

        let endpoint = format!("{}/i/v0/e/", server.uri());
        let sink = PosthogSink::with_endpoint("phc_test".into(), endpoint);
        sink.capture(Event::AppFirstOpen);
        sink.capture(Event::AppFirstOpen); // 第二次也不发，warn 不重复

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let received = server.received_requests().await.expect("requests 列表");
        assert!(received.is_empty(), "context 缺失时不应有 POST");
    }

    /// 全局 RwLock + wiremock 实例都是进程级共享资源——和
    /// `analytics::sinks::stdout::tests::stdout_sink_lifecycle` 同款，用单一
    /// async fn 串行化 case，避免 `cargo test` 多线程下竞态。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn posthog_sink_lifecycle() {
        let _guard = lock_global_event_context_for_tests();
        let server = MockServer::start().await;

        case_capture_posts_to_endpoint(&server).await;

        // 重置 mock server 计数器；wiremock 的 expect 是 per-mount 而非 per-server，
        // 重新 start 一个新实例可保各 case 隔离干净。
        drop(server);
        let server = MockServer::start().await;
        case_drops_event_when_context_missing(&server).await;

        clear_global_event_context();
    }

    // —— PR 3：$identify wire 形态（PostHog spec）————————————————————

    fn sample_identify(extra_set: bool) -> IdentifyPayload {
        let old = Uuid::parse_str("018f0000-0000-7000-8000-000000000001").unwrap();
        let new = Uuid::parse_str("018f0000-0000-7000-8000-00000000000a").unwrap();
        let mut payload = IdentifyPayload::switch_only(old, new);
        if extra_set {
            payload
                .set
                .insert("active_device_count".into(), Value::Number(2.into()));
            payload.set_once.insert(
                "first_paired_at".into(),
                Value::String("2026-05-15T00:00:00Z".into()),
            );
        }
        payload
    }

    #[test]
    fn build_identify_body_emits_top_level_event_and_distinct_id() {
        let body = build_identify_body(&sample_identify(false), "phc_test");
        let obj = body.as_object().unwrap();
        assert_eq!(obj.get("api_key").and_then(Value::as_str), Some("phc_test"));
        assert_eq!(
            obj.get("event").and_then(Value::as_str),
            Some("$identify"),
            "顶层 event 必须是 $identify"
        );
        assert_eq!(
            obj.get("distinct_id").and_then(Value::as_str),
            Some("018f0000-0000-7000-8000-00000000000a"),
            "顶层 distinct_id 必须是 new_distinct_id（PostHog alias 协议）"
        );
        // RFC3339 时间戳结构校验。
        let ts = obj.get("timestamp").and_then(Value::as_str).unwrap();
        assert!(ts.contains('T') && (ts.ends_with("+00:00") || ts.ends_with('Z')));
    }

    #[test]
    fn build_identify_body_places_anon_distinct_id_inside_properties() {
        // PostHog 协议硬要求：$anon_distinct_id 必须在 properties，不在顶层。
        // 顶层有 distinct_id（new），再有 $anon_distinct_id 会被服务端解释为冲突。
        let body = build_identify_body(&sample_identify(false), "phc_test");
        let obj = body.as_object().unwrap();
        assert!(
            !obj.contains_key("$anon_distinct_id"),
            "$anon_distinct_id 不应出现在顶层"
        );
        let props = obj["properties"].as_object().unwrap();
        assert_eq!(
            props.get("$anon_distinct_id").and_then(Value::as_str),
            Some("018f0000-0000-7000-8000-000000000001"),
            "$anon_distinct_id 必须在 properties"
        );
    }

    #[test]
    fn build_identify_body_omits_empty_set_and_set_once() {
        // 空 map 不进 wire——PostHog 把空 $set 当成"清空所有 person property"，
        // 默认不传比传空对象安全。
        let body = build_identify_body(&sample_identify(false), "phc_test");
        let props = body["properties"].as_object().unwrap();
        assert!(
            !props.contains_key("$set"),
            "空 set 不应出现在 wire：{props:?}"
        );
        assert!(
            !props.contains_key("$set_once"),
            "空 set_once 不应出现在 wire：{props:?}"
        );
    }

    #[test]
    fn build_identify_body_includes_set_and_set_once_when_present() {
        let body = build_identify_body(&sample_identify(true), "phc_test");
        let props = body["properties"].as_object().unwrap();

        let set = props["$set"].as_object().expect("$set 应为 object");
        assert_eq!(set["active_device_count"], Value::Number(2.into()));

        let set_once = props["$set_once"]
            .as_object()
            .expect("$set_once 应为 object");
        assert_eq!(
            set_once["first_paired_at"].as_str(),
            Some("2026-05-15T00:00:00Z")
        );
    }

    // —— PR 7：$groups 注入 + $groupidentify wire 形态 ————————————

    #[test]
    fn build_capture_body_injects_groups_when_space_id_hash_present() {
        let payload = payload_with(
            "sync_succeeded",
            "user-1",
            &[("space_id_hash", Value::String("0123456789abcdef".into()))],
        );
        let body = build_capture_body("sync_succeeded", payload, "phc_test");
        let props = body["properties"].as_object().unwrap();

        let groups = props
            .get("$groups")
            .and_then(Value::as_object)
            .expect("$groups 应为 object");
        assert_eq!(
            groups.get("space").and_then(Value::as_str),
            Some("0123456789abcdef"),
            "$groups.space 必须等于 space_id_hash"
        );
    }

    /// Solo 状态下 space_id_hash = None → $groups 不应出现，避免 PostHog 把
    /// "空 group" 误解为"清空已有归属"。
    #[test]
    fn build_capture_body_omits_groups_when_space_id_hash_missing() {
        let payload = payload_with("app_first_open", "user-1", &[]);
        let body = build_capture_body("app_first_open", payload, "phc_test");
        let props = body["properties"].as_object().unwrap();
        assert!(
            !props.contains_key("$groups"),
            "Solo 状态下不应出现 $groups：{props:?}"
        );
    }

    #[test]
    fn build_group_identify_body_emits_top_level_fields() {
        let mut set = Map::new();
        set.insert("device_count".into(), Value::Number(1.into()));
        set.insert(
            "created_at".into(),
            Value::String("2026-05-15T00:00:00Z".into()),
        );
        let payload = GroupIdentifyPayload::for_space("0123456789abcdef".into(), set);
        let body = build_group_identify_body(
            &payload,
            "018f0000-0000-7000-8000-00000000000a".into(),
            "phc_test",
        );
        let obj = body.as_object().unwrap();

        assert_eq!(obj.get("api_key").and_then(Value::as_str), Some("phc_test"));
        assert_eq!(
            obj.get("event").and_then(Value::as_str),
            Some("$groupidentify"),
            "顶层 event 必须是 $groupidentify"
        );
        assert_eq!(
            obj.get("distinct_id").and_then(Value::as_str),
            Some("018f0000-0000-7000-8000-00000000000a"),
            "顶层 distinct_id 必须等于 caller 传入的"
        );

        let props = obj["properties"].as_object().unwrap();
        assert_eq!(
            props.get("$group_type").and_then(Value::as_str),
            Some("space")
        );
        assert_eq!(
            props.get("$group_key").and_then(Value::as_str),
            Some("0123456789abcdef")
        );
        let group_set = props["$group_set"]
            .as_object()
            .expect("$group_set 应为 object");
        assert_eq!(group_set["device_count"], Value::Number(1.into()));
        assert_eq!(
            group_set["created_at"].as_str(),
            Some("2026-05-15T00:00:00Z")
        );
    }

    #[test]
    fn build_identify_body_carries_lib_and_geoip_disable() {
        // identify 也是客户端流量，必须自报 $lib 并兜底 disable IP geoip。
        let body = build_identify_body(&sample_identify(false), "phc_test");
        let props = body["properties"].as_object().unwrap();
        assert_eq!(
            props.get("$lib").and_then(Value::as_str),
            Some("uniclipboard-rust")
        );
        assert_eq!(
            props.get("$geoip_disable"),
            Some(&Value::Bool(true)),
            "$geoip_disable 必须默认 true（schema doc §6.1 兜底）"
        );
    }
}
