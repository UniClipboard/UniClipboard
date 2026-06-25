//! 把跨设备 root span 上的 correlation field 映射成 Sentry 的 tag / attribute /
//! breadcrumb data,让 Issue 列表与 Log search 可以按这些维度 filter。
//!
//! ## 为什么需要这个模块
//!
//! PR2 已经把 `flow.id` / `peer.device_id` / `session.id` / `transfer.id` /
//! `flow.kind` / `flow.synthetic` / `clipboard.entry_id` 这些字段挂到了 root
//! span 上 —— 在 Sentry 的 Performance / Trace UI 上确实可以按它们 join 跨设备
//! 的两条 trace。但 sentry-tracing 默认只把它们当成 span attribute 上报,不进
//! 出站 Event / Log 的 `tags` / `attributes` 字段,**Issue 列表与 Log search
//! 因此搜不到**。
//!
//! 这个模块补上最后一公里:
//!
//! 1. [`CorrelationLayer`] 是一个轻量 `tracing_subscriber::Layer`,只做一件事
//!    —— 在 `on_new_span` / `on_record` 时把这些字段抓出来存到 span 的
//!    extensions 里,供后续 lookup。
//! 2. [`collect_from_event`] 给 [`crate::tracing`] 的 sentry `event_mapper`
//!    用 —— 当一条 tracing event 即将被翻译成 Sentry Event 时,我们沿 event
//!    所属 span 一路向上走到 root,把这些字段合并起来。
//! 3. `apply_*` 系列把合并结果应用到不同的 Sentry 出站载体:Event 用 `tags`,
//!    Log 用 `attributes`,Breadcrumb 用 `data` —— 这是 Sentry 协议里这三种
//!    类型自己规定的"可搜索 key-value"槽位。
//!
//! ## Leaf wins
//!
//! 跨级合并时 leaf(离 event 最近的祖先)优先。出现冲突的常见情形是
//! `clipboard.flow` root 与下游 `peer.dispatch` child 都带 `flow.id`(child
//! 是同一个 id 的冗余,理论上不冲突);若哪天真的冲突,leaf 表达的是"当前
//! 子流程的视角",更接近实际语义。

use std::collections::BTreeMap;
use std::fmt;

use sentry::protocol::{Breadcrumb, Event, Log, LogAttribute, Value};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// 已知的跨设备 correlation 字段子集。命名 / 类型与 PR2 标准化的 span field
/// 一一对应。
///
/// 字段全部 `Option` —— 一条 event 大概率只走过其中两三个 span,其他保持
/// `None` 不会出现在 Sentry 出站载体上(`apply_*` 系列只在 `Some` 时写入)。
#[derive(Debug, Default, Clone)]
pub(crate) struct CorrelationFields {
    pub flow_id: Option<String>,
    pub flow_kind: Option<String>,
    pub flow_synthetic: Option<bool>,
    pub peer_device_id: Option<String>,
    pub session_id: Option<String>,
    pub transfer_id: Option<String>,
    pub clipboard_entry_id: Option<String>,
}

impl CorrelationFields {
    /// 把 `parent` 中的字段填进 `self` —— 只填 `self` 还没值的位,即 leaf wins。
    fn merge_from(&mut self, parent: &Self) {
        if self.flow_id.is_none() {
            self.flow_id = parent.flow_id.clone();
        }
        if self.flow_kind.is_none() {
            self.flow_kind = parent.flow_kind.clone();
        }
        if self.flow_synthetic.is_none() {
            self.flow_synthetic = parent.flow_synthetic;
        }
        if self.peer_device_id.is_none() {
            self.peer_device_id = parent.peer_device_id.clone();
        }
        if self.session_id.is_none() {
            self.session_id = parent.session_id.clone();
        }
        if self.transfer_id.is_none() {
            self.transfer_id = parent.transfer_id.clone();
        }
        if self.clipboard_entry_id.is_none() {
            self.clipboard_entry_id = parent.clipboard_entry_id.clone();
        }
    }

