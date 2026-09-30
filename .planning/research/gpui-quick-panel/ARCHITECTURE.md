# GPUI 快捷面板代码架构

日期：2026-09-30
状态：已实施（未做实机验收，见末尾）

## 目标

macOS 版本已经可用，之后要增加 Windows 与 Linux。代码结构必须满足：

- 业务规则可以脱离 GPUI、AppKit 与真实守护进程做单元测试；
- 新增快捷键、筛选维度、平台时，改动点集中且可预期；
- 依赖方向由编译器约束，而不是靠约定；
- 平台差异只出现在一处，且"不支持"必须显式表达。

## 现状问题

| 问题 | 证据 |
|---|---|
| `Panel` 是 God Object | 近 50 个字段，混合查询状态、预览、操作菜单、建议、重连、窗口位置与异步任务句柄 |
| 文件拆分不等于边界 | `panel/history.rs`、`panel/view.rs` 都是 `impl Panel`，共享全部私有字段 |
| 状态重置靠手写 | `toggle`、`dismiss`、`drop_content` 各自重置不同的字段集合 |
| 业务规则埋在闭包里 | 搜索的锁定/断连迁移写在 `spawn_in` 闭包中；`key_down` 是约 150 行 if 链 |
| 后端不可替换 | `backend.rs` 是自由函数加全局连接缓存，`Panel` 直接调用 |
| 平台层是 cfg 堆叠 | `platform.rs` 每个函数两份 `#[cfg]`，非 macOS 为静默空实现 |

## 目标结构

依赖只向内：`app → ui → core`，`app → adapters/platform → core`。`core` 不依赖 gpui、objc，也不启动异步运行时（只使用 tokio 的 `sync` 通道类型）。

```text
crates/quick-panel-core/          # 纯逻辑，可在任意平台编译与测试
└─ src/
   ├─ query/        filters、date_range（搜索条件、chip、建议、时间范围）
   ├─ content.rs    条目分类、页眉文案、命中范围
   ├─ actions.rs    操作菜单的行与游标
   ├─ grid.rs  selection.rs  empty_page.rs  language.rs
   ├─ geometry/     window_pair、image_geometry
   ├─ text.rs       用户可见文案（含各类错误的显示文本）
   ├─ ports/        HistoryService、PasteTarget、Capabilities、类型化错误
   └─ state/        PanelState 及子状态、Effect / Event、时序常量、按键映射（keys.rs）

apps/quick-panel/
└─ src/
   ├─ main.rs           入口，仅调用 app::run
   ├─ app/              装配根：runtime、触发器（hotkey、double_tap）、开窗、父进程监视
   ├─ adapters/daemon/  HistoryService 的守护进程实现（连接、搜索、条目、预览、实时事件）
   ├─ adapters/host.rs  向监督它的 GUI 发请求
   ├─ platform/         唯一存放 cfg 的目录：macos/、fallback.rs
   └─ ui/               GPUI 视图；mod.rs（Panel 薄壳）、effects.rs（Effect 执行器）、
                        keyboard.rs、intents.rs、windows.rs、history/（按区块分文件）、view、preview_window、image_preview
```

### 数据流

```text
按键 / 点击 ─▶ PanelState 的方法 ─▶ Vec<Effect> ─ui 执行器─▶ ports / platform
                     ▲                                            │
                     └──────────── on_event(Event) ◀──────────────┘
```

- `PanelState` 只保存业务状态，方法同步执行、不读时钟（时间由 `Ctx` 传入），返回需要执行的 `Effect`。按键规则在 `state/keys.rs`：`on_key` 返回是否已被面板处理，未处理的按键继续交给搜索框。
- `Panel`（GPUI 实体）持有 `PanelState`、输入框、滚动句柄、任务句柄和窗口几何，负责把 `Effect` 变成真实操作，再把结果作为 `Event` 送回。
- 几何信息（`ScrollHandle`、窗口边界）留在 UI 层，state 只保存 `PreviewAnchor` 这样的纯值。

### 状态切分与重置

三个入口（显示、隐藏、内容锁定）清理的字段集合不同，不能合并成一个 `reset()`。它们分别是 `show`、`on_hidden`、`drop_content`，各自的清理范围与顺序由 `state/tests.rs` 固定。

