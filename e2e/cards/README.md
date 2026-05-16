# e2e 卡片索引

真机 e2e 场景库。每张卡片是一个独立可重放的场景，由编排器读取并执行，由归因 agent 在失败时定位嫌疑模块。

格式约定见 [SCHEMA.md](./SCHEMA.md)。

## 当前卡片

| ID | Topology | Runtime | 关键模块 | 首批来源 |
|----|----------|---------|----------|----------|
| [pairing-delivery-badge-realtime](./pairing-delivery-badge-realtime.md) | dual-device | linux, windows | `EntryDeliveryBadge`, `apply_inbound`, `host_event_bus` | #749 |
| [delivery-bus-unregister](./delivery-bus-unregister.md) | in-process-stack | linux, windows | `host_event_bus`, `daemon/app.rs`, `assembly.rs` | #749 |
| [daemon-ws-delivery-skip](./daemon-ws-delivery-skip.md) | daemon-only | linux, windows | `uc-webserver/event_emitter`, `host_event_bus` | #749 |
| [quick-panel-delivery-badge](./quick-panel-delivery-badge.md) | single | linux, windows | `ClipboardPreviewPane`, `EntryDeliveryBadge` | #746 |
| [update-dialog-bg-download](./update-dialog-bg-download.md) | single | linux, windows | `Sidebar.tsx`, `UpdateContext` | #743 |

## 待补卡片（TODO）

| 来源 PR | 阻塞原因 |
|---------|----------|
| #748 macOS Finder 图片摄取 | 三条手动场景全部依赖 macOS Finder 复制；tauri-driver 不支持 macOS，需先确定 macOS 自动化方案（AppleScript / pasteboard CLI / 替代 driver）再翻译为卡片 |

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

## 给卡片维护者

- 卡片是 living document：代码演进时一并更新
- "断言"章节增减必须在 commit message 里说清
- 字段含义变了（如 `topology` 加新枚举值），先改 [SCHEMA.md](./SCHEMA.md) 再改卡片
- 卡片彻底失效（场景已不存在）时直接删除，不要留"已废弃"标记
- 新写卡片时，selectors 里可以填 **尚未存在** 的 testid——这是反向契约，第一次执行失败时把 testid 补进组件，而不是改卡片去迁就当前 DOM
