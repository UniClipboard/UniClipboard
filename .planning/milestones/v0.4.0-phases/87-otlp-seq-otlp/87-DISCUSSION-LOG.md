# Phase 87 Discussion Log

**Date:** 2026-04-04
**Mode:** discuss (interactive)
**Phase:** 87 — 全面迁移 otlp, 兼容 seq 展示, 采用 otlp 的最佳实践

---

## Gray Areas Selected

**Q:** Phase 87 要讨论哪些 gray area?(可多选)
**Options:**
1. 数据模型范围 — OTLP 同时导出 traces + logs / 仅 logs / traces-only,metrics 是否进入
2. 迁移策略与兼容窗口 — 硬切换 / 并行运行 / feature flag
3. Span 层级与字段命名 — 平铺 vs parent-child,字段命名是否保留
4. 跨设备传播与导出配置 — origin_flow_id vs W3C traceparent,协议与 env 变量

**Selected:** 全部 4 项

---

## Area 1: 数据模型范围

**Q:** 本期通过 OTLP 导出哪些信号类型?
**Options:**
- Traces + Logs (推荐)
- 仅 Logs
- Traces + Logs + Metrics

**Selected:** Traces + Logs (推荐)

**Rationale captured:** 与现有 span + event 混合代码对齐;metrics 明确推迟。

---

## Area 2: 迁移策略与兼容窗口

**Q:** 如何处理现有的自定义 SeqLayer/CLEFFormat 代码?
**Options:**
- 硬切换—一步到位 (推荐)
- Feature flag 并行运行
- 并行双发送

**Selected:** 硬切换—一步到位 (推荐)

**Rationale captured:** Dev-only 工具,回归风险低;不容忍长期两套实现。

---

## Area 3: Span 层级与字段命名

**Q1:** 是否在本期重构 clipboard pipeline 的 span 拓扑?
**Options:**
- 保持平铺 sibling span (推荐)
- 重构为 parent-child 层级

**Selected:** 重构为 parent-child 层级

**Rationale captured:** 用户主动选择非推荐项,愿意承担跨 crate instrumentation 改造成本以对齐 OTel 最佳实践。

---

**Q2:** flow_id / stage / device_id 在 OTLP 下如何命名?
**Options:**
- 保留原名为自定义属性 (推荐)
- 全面对齐 semconv
- 混合: resource 用 semconv + business 用自定义

**Selected:** 全面对齐 semconv

**Claude flagged implication:** 会破坏 Phase 22/23 的 Seq signals 和文档。

---

**Q3 (confirmation):** 全面 semconv 对齐的落地方式是?
**Options:**
- 确认全面对齐,同时重写 Phase 22/23 的 Seq signals 和文档
- 对齐 semconv 但保留 flow_id/stage 作为冗余字段
- 改回混合策略

**Selected:** 确认全面对齐,同时重写 Phase 22/23 的 Seq signals 和文档

**Rationale captured:** 用户明确接受一次性破坏性变更以彻底对齐 OTel。Phase 87 的工作量因此包含 docs/seq/signals/*.json 和 logging-architecture.md 的重写。

---

## Area 4: 跨设备传播与导出配置

**Q1:** 跨设备的 flow 关联如何处理?
**Options:**
- 切换到 W3C traceparent (推荐)
- 保留 origin_flow_id 主路
- 双写: traceparent + origin_trace_id

**Selected:** 切换到 W3C traceparent (推荐)

**Rationale captured:** 与"全面 semconv 对齐"方向一致;origin_flow_id 字段墓碑化处理(结构体保留以兼容旧 peer,逻辑不再使用)。

---

**Q2:** OTLP exporter 的协议与环境变量如何设计?
**Options:**
- OTLP/HTTP-protobuf + 标准 OTel env (推荐)
- OTLP/gRPC + 标准 OTel env
- OTLP/HTTP + 保留 UC_SEQ_URL 命名

**Selected:** OTLP/HTTP-protobuf + 标准 OTel env (推荐)

**Rationale captured:** 复用现有 reqwest + rustls-tls 依赖链,不引入 tonic gRPC 新栈;env 全面对齐 OTel 标准命名,UC_SEQ_URL 停用并在启动时打 warn。

---

## Final Confirmation

**Q:** 已经覆盖: 数据模型、迁移策略、Span 拓扑与命名、跨设备传播、导出配置。准备写 CONTEXT.md?
**Selected:** 写 CONTEXT.md

---

## Summary of Locked Decisions

| # | Area | Decision |
| - | ---- | -------- |
| D-01 | 信号类型 | Traces + Logs,Metrics deferred |
| D-02 | 迁移方式 | 硬切换,删除 seq/ 子模块和 clef_format |
| D-05 | Span 拓扑 | 重构为 parent-child(root flow span + stage child) |
| D-07 | 命名策略 | 全面对齐 OTel semantic conventions |
| D-08 | 文档同步 | 本期重写 docs/seq/signals/*.json + logging-architecture.md |
| D-09 | 跨设备传播 | W3C traceparent,origin_flow_id 墓碑化 |
| D-10 | 协议兼容 | ClipboardMessage 新增 traceparent(serde default) |
| D-12 | 传输协议 | OTLP/HTTP-protobuf |
| D-13 | Env 命名 | OTEL_EXPORTER_OTLP_* 标准变量,UC_SEQ_URL 停用 |

---

_This file is for human audit reference only. Downstream agents (researcher, planner, executor) consume 87-CONTEXT.md._