### 平台层

- 窗口外观（显示、定位、形状）与屏幕几何留在 app 的 `platform/`，因为需要 `gpui::Window`。
- 焦点目标与按键注入抽象为 core 的 `PasteTarget` trait，可以被 fake。
- `Capabilities` 显式声明每个平台支持什么，界面据此降级；不支持的能力返回 `Unsupported`，不再静默成功。
- `clear_action` 之类按键层面的平台差异属于输入映射（`ui/keyboard.rs`），不进 `platform/`。
- 非 macOS 的提示文案会变化：原先各处的"尚未实现"中文提示统一为"此平台暂不支持该功能。"。这些平台尚未发布，macOS 文案逐字不变。
- 没有自动粘贴的平台，回车只复制，操作菜单不列粘贴类操作，页脚显示"复制"（`state` 里用 `Capabilities` 判断）。

## 必须保持不变的行为

这些时序由 `tests/e2e.mjs` 等脚本与实机验证，重构时作为具名常量原样搬迁：

- 搜索防抖 300 ms；实时事件合并 120 ms，合并时锁定事件优先；
- 预览延迟：已展开 120 ms，未展开 500 ms；
- 粘贴前等待 80 ms；保持面板打开时，粘贴后 150 ms 取回面板，并有 800 ms 失焦宽限；
- 粘贴失败：先重新唤起面板，再显示错误信息；
- 全部中文文案逐字不变。

## 迁移记录

按下面的顺序完成，每一步都保持编译与测试通过：

1. 建立 `quick-panel-core`，搬迁纯逻辑模块（测试数不变）。
2. `platform/` 拆成 `macos/` 与 `fallback.rs`，加入 `Capabilities` 与类型化错误。
3. `backend.rs` 拆为 `adapters/daemon/*`，实现 `HistoryService`，`Panel` 通过 trait 注入；连接缓存由全局静态变量改为适配器的字段，测试不再需要子进程。
4. 抽出 `state/`，覆盖搜索、预览、操作菜单、建议、会话、粘贴流程与按键，并补 44 个状态机测试。
5. `Panel` 改为薄壳，`effects.rs` 执行 Effect，异步结果以 Event 回到状态机。
6. 目录整理为 `app/`、`adapters/`、`platform/`、`ui/`。
7. 在 `x86_64-unknown-linux-gnu` 上编译 core，确认没有夹带 macOS 依赖。

## 新增功能时的改动点

| 场景 | 改动 |
|---|---|
| 新快捷键 | `state/keys.rs` 的 `on_key` 加分支，补状态机测试 |
| 新筛选维度 | `core/query`，UI 只渲染 |
| 新平台 | 新建 `platform/<os>/`，在 `platform/mod.rs` 加一行选择，声明 `Capabilities` |
| 测试断连重连 | fake `HistoryService` 加 `PanelState::on_event` |

## 验证边界

已验证：`cargo test`（core 133 个、应用 12 个，1 个需要真实后台的测试仍为 ignored）、`cargo clippy -D warnings`、`cargo fmt --check`、core 在 `x86_64-unknown-linux-gnu` 上编译、应用二进制启动后无 panic（无后台时初次搜索与选项加载按预期失败）。`platform/fallback.rs` 通过临时反转 `platform/mod.rs` 的 cfg，在 macOS 上用 `clippy -D warnings` 检查过（只覆盖 `platform/` 的实现选择，`ui/keyboard.rs` 里非 macOS 的 `clear_action` 分支没有覆盖到）。

crate 名：应用为 `quick-panel`（目录 `apps/quick-panel`，可执行文件仍为 `uniclip-quick-panel`），核心为 `quick-panel-core`（目录 `crates/quick-panel-core`）。

未验证，需要在可见、已解锁的桌面上执行：

- `apps/quick-panel/tests/e2e.mjs` 与 Python 校验脚本（依赖 peekaboo 与真实输入）；
- 粘贴到原应用、失焦关闭、重复唤起、保持面板粘贴；
- 输入法组合输入；
- 预览窗口的锚点、图片缩放。

时序（防抖、预览延迟、粘贴延迟、失焦宽限）作为具名常量保存在 `state::timing`，数值与重构前一致。
