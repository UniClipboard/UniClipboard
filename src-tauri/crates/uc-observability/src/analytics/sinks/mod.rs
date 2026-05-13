//! 真实 [`AnalyticsPort`] 实现的所在地。
//!
//! [`AnalyticsPort`]: super::port::AnalyticsPort
//!
//! ## 共享 wire 形态
//!
//! 所有 sink 在上报前必须把 [`super::context::EventContext`] 与
//! [`super::events::Event`] 自身 properties 合并为同一个 JSON object。
//! 合并规则集中在 [`build_event_payload`]，保证 `StdoutSink`、未来的
//! `PosthogSink` 与任何 self-host 适配器对外字段形态完全等价——切换 sink
//! 不需要改 dashboard 或埋点点位。
//!
//! ## 字段冲突仲裁
//!
//! [`super::events::Event::properties`] 永远不会与 `EventContext` 字段同名
//! （由 `events` 模块的 `properties_are_pure_event_fields_only` 测试守住）。
//! 实现里 properties 在合并的 *后* 一步插入：万一未来违反 invariant 也是
//! event-specific 字段覆盖 context，行为不会反过来悄悄丢事件信息。

pub mod gated;
pub mod posthog;
pub mod stdout;

pub use gated::GatedAnalyticsSink;
pub use posthog::PosthogSink;
pub use stdout::StdoutSink;

use serde_json::{Map, Value};

use super::context::EventContext;
use super::events::Event;

/// 把 [`EventContext`] 与 [`Event`] 合并为 sink 上报用的 JSON object。
///
/// 输出 wire 形态：
///
/// ```json
/// {
///   "event": "<event name>",
///   "distinct_id": "<anonymous_user_id>",
///   "anonymous_user_id": "...",
///   "analytics_device_id": "...",
///   "session_id": "...",
///   "app_version": "...",
///   "app_channel": "stable",
///   "os": "macos",
///   "os_version": "...",
///   "arch": "aarch64",
///   "locale": "zh-CN",
///   "timezone": "+08:00",
///   "install_source": "unknown",
///   "is_first_run": true,
///   "active_device_count": 2,
///   "space_id_hash": "...",
///   <event-specific properties...>
/// }
/// ```
///
/// `distinct_id` 用 [`EventContext::anonymous_user_id`] 充任——PostHog 用
/// distinct_id 做漏斗主键。schema doc §3.1 明确 `anonymous_user_id` 是
/// 留存计算唯一锚点，因此跨 sink 一致。
pub fn build_event_payload(event: &Event, ctx: &EventContext) -> Map<String, Value> {
    let mut payload = Map::new();

    if let Ok(Value::Object(ctx_map)) = serde_json::to_value(ctx) {
        payload.extend(ctx_map);
    }

    payload.extend(event.properties());

    payload.insert("event".into(), Value::String(event.name().to_string()));

    if let Some(uid) = payload.get("anonymous_user_id").cloned() {
        payload.insert("distinct_id".into(), uid);
    }

    payload
}

#[cfg(test)]
mod tests {
    use super::super::context::{
        build_event_context, AppChannel, EventContextInputs, InstallSource,
    };
    use super::super::events::{
        Direction, Event, FailureReason, PayloadSizeBucket, PayloadType, SyncEventProps,
        SyncFailureStage, TransportType,
    };
    use super::*;
    use uuid::Uuid;

    fn sample_ctx() -> EventContext {
        let anon = Uuid::parse_str("018f0000-0000-7000-8000-000000000001").unwrap();
        let dev = Uuid::parse_str("018f0000-0000-7000-8000-000000000002").unwrap();
        build_event_context(EventContextInputs {
            anonymous_user_id: anon,
            analytics_device_id: dev,
            app_version: "0.7.0-alpha.6".into(),
            app_channel: AppChannel::Alpha,
            install_source: InstallSource::Unknown,
            is_first_run: true,
            active_device_count: 2,
            space_id_hash: Some("0123456789abcdef".into()),
        })
    }

    #[test]
    fn payload_top_level_includes_event_name_and_distinct_id() {
        let ctx = sample_ctx();
        let payload = build_event_payload(&Event::AppFirstOpen, &ctx);

        assert_eq!(
            payload.get("event").and_then(Value::as_str),
            Some("app_first_open")
        );
        // distinct_id 必等于 anonymous_user_id（漏斗主键约束）。
        assert_eq!(payload.get("distinct_id"), payload.get("anonymous_user_id"));
        assert!(payload.get("distinct_id").is_some());
    }

    #[test]
    fn payload_carries_all_context_fields() {
        let ctx = sample_ctx();
        let payload = build_event_payload(&Event::AppFirstOpen, &ctx);

        // schema doc §4 字段全集——任何漏字段都意味着 sink 上报缺信息。
        for field in [
            "anonymous_user_id",
            "analytics_device_id",
            "session_id",
            "app_version",
            "app_channel",
            "os",
            "os_version",
            "arch",
            "locale",
            "timezone",
            "install_source",
            "is_first_run",
            "active_device_count",
            "space_id_hash",
        ] {
            assert!(
                payload.contains_key(field),
                "EventContext 字段 `{field}` 缺失"
            );
        }
    }

    #[test]
    fn payload_merges_event_specific_properties() {
        let ctx = sample_ctx();
        let event = Event::SyncFailed(SyncEventProps {
            direction: Direction::Outbound,
            payload_type: PayloadType::Text,
            payload_size_bucket: PayloadSizeBucket::Lt1Kb,
            transport_type: TransportType::Relay,
            peer_os: None,
            sync_latency_ms: None,
            failure_reason: Some(FailureReason::Timeout),
            failure_stage: Some(SyncFailureStage::ImmediateSend),
        });
        let payload = build_event_payload(&event, &ctx);

        // event 名 + 事件特有字段都在；context 仍在。
        assert_eq!(
            payload.get("event").and_then(Value::as_str),
            Some("sync_failed")
        );
        assert_eq!(
            payload.get("failure_reason").and_then(Value::as_str),
            Some("timeout")
        );
        assert_eq!(
            payload.get("failure_stage").and_then(Value::as_str),
            Some("immediate_send")
        );
        assert_eq!(
            payload.get("transport_type").and_then(Value::as_str),
            Some("relay")
        );
        // None 字段不上 wire（events 测试已有保证；这里再守一道）。
        assert!(!payload.contains_key("sync_latency_ms"));
        // context 字段并存。
        assert_eq!(
            payload.get("app_version").and_then(Value::as_str),
            Some("0.7.0-alpha.6")
        );
    }

    #[test]
    fn payload_serializes_to_single_line_json() {
        let ctx = sample_ctx();
        let payload = build_event_payload(&Event::AppFirstOpen, &ctx);
        let line = serde_json::to_string(&Value::Object(payload)).expect("serialize");

        // 关键约束：sink 输出必须是 single-line JSON，便于 `tail -F | jq`。
        assert!(!line.contains('\n'), "wire 必须为单行：{line}");
        assert!(line.starts_with('{') && line.ends_with('}'));
    }
}