    /// 应用到 Sentry Event 的 `tags`(Issue 列表可搜索维度)。
    pub fn apply_event_tags(&self, tags: &mut BTreeMap<String, String>) {
        if let Some(v) = &self.flow_id {
            tags.insert("flow.id".into(), v.clone());
        }
        if let Some(v) = &self.flow_kind {
            tags.insert("flow.kind".into(), v.clone());
        }
        if let Some(v) = self.flow_synthetic {
            tags.insert("flow.synthetic".into(), v.to_string());
        }
        if let Some(v) = &self.peer_device_id {
            tags.insert("peer.device_id".into(), v.clone());
        }
        if let Some(v) = &self.session_id {
            tags.insert("session.id".into(), v.clone());
        }
        if let Some(v) = &self.transfer_id {
            tags.insert("transfer.id".into(), v.clone());
        }
        if let Some(v) = &self.clipboard_entry_id {
            tags.insert("clipboard.entry_id".into(), v.clone());
        }
    }

    /// 应用到 Sentry Log 的 `attributes`(Log search 可过滤维度)。
    ///
    /// `Log` 协议没有 `tags`,只有 `attributes`,这是 Sentry Logs 产品的
    /// 设计 —— 用 [`LogAttribute`] 这层把任意 `serde_json::Value` 包起来。
    pub fn apply_log_attributes(&self, attrs: &mut BTreeMap<String, LogAttribute>) {
        if let Some(v) = &self.flow_id {
            attrs.insert("flow.id".into(), LogAttribute::from(v.clone()));
        }
        if let Some(v) = &self.flow_kind {
            attrs.insert("flow.kind".into(), LogAttribute::from(v.clone()));
        }
        if let Some(v) = self.flow_synthetic {
            attrs.insert("flow.synthetic".into(), LogAttribute::from(v));
        }
        if let Some(v) = &self.peer_device_id {
            attrs.insert("peer.device_id".into(), LogAttribute::from(v.clone()));
        }
        if let Some(v) = &self.session_id {
            attrs.insert("session.id".into(), LogAttribute::from(v.clone()));
        }
        if let Some(v) = &self.transfer_id {
            attrs.insert("transfer.id".into(), LogAttribute::from(v.clone()));
        }
        if let Some(v) = &self.clipboard_entry_id {
            attrs.insert("clipboard.entry_id".into(), LogAttribute::from(v.clone()));
        }
    }

    /// 应用到 Sentry Breadcrumb 的 `data`(下一条 Issue 出现时附带的上下文)。
    pub fn apply_breadcrumb_data(&self, data: &mut BTreeMap<String, Value>) {
        if let Some(v) = &self.flow_id {
            data.insert("flow.id".into(), Value::String(v.clone()));
        }
        if let Some(v) = &self.flow_kind {
            data.insert("flow.kind".into(), Value::String(v.clone()));
        }
        if let Some(v) = self.flow_synthetic {
            data.insert("flow.synthetic".into(), Value::Bool(v));
        }
        if let Some(v) = &self.peer_device_id {
            data.insert("peer.device_id".into(), Value::String(v.clone()));
        }
        if let Some(v) = &self.session_id {
            data.insert("session.id".into(), Value::String(v.clone()));
        }
        if let Some(v) = &self.transfer_id {
            data.insert("transfer.id".into(), Value::String(v.clone()));
        }
        if let Some(v) = &self.clipboard_entry_id {
            data.insert("clipboard.entry_id".into(), Value::String(v.clone()));
        }
    }
}

/// 自定义 tracing field visitor:只关心 7 个 correlation field,其他全部跳过,
/// 避免分配。
///
/// tracing 的字段记录路径:
/// - 字符串字面量 → `record_str`
/// - `%x`(Display-as-Debug,绝大多数 root span 用法) → `record_debug`
/// - `bool` → `record_bool`
///
/// 这三条路径都覆盖。`record_debug` 拿到的是 `&dyn Debug`,对 `DisplayValue`
/// 包装而言其 Debug 输出就是 Display 输出(干净的字符串,无引号);若上层
/// 用了 `?x` 形式且字段类型是 String,Debug 输出会带引号,这里 `trim_matches`
/// 去掉,让 tag 值规范化。
struct Visitor<'a>(&'a mut CorrelationFields);

impl<'a> Visit for Visitor<'a> {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "flow.id" => self.0.flow_id = Some(value.into()),
            "flow.kind" => self.0.flow_kind = Some(value.into()),
            "peer.device_id" => self.0.peer_device_id = Some(value.into()),
            "session.id" => self.0.session_id = Some(value.into()),
            "transfer.id" => self.0.transfer_id = Some(value.into()),
            "clipboard.entry_id" => self.0.clipboard_entry_id = Some(value.into()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "flow.id" | "flow.kind" | "peer.device_id" | "session.id" | "transfer.id"
            | "clipboard.entry_id" => {
                let raw = format!("{value:?}");
                let cleaned = raw.trim_matches('"').to_string();
                self.record_str(field, &cleaned);
            }
            _ => {}
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "flow.synthetic" {
            self.0.flow_synthetic = Some(value);
        }
    }
}

