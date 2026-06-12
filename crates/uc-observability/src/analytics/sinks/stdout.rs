//! `StdoutSink` —— dev 构建专用的事件镜像通道。
//!
//! 行为：把每条 [`Event`] 与 [`global_event_context`] 合并后，序列化为
//! 单行 JSON 写到 `tracing::debug!`。schema doc §6.5 约定 dev 构建可以
//! 把事件**额外**镜像到 stdout，方便核对——本 sink 是该约定的实现。
//!
//! ## 为什么走 tracing 而不是 `println!`
//!
//! - 与 `uc-observability` 的 dual-output（pretty console + JSON file）一致；
//! - 调试期 `RUST_LOG=uc_observability::analytics=debug` 即可开关；
//! - release 构建默认级别会自然吞掉，避免误投生产 stdout。
//!
//! ## context 缺失的处理
//!
//! `compose_event_context` 在 bootstrap `wire_dependencies` 之后才能完成。
//! 如果某条事件早于 EventContext 装配（例如 sink 比 context 先被构造），
//! 我们选择**丢事件 + warn**而不是落部分上下文：dev 信号要么完整要么
//! 缺席，半截事件会让 dashboard 误把"context 缺字段"当成枚举值缺漏排查。
//!
//! warn 走 `AtomicBool` 节流——每个 sink 实例只在第一次 capture 缺 context
//! 时打一条，避免日志 spam（启动早期可能连续多条）。
//!
//! [`Event`]: super::super::events::Event
//! [`global_event_context`]: super::super::context::global_event_context

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Map, Value};
use tracing::{debug, warn};

use super::super::context::global_event_context;
use super::super::events::Event;
use super::super::port::{AnalyticsPort, GroupIdentifyPayload, IdentifyPayload};
use super::build_event_payload;

/// 与 `RUST_LOG` 过滤器对齐的 tracing target。统一常量便于跨 sink 复用。
pub(crate) const TRACE_TARGET: &str = "uc_observability::analytics";

/// 把事件镜像到 `tracing::debug!` 的 dev sink。
///
/// 调用方应在构造前确认 [`crate::analytics_gate::is_analytics_enabled`]
/// 为 `true`——本 sink 不重复检查 gate（schema doc §6.4 约定 gate 责任
/// 在调用方）。
#[derive(Debug, Default)]
pub struct StdoutSink {
    warned_missing_context: AtomicBool,
}

