# 架构冗余 Review — spot-capricorn vs main

> 用户指令："review 一遍，against base 的 diff changes, 看下架构是否冗余"
>
> Base: `main` (merge-base `07adc0bc`), HEAD: `ea09cdd3` (spot-capricorn)
> Diff scale: 204 commits / 971 files / +63863/-78089

## Goal

对 spot-capricorn 相对 main 的全量 diff 做一次"找架构冗余"的 review。
重点不是"找 bug"或"代码风格",而是回答：

- 是否有为了已废弃的设计目标遗留的过渡性抽象？
- 是否有同一职责的代码 / 类型 / 函数被多处重复表达？
- 是否有"调用一次 / 永远只走一条分支"的接口拐杖？
- 是否有"加了但其实没派上用场"的间接层？
- 最近 daemon reload 主线决策走方案 C (取消 in-process reload) 之后，
  Phase A/B/C 为 reload 做的铺垫是否变成了死负载？

## 范围切片

按 line count 排序的产品代码 bucket(已剔除 `.claude/` / `.gsd/` /
`.planning/` / `docs/` 这些非产品代码):

| Bucket | Lines | Owner Agent |
|---|---|---|
| `crate:uc-application` | 10137 | A1 |
| `frontend` (`src/`) | 6456 | A4 |
| `crate:uc-infra` | 5319 | A2 |
| `crate:uc-desktop` | 3231 | A3 |
| `crate:uc-cli` | 2947 | A3 |
| `crate:uc-tauri` | 2873 | A3 |
| `crate:uc-webserver` | 2244 | A2 |
| `crate:uc-core` | 2077 | A1 |
| `crate:uc-daemon-local` | 1618 | A2 |
| `crate:uc-bootstrap` | 1082 | A3 |
| `crate:uc-platform` | 923 | A2 |
| `crate:uc-observability` | 858 | A4 |

合并为 4 个 review 主题：

- **A1 业务域核心** (uc-application + uc-core): 业务用例 / facade /
  domain types 是否有冗余抽象、未使用 trait、过度拆分的模块
- **A2 基础设施 + IO** (uc-infra + uc-webserver + uc-daemon-local +
  uc-platform): 多份 implementation 是否合理 / 是否有死代码 / 是否
  有 wire 端口重复
- **A3 引导与 GUI shell** (uc-bootstrap + uc-desktop + uc-cli +
  uc-tauri): **重点** — Phase A/B/C 重构后 (build_process_runtime /
  build_daemon_lifecycle / ArcSwapOption / Clone everywhere) 是否在
  方案 C 之后变成冗余？`restart_app` 路径与 in-process reload 路径
  是否还有残留？
- **A4 前端 + 观测** (frontend `src/` + uc-observability):
  observability OTLP 模块被整体删除，残留是否清理干净？前端 Settings
  / Network 等大改动后，老组件 / dead types / 重复 hooks 是否冗余？

## 已完成阶段

(尚未开始)

## 待办阶段

### Phase 1 — 范围切分 + planning 落地

**Status**: ✅ complete

- 已分桶统计 diff scale
- 已确定 4 个 review 主题，各自 sub-agent

### Phase 2 — 4 个并行 sub-agent review

**Status**: ✅ complete

- [x] A1: `findings-A1-app-core.md`
- [x] A2: `findings-A2-infra-io.md`
- [x] A3: `findings-A3-bootstrap-shell.md`
- [x] A4: `findings-A4-frontend-obs.md`

### Phase 3 — 主线汇总成最终冗余清单

**Status**: ✅ complete

`findings.md` 汇总完成，三档 + 处理顺序见该文件。

## 验收标准

- [x] Planning 文件已建立
- [x] 4 个子主题 findings 文件各自落地
- [x] 汇总清单按"必删 / 可削减 / 待定"分档
- [x] 每条冗余项有具体 file:line / 根因 / 处理建议
- [x] 重点检查方案 C 后 Phase A/B/C 是否变冗余 (用户最关心的角度)

## 决策记录

| 时间 | 决策 | 理由 |
|---|---|---|
| 2026-05-10 | 不逐文件 review, 按 4 主题切并行 | 50K 产品代码行无法串行，子主题间耦合度低 |
| 2026-05-10 | 排除 `.claude/` / `.gsd/` / `.planning/` / `docs/` | 非产品代码，不属于"架构冗余" review 范畴 |
| 2026-05-10 | 把方案 C 后 Phase A/B/C 是否还成立 列为 A3 的 P0 问题 | 这是用户最自然会怀疑的"冗余"信号 |
