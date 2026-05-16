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
///   "distinct_id": "<analytics_person_id>",  // v2: Solo→anonymous_user_id；SpaceShared→space_person_id
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
/// ## distinct_id 的派生（v2 跨设备 person 聚合）
///
/// `distinct_id` 来自 [`EventContext::analytics_person_id`]：
///
/// | analytics_person_id | distinct_id |
/// |---|---|
/// | `Solo(uuid)` | `uuid`（即 `anonymous_user_id`，与 v1 wire 兼容） |
/// | `SpaceShared(uuid)` | `uuid`（即 sponsor 派发的 `space_person_id`，同 Space 多设备共享） |
///
/// schema doc §3.4：v1 → v2 wire 形态零破坏——字段名 `distinct_id` 不变，只换
/// 取值来源。Solo 状态下与 v1 byte-for-byte 等价；SpaceShared 状态下值变化
/// 但通过 PR 3 的 `$identify` 事件让 PostHog 服务端把两个 person 合并归档。
///
/// `anonymous_user_id` flat 字段**永远保留**在 properties 中，dashboard 可同时
/// 按设备级 anonymous ID 切片（schema doc §10.1 "Flat-name 字段同时保留"）。
pub fn build_event_payload(event: &Event, ctx: &EventContext) -> Map<String, Value> {
    let mut payload = Map::new();

    if let Ok(Value::Object(ctx_map)) = serde_json::to_value(ctx) {
        payload.extend(ctx_map);
    }

    payload.extend(event.properties());

    payload.insert("event".into(), Value::String(event.name().to_string()));

    // v2 切换：distinct_id 不再直接拷 anonymous_user_id 字段，而是从
    // ctx.analytics_person_id 派生。Solo 状态下两者数值相同（与 v1 兼容），
    // SpaceShared 状态下取 sponsor 派发的 space_person_id。
    payload.insert(
        "distinct_id".into(),
        Value::String(ctx.analytics_person_id.as_uuid().to_string()),
    );

    payload
}

#[cfg(test)]
mod tests {
    use super::super::context::{
        build_event_context, AnalyticsPersonId, AppChannel, EventContextInputs, InstallSource,
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
            analytics_person_id: AnalyticsPersonId::Solo(anon),
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

    // —— PR 2：v2 distinct_id 派生（schema doc §3.4）——————————————————

    /// Solo 状态：distinct_id 必须等于 anonymous_user_id（v1 wire 兼容）。
    ///
    /// 这条测试是 v1 → v2 升级时"已配对的老用户行为不变"的护栏：sample_ctx()
    /// 默认是 Solo，PR 2 切换 distinct_id 派生源后这条仍必须通过。
    #[test]
    fn payload_distinct_id_equals_anonymous_user_id_in_solo_state() {
        let ctx = sample_ctx();
        let payload = build_event_payload(&Event::AppFirstOpen, &ctx);

        assert_eq!(
            payload.get("distinct_id"),
            payload.get("anonymous_user_id"),
            "Solo 状态下 distinct_id 必须等同 anonymous_user_id（v1 兼容）"
        );
    }

    /// SpaceShared 状态：distinct_id 必须等于 space_person_id，**不再**等同
    /// anonymous_user_id；后者仍以 flat 字段保留在 properties 中。
    #[test]
    fn payload_distinct_id_equals_space_person_id_in_space_shared_state() {
        use super::super::context::AnalyticsPersonId;

        let anon = Uuid::parse_str("018f0000-0000-7000-8000-000000000001").unwrap();
        let dev = Uuid::parse_str("018f0000-0000-7000-8000-000000000002").unwrap();
        let space_person = Uuid::parse_str("018f0000-0000-7000-8000-00000000000a").unwrap();

        let ctx = build_event_context(EventContextInputs {
            anonymous_user_id: anon,
            analytics_device_id: dev,
            app_version: "0.7.0-alpha.7".into(),
            app_channel: AppChannel::Alpha,
            install_source: InstallSource::Unknown,
            is_first_run: false,
            active_device_count: 2,
            space_id_hash: Some("0123456789abcdef".into()),
            analytics_person_id: AnalyticsPersonId::SpaceShared(space_person),
        });
        let payload = build_event_payload(&Event::AppFirstOpen, &ctx);

        assert_eq!(
            payload.get("distinct_id").and_then(Value::as_str),
            Some(space_person.to_string().as_str()),
            "SpaceShared 状态下 distinct_id 必须等于 space_person_id"
        );
        assert_ne!(
            payload.get("distinct_id"),
            payload.get("anonymous_user_id"),
            "SpaceShared 状态下 distinct_id 不再等同 anonymous_user_id"
        );
        // schema doc §10.1：flat 字段保留——dashboard 仍可按设备级 anonymous 切片。
        assert_eq!(
            payload.get("anonymous_user_id").and_then(Value::as_str),
            Some(anon.to_string().as_str()),
            "anonymous_user_id flat 字段必须保留"
        );
    }

    /// PR 1 红线：analytics_person_id 字段本身**不**进 wire，
    /// 即使在 SpaceShared 状态下也不应出现在 payload 顶层。
    #[test]
    fn payload_does_not_carry_analytics_person_id_field() {
        let ctx = sample_ctx();
        let payload = build_event_payload(&Event::AppFirstOpen, &ctx);

        assert!(
            !payload.contains_key("analytics_person_id"),
            "analytics_person_id 是 sink 派生 distinct_id 的输入，不应出现在 wire payload"
        );
    }
}
