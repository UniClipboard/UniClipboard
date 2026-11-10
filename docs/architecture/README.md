# 架构（活文档）

这里描述系统**现在**的样子：每页回答“它是什么、必须成立的不变量、代码在哪、规格在哪”。它是后来者的入口，也是代码与 agent 引用的唯一叙述性来源；“为什么这么定”只在 `docs/decisions/` 里，且不被引用。生命周期见 [docs/decisions/README.md](../decisions/README.md)。

## 页面格式

每页（本文件除外）以 front matter 开头，由 `go run ./tools/docscheck` 校验：

```markdown
---
paths:            # 本页所述行为的代码路径；这些路径自 verified 之后有改动，检查会报 stale
  - engine/pipeline.go
  - internal/store
verified: <commit> # 对照代码核实时的提交；用 docscheck -bump 更新
---
# 标题

一句话定位。

## 现状
当前实现的行为，按事实写，不写计划。

## 不变量
违反就是 bug 的规则，每条一行。

## 代码地图
包/文件 → 职责。

## 规格与背景
- 契约：[xxx-contract](../contracts/xxx-contract.md)

## 缺口
已知未做的部分（没有就写“无”）。
```

规则：只写已实现的事实；未实现的放“缺口”一节；改了行为就同步改本页并 `-bump`。

## 索引

- [overview](overview.md)：仓库布局、应用与核心的依赖边界、模块和 CI 现状。
- [pipeline](pipeline.md)：处理流水线、提交/同步/应用、本机历史、Action。
- [identity-and-trust](identity-and-trust.md)：设备身份、握手、配对码/票据、信任与按后端关联、设备介绍。

- [desktop-runtime](desktop-runtime.md): CLI configuration, restore sends, default-cache ownership, and custom received files.
- [observability](observability.md): bounded local logs, dynamic levels, optional OTLP logs/metrics, and nonvisual acceptance limits.

- [Headless service and desktop integration](headless.md): independent distributions, process ownership and adapter sessions.
- [Real-machine CLI testing](real-machine-testing.md): node inventories, native workers, multi-host synchronization, timing and evidence.
