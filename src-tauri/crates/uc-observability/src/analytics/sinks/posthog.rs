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
use super::super::port::AnalyticsPort;
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
}

/// 把 [`build_event_payload`] 的产物转成 PostHog capture endpoint 的请求 body。
///
/// 输出 wire 形态（顶层）：
///
/// ```json
/// {
///   "api_key": "phc_xxx",
///   "event": "<event name>",
///   "distinct_id": "<anonymous_user_id>",
///   "properties": { <context + event-specific 字段，不含 event / distinct_id> },
///   "timestamp": "2026-05-09T12:34:56.789+00:00"
/// }
/// ```
///
/// `event` / `distinct_id` 必须从 properties 移出——两键由 [`build_event_payload`]
/// 在顶层平铺，PostHog 服务端会用顶层值做漏斗主键；若 properties 也保留 `distinct_id`
/// 服务端会触发 property collision 警告（dashboard 上漏斗折线会出现重复 series）。
fn build_capture_body(event_name: &str, mut payload: Map<String, Value>, api_key: &str) -> Value {
    let distinct_id = payload
        .remove("distinct_id")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    payload.remove("event");

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::super::context::{
        build_event_context, clear_global_event_context, lock_global_event_context_for_tests,
        set_global_event_context, AppChannel, EventContextInputs, InstallSource,
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
}