impl StdoutSink {
    pub fn new() -> Self {
        Self::default()
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

impl AnalyticsPort for StdoutSink {
    fn capture(&self, event: Event) {
        let Some(ctx) = global_event_context() else {
            self.warn_missing_context_once(event.name());
            return;
        };
        let payload = build_event_payload(&event, &ctx);
        match serde_json::to_string(&Value::Object(payload)) {
            Ok(line) => debug!(target: TRACE_TARGET, "{line}"),
            Err(err) => warn!(
                target: TRACE_TARGET,
                event = event.name(),
                error = %err,
                "serialize analytics event failed"
            ),
        }
    }

    fn identify(&self, payload: IdentifyPayload) {
        // dev 路径下 $identify 同样镜像到 tracing::debug，方便核对身份切换时序。
        // 与 capture 不同，identify 不需要 EventContext —— 字段全部由 payload
        // 自带，缺 context 也不影响 wire 形态。
        let body = build_stdout_identify_payload(&payload);
        match serde_json::to_string(&Value::Object(body)) {
            Ok(line) => debug!(target: TRACE_TARGET, "{line}"),
            Err(err) => warn!(
                target: TRACE_TARGET,
                event = "$identify",
                error = %err,
                "serialize $identify event failed"
            ),
        }
    }

    fn group_identify(&self, payload: GroupIdentifyPayload) {
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

        let mut out = Map::new();
        out.insert("event".into(), Value::String("$groupidentify".into()));
        // distinct_id 取当前 ctx；缺失则用占位空串（dev 镜像，不发 HTTP）。
        let distinct_id = global_event_context()
            .map(|c| c.analytics_person_id.as_uuid().to_string())
            .unwrap_or_default();
        out.insert("distinct_id".into(), Value::String(distinct_id));
        out.insert("properties".into(), Value::Object(props));

        match serde_json::to_string(&Value::Object(out)) {
            Ok(line) => debug!(target: TRACE_TARGET, "{line}"),
            Err(err) => warn!(
                target: TRACE_TARGET,
                event = "$groupidentify",
                error = %err,
                "serialize $groupidentify event failed"
            ),
        }
    }
}

/// 把 [`IdentifyPayload`] 翻译成 stdout sink 用的可读 JSON。
///
/// 与 PosthogSink 的 `build_identify_capture_body` 同形态（顶层 `event` /
/// `distinct_id`，properties 内 `$anon_distinct_id` / `$set` / `$set_once`），
/// 便于 dev 时 stdout 输出与 release 上报形态对齐。
fn build_stdout_identify_payload(payload: &IdentifyPayload) -> Map<String, Value> {
    let mut props = Map::new();
    props.insert(
        "$anon_distinct_id".into(),
        Value::String(payload.old_distinct_id.to_string()),
    );
    if !payload.set.is_empty() {
        props.insert("$set".into(), Value::Object(payload.set.clone()));
    }
    if !payload.set_once.is_empty() {
        props.insert("$set_once".into(), Value::Object(payload.set_once.clone()));
    }

    let mut out = Map::new();
    out.insert("event".into(), Value::String("$identify".into()));
    out.insert(
        "distinct_id".into(),
        Value::String(payload.new_distinct_id.to_string()),
    );
    out.insert("properties".into(), Value::Object(props));
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tracing::subscriber;
    use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter};
    use uuid::Uuid;

    use super::super::super::context::{
        build_event_context, clear_global_event_context, lock_global_event_context_for_tests,
        set_global_event_context, AnalyticsPersonId, AppChannel, EventContextInputs, InstallSource,
    };
    use super::super::super::events::{
        Direction, Event, PayloadSizeBucket, PayloadType, SyncEventProps, TransportType,
    };
    use super::*;

    /// 把 tracing 输出捕获到一个 buffer 里——单元测试用，不依赖 stdout。
    #[derive(Clone, Default)]
    struct CapturedWriter(Arc<std::sync::Mutex<Vec<u8>>>);

    impl CapturedWriter {
        fn dump(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl std::io::Write for CapturedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedWriter {
        type Writer = CapturedWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn install_ctx() {
        let anon = Uuid::parse_str("018f0000-0000-7000-8000-000000000001").unwrap();
        let dev = Uuid::parse_str("018f0000-0000-7000-8000-000000000002").unwrap();
        let ctx = build_event_context(EventContextInputs {
            anonymous_user_id: anon,
            analytics_device_id: dev,
            app_version: "0.7.0-alpha.6".into(),
            app_channel: AppChannel::Alpha,
            install_source: InstallSource::Unknown,
            is_first_run: true,
            active_device_count: 2,
            space_id_hash: Some("0123456789abcdef".into()),
            analytics_person_id: AnalyticsPersonId::Solo(anon),
        });
        set_global_event_context(Arc::new(ctx));
    }

    fn with_capture<F: FnOnce()>(f: F) -> String {
        let writer = CapturedWriter::default();
        let layer = fmt::layer()
            .with_writer(writer.clone())
            .with_ansi(false)
            .with_target(true);
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("uc_observability::analytics=debug"))
            .with(layer);
        subscriber::with_default(subscriber, f);
        writer.dump()
    }

    /// 全局 RwLock 在 cargo test 里被多 fn 并发触达会竞态——和
    /// `analytics::context::tests::global_event_context_lifecycle` 同样
    /// 用单一 fn 串行化。
    #[test]
    fn stdout_sink_lifecycle() {
        let _guard = lock_global_event_context_for_tests();
        // —— case 1：context 已装好，capture 输出单行 JSON ——
        clear_global_event_context();
        install_ctx();
        let sink = StdoutSink::new();
        let captured = with_capture(|| {
            sink.capture(Event::AppFirstOpen);
        });
        assert!(
            captured.contains("\"event\":\"app_first_open\""),
            "应输出事件名：\n{captured}"
        );
        assert!(
            captured.contains("\"distinct_id\":\"018f0000-0000-7000-8000-000000000001\""),
            "应输出 distinct_id：\n{captured}"
        );
        assert!(
            captured.contains("\"app_version\":\"0.7.0-alpha.6\""),
            "应平铺 EventContext 字段：\n{captured}"
        );

        // —— case 2：sync_failed 携带事件特有字段 ——
        let captured = with_capture(|| {
            sink.capture(Event::SyncFailed(SyncEventProps {
                direction: Direction::Outbound,
                payload_type: PayloadType::Text,
                payload_size_bucket: PayloadSizeBucket::Lt1Kb,
                transport_type: TransportType::Relay,
                peer_os: None,
                sync_latency_ms: None,
                failure_reason: Some(super::super::super::events::FailureReason::Timeout),
                failure_stage: Some(super::super::super::events::SyncFailureStage::ImmediateSend),
            }));
        });
        assert!(captured.contains("\"event\":\"sync_failed\""), "{captured}");
        assert!(
            captured.contains("\"failure_reason\":\"timeout\""),
            "{captured}"
        );
        assert!(
            captured.contains("\"failure_stage\":\"immediate_send\""),
            "{captured}"
        );
        assert!(
            !captured.contains("\"sync_latency_ms\""),
            "None 字段不应上 wire：{captured}"
        );

        // —— case 3：context 缺失 → 丢事件 + warn 一次 ——
        clear_global_event_context();
        let sink = StdoutSink::new();
        let captured = with_capture(|| {
            sink.capture(Event::AppFirstOpen);
            // 第二次 capture 不应再 warn，避免日志 spam。
            sink.capture(Event::AppFirstOpen);
        });
        assert!(
            !captured.contains("\"event\":\"app_first_open\""),
            "无 context 时事件应被丢弃，不上 wire：\n{captured}"
        );
        assert!(
            captured.contains("global EventContext not yet set"),
            "应记录一次 warn：\n{captured}"
        );
        // warn 节流：完整捕获里只有一条 warn 行（按出现次数粗略校验）。
        let warn_count = captured.matches("global EventContext not yet set").count();
        assert_eq!(warn_count, 1, "warn 应节流为 1 次：实际 {warn_count}");

        // —— 收尾：恢复全局状态，避免污染其它测试 ——
        clear_global_event_context();
    }

    /// PR 3：StdoutSink 的 identify 输出形态必须与 PostHog wire 对齐
    /// （顶层 `event=$identify` + `distinct_id`，properties 内 `$anon_distinct_id`），
    /// 这样 dev 时 `tail -F | jq` 看到的就是 release 真实上报的样子。
    #[test]
    fn stdout_sink_identify_emits_alias_event() {
        // identify 不依赖 EventContext，所以不需要 lock_global_event_context_for_tests。
        let sink = StdoutSink::new();
        let old = Uuid::parse_str("018f0000-0000-7000-8000-000000000001").unwrap();
        let new = Uuid::parse_str("018f0000-0000-7000-8000-00000000000a").unwrap();

        let captured = with_capture(|| sink.identify(IdentifyPayload::switch_only(old, new)));

        assert!(
            captured.contains("\"event\":\"$identify\""),
            "应输出 $identify 事件名：\n{captured}"
        );
        assert!(
            captured.contains(&format!("\"distinct_id\":\"{}\"", new)),
            "顶层 distinct_id 必须是 new_distinct_id：\n{captured}"
        );
        assert!(
            captured.contains(&format!("\"$anon_distinct_id\":\"{}\"", old)),
            "$anon_distinct_id 必须出现在 properties（PostHog spec）：\n{captured}"
        );
    }
}