/// `tracing_subscriber::Layer`,只做一件事:在 span 生命周期里把 correlation
/// 字段抓出来存到 span extensions 上,供 [`collect_from_event`] 读。
///
/// 注册到 registry 上即可,不影响其他 layer 行为。
pub(crate) struct CorrelationLayer;

impl<S> Layer<S> for CorrelationLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut fields = CorrelationFields::default();
        attrs.record(&mut Visitor(&mut fields));
        span.extensions_mut().insert(fields);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut ext = span.extensions_mut();
        // 正常路径上 `on_new_span` 已经塞过一个空壳;`Span::record`
        // 后续回填的字段(如 PR2 用 `Empty` 占位再 `Span::current().record(...)`)
        // 走这条路径,合并到既有 entry 上。
        match ext.get_mut::<CorrelationFields>() {
            Some(fields) => values.record(&mut Visitor(fields)),
            None => {
                let mut fields = CorrelationFields::default();
                values.record(&mut Visitor(&mut fields));
                ext.insert(fields);
            }
        }
    }
}

/// 给 sentry `event_mapper` 用:沿 event 所属 span 一路向上走到 root,
/// 把 correlation 字段合并成一份。Leaf wins(子 span 已写的字段不会被祖先
/// 覆盖)。
///
/// `event.record(...)` 先跑一遍,把 event 自己 inline 的字段也算上 —— 极少数
/// 情况下业务代码会在 emit 时直接写 `flow.id = ...`,而不是依赖 span 上下文。
pub(crate) fn collect_from_event<S>(
    event: &tracing::Event<'_>,
    ctx: &Context<'_, S>,
) -> CorrelationFields
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let mut fields = CorrelationFields::default();
    event.record(&mut Visitor(&mut fields));
    let Some(span) = ctx.event_span(event) else {
        return fields;
    };
    for ancestor in span.scope() {
        let ext = ancestor.extensions();
        if let Some(parent) = ext.get::<CorrelationFields>() {
            fields.merge_from(parent);
        }
    }
    fields
}

/// Helper:在 sentry `event_mapper` 里把 enriched 字段一次性应用到 Event。
///
/// 单独包一层是为了让 `tracing.rs` 那段 event_mapper 更紧凑 —— 调用方拿到
/// `Event<'static>` 直接传 `&mut` 进来就行,不用关心字段映射规则。
pub(crate) fn enrich_event(event: &mut Event<'static>, fields: &CorrelationFields) {
    fields.apply_event_tags(&mut event.tags);
}

pub(crate) fn enrich_log(log: &mut Log, fields: &CorrelationFields) {
    fields.apply_log_attributes(&mut log.attributes);
}

pub(crate) fn enrich_breadcrumb(breadcrumb: &mut Breadcrumb, fields: &CorrelationFields) {
    fields.apply_breadcrumb_data(&mut breadcrumb.data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_from_leaf_wins() {
        let mut leaf = CorrelationFields {
            flow_id: Some("leaf-flow".into()),
            ..Default::default()
        };
        let parent = CorrelationFields {
            flow_id: Some("root-flow".into()),
            peer_device_id: Some("peer-A".into()),
            ..Default::default()
        };
        leaf.merge_from(&parent);
        assert_eq!(leaf.flow_id.as_deref(), Some("leaf-flow"));
        assert_eq!(leaf.peer_device_id.as_deref(), Some("peer-A"));
    }

    #[test]
    fn apply_event_tags_skips_none_fields() {
        let mut tags = BTreeMap::new();
        let fields = CorrelationFields {
            flow_id: Some("F1".into()),
            ..Default::default()
        };
        fields.apply_event_tags(&mut tags);
        assert_eq!(tags.get("flow.id").map(String::as_str), Some("F1"));
        assert!(tags.get("peer.device_id").is_none());
        assert_eq!(tags.len(), 1);
    }

    #[test]
    fn apply_event_tags_serializes_bool_synthetic() {
        let mut tags = BTreeMap::new();
        let fields = CorrelationFields {
            flow_synthetic: Some(true),
            ..Default::default()
        };
        fields.apply_event_tags(&mut tags);
        assert_eq!(tags.get("flow.synthetic").map(String::as_str), Some("true"));
    }
}
