# 卡片格式（e2e/cards/）

## 这是什么

`e2e/cards/` 是 living test corpus：每张卡片描述一个独立、可重放的真机场景。

卡片不是测试代码，而是测试 **契约**。执行器（webdriverio + tauri-driver）读卡片决定怎么操作两台 Tauri 实例；编排 agent 读卡片决定 PR diff 命中哪些场景；归因 agent 读卡片的"已知失败模式"做先验。

## 命名

- 一卡片 = 一文件 = 一场景
- 文件名 kebab-case，体现"对什么的什么验证"，例：`pairing-delivery-badge-realtime.md`
- 文件名里不要写进度 / 版本 / PR 号——卡片是 living 的，那些会过期

## frontmatter 字段

### 必填

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | 与文件名（不含 `.md`）一致，全局唯一 |
| `title` | string | 一句话场景描述 |
| `topology` | enum | `single` / `dual-device` / `daemon-only` / `in-process-stack` |
| `runtime` | array | 当前只支持 `[linux, windows]`；macOS 因 tauri-driver 不支持被排除 |
| `modules` | array | 文件 / 目录路径，agent 用 PR diff 与之求交集做选片 |
| `selectors` | map | 命名 → CSS 选择器；断言层只走这些确定性锚点，不用视觉判定 |
| `budget_ms` | number | 主要断言的时间预算，超时即失败 |

### 可选

| 字段 | 类型 | 说明 |
|------|------|------|
| `event_paths` | array | 后端事件路径，归因 agent 抓日志用 |
| `known_flakes` | array | 已知抖动来源，归因时降权而非误报 |
| `preconditions` | array | 前置环境约束（已配对、特定窗口已打开） |
| `requires_fixture` | string | 需要的外部 mock / 夹具名（如 `update-mock`） |

## markdown 章节

必填四节，顺序固定：

- `## 前置` — 卡片开跑前必须满足的状态
- `## 步骤` — 编号操作序列，agent 严格按序执行
- `## 断言` — 子弹列表，每条一个可独立判定的命题
- `## 已知失败模式` — 给归因 agent 的先验，格式 `症状 → 嫌疑模块/原因`

复杂卡片（如成功 + 失败两条路径）可以把"步骤"和"断言"拆成 `## 步骤（成功路径）` / `## 断言（成功路径）` / `## 步骤（失败路径）` / `## 断言（失败路径）`。

## Radix portal 与 selector 写法

uniclipboard UI 用 Radix UI 的 HoverCard / Popover / Dialog / Tooltip。这些组件运行时通过 portal 把 content 渲染到 `document.body` 末尾，**不在 trigger 的 DOM 子树里**。

卡片 selector 涉及这些组件的 content 时，**不要** 写 `[trigger-ancestor] [content-marker]` —— 会匹配不到。

正确做法：
- 直接全局选：`'[data-delivery-popover]'`
- 或在 content 上加 origin 标记区分来源：`'[data-delivery-popover][data-popover-origin="quick-panel"]'`

## selectors 字段的契约方向

卡片里写的 selector **不一定已存在于代码**。卡片定义的是"代码应该暴露什么 testid"——一种反向约束。

第一次执行卡片缺 testid 而失败时，做法是 **把 testid 补到组件** 作为实现的一部分，而不是改卡片去迁就当前 DOM。这样卡片库会逐步把"可测试性"沉淀进代码。

## 卡片不该做什么

- 不写"为什么这个功能存在"——那是 PR description / ADR 的职责
- 不写实现细节——卡片是黑盒场景，描述用户能看到 / 系统能观测到的事实
- 不内联具体 PR 号 / commit hash——卡片是 living，PR 是历史
- 不把多个场景塞进同一张卡片——拆开命中率才准
- 不写 macOS 专属场景——目前 tauri-driver 不支持 macOS，相关 case 暂挂 README 的 TODO 表

## 卡片与 spec 的映射

每张卡片对应一个 wdio spec 文件，路径约定：

```text
e2e/cards/<card-id>.md     卡片本身（契约 / 给人读 / 给归因 agent 读）
e2e/specs/<card-id>.e2e.js wdio 实现（执行步骤 + 断言 selectors 命中）
```

`scripts/e2e/run-card.sh <card-id>` 同时校验两者存在再触发远端运行。

## 卡片如何被消费

```text
PR diff ──► agent 读所有卡片 modules ──► 求交集 ──► 候选卡片集
                                                       │
                                                       ▼
按 topology 启动 1 / 2 个 Tauri 实例（隔离 UC_PROFILE）
                                                       │
                                                       ▼
按"步骤"执行 + 按"断言"用 webdriver 读 selectors 判定
                                                       │
                          ┌────────────失败────────────┤
                          │                            │
                          ▼                            ▼
            抓 event_paths 对应日志             全部通过 → 报告
            + 已知失败模式喂给归因 agent
                          │
                          ▼
                生成 PR 评论（结论 + 嫌疑模块）
```

## 维护约定

- 卡片随代码演进而更新；但"断言"章节增减必须在 commit message 里说清
- 字段含义变了（如 `topology` 加新枚举值），先改 SCHEMA.md 再改卡片
- 卡片彻底失效（场景已不存在）时直接删除，不要留"已废弃"标记
