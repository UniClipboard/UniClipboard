# 桌面流畅模式实施规格

- 日期：2026-09-08。
- 状态：已实现三档选择、统一效果控制、原生能力初判和运行评估规则；完整跨平台实测仍需补齐。能力规则及当前证据见[校准记录](2026-09-08-adaptive-smooth-mode-calibration.md)，界面验证见[验收记录](2026-09-08-adaptive-smooth-mode-verification.md)。
- 需求：[GitHub issue #1622](https://github.com/UniClipboard/UniClipboard/issues/1622)。
- 需求修订：用户于 2026-09-08 明确 Linux 的自动模式默认采用流畅优先。本文按自动固定流畅、允许手动效果优先执行；此规则优先于原 issue 中所有系统均按能力初判的描述。
- 代码研究基线：`1861d0999`。
- 读者：能修改 React/TypeScript、编写基础 Rust 测试的初级开发者。
- 范围：当前设备的桌面界面偏好、视觉效果、轻量运行评估；不改 Engine、同步、传输或遥测。

## 1. 文档完成标准与实施方法

本文要让执行者逐片完成，不要求执行者自行决定状态所有权、系统偏好优先级、跨窗口一致性、失败处理或采样算法。第 8 节每片均列出入口、步骤、交付物、验收和停止条件。一次只做一片，通过后再进入下一片；提交粒度可小于切片。

文档完成与功能完成分开。自动模式先用明确的原生能力门槛选择初始效果，再由真实交互修正下一次启动结果；能力检测不等同于帧率保证。修复原实现始终返回 unknown 后，不再让缺少完整设备校准报告阻止有可靠能力信号的设备启用效果。S5/S7 仍负责补充代表设备和完整跨平台证据，不能用模拟输入冒充真机实测。

开始实施时依次读 `VISION.md`、`docs/agent/workflow-rules.md`、`docs/agent/frontend-ui-rules.md`、`src/AGENTS.md`；修改 Rust 前补读 `docs/agent/rust-tauri-rules.md`、`crates/AGENTS.md`、`src-tauri/AGENTS.md`。本文拟新增的路径会明确标为“新增”，不要误认为已有模块。

## 2. 研究依据与现状

### 2.1 已核实的代码事实

| 位置 | 当前行为 | 实施影响 |
| --- | --- | --- |
| `src/lib/platform.ts` | `reduceVisualEffects = isLinux || isWindows`；写入 `data-uc-low-effects` | 移除此处分散的效果判断；Linux 自动流畅规则移入统一策略，平台信息仍保留 |
| `src/lib/window-ui.ts` | 两种窗口共同使用的初始化入口，写平台标记并初始化缩放等 | 在此接入效果初始化及清理，不改变其他偏好 |
| `src/App.tsx`、`src/quick-panel/QuickPanelApp.tsx` | 各有 `LazyMotion`、`MotionConfig` | 分别接同一个状态服务；快捷面板的 `Toaster` 也须放入效果控制范围 |
| `src/components/setting/AppearanceSection.tsx` | 业务外观设置通过 `useSetting`，已有设备本地缩放与窗口边框设置 | 新增独立设置行，不给 Engine 设置对象加字段 |
| `src/lib/ui-scale.ts`、`src/lib/window-frame.ts` | 本地存储、当前窗口事件和 `storage` 事件 | 可参考订阅方式，但不足以定义一次 GUI 启动的统一自动结果 |
| `src/styles/globals.css` | 全局关闭 CSS 动画、过渡、背景模糊；清除阴影变量，玻璃样式变不透明 | 不保证停止 Motion/命令式动画；须审核必要加载反馈 |
| `src/components/motion/center-morph-modal.css` | 已响应低效果标记 | 保留弹窗布局和关闭语义，测试零时长退出 |
| `src/lib/theme-transition.ts` | 读取低效果标记；另有 `documentElement.animate()` | 切换模式时应结束已启动效果，保留最终主题状态 |
| `src/components/motion/input.tsx` | 有命令式 `animate()` 错误抖动 | 公共控制需覆盖取消、复位与静态错误提示 |
| `src-tauri/crates/uc-tauri/src/run.rs`、`specta_builder.rs` | 管理桌面状态、集中注册类型化命令 | 新服务归此壳层；注意不是 `crates/uc-tauri/` |
| `Cargo.lock` | 已锁定 `sysinfo 0.38.4`，`uc-tauri` 尚未直接依赖 | 原生 CPU/内存采集优先复用同版本，不再引入完整系统监控栈 |
| `src/updater/main.tsx` | 另一个独立界面入口 | 纳入效果覆盖审计；不改更新业务 |

本地安装的 Motion 代码也已核实：`useReducedMotion()` 在 `useState` 初始化时读取系统值，并不读取本产品的用户选择；不能把替换根部属性当成所有已挂载组件都会立即更新的证据。实施时以锁定版本的源码和运行测试为准。

### 2.2 外部资料与采用结论

- [VS Code 1.66](https://code.visualstudio.com/updates/v1_66)：成熟桌面产品提供自动、开启、关闭三态。借鉴三态交互，不照搬其系统偏好覆盖规则，也不声称它提供硬件自适应。
- [Motion 无障碍指南](https://motion.dev/docs/react-accessibility)：根配置减少位移和布局动画，透明度等仍可能动画。因此还需共享动画层控制。
- [Tauri 消息机制](https://tauri.app/concept/inter-process-communication/)：命令用于取值和修改，事件用于状态通知；事件不是持久化事实来源。
- [sysinfo](https://docs.rs/sysinfo/0.38.4/sysinfo/)：用按需刷新读取 CPU/内存，不调用包含进程枚举的全量刷新。GPU 与实际 WebView 合成性能不能从核心数推断。
- [MDN requestAnimationFrame](https://developer.mozilla.org/en-US/docs/Web/API/Window/requestAnimationFrame)、[Page Visibility](https://developer.mozilla.org/en-US/docs/Web/API/Page_Visibility_API)：刷新率不同，隐藏窗口可能暂停回调；恢复后的长间隔不能算卡顿。
- [WebView2 性能指南](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)：浏览器、渲染器、GPU 是不同进程。仅测 GUI 主进程 CPU 不足以判断监测开销。

上述资料支持边界判断，不提供本产品硬件阈值。以下数值是可复现的初始实验参数，需 S5 校准。

## 3. 产品规则：执行者不再自行选择

### 3.1 三态与系统偏好

外观设置增加“流畅模式”单选组，三个并排选项为“自动 / 效果优先 / 流畅优先”，默认自动。键盘方向键选项、空格确认；窄窗口文字可换行，不截断。可复用项目已有单选控件，不新装动画控件库。

| 用户选择 | 系统要求减少动效 | 当前结果 |
| --- | --- | --- |
| 自动 | 是 | 流畅 |
| 自动 | 否 | Linux 固定流畅；macOS/Windows 使用本次 GUI 启动冻结的自动结果 |
| 效果优先 | 是 | 保留可用静态视觉效果，关闭非必要动画 |
| 效果优先 | 否 | 完整效果，不受自动评估覆盖 |
| 流畅优先 | 任意 | 流畅 |

内部输出分成 `reduceMotion` 和 `lowEffects` 两个布尔值。效果优先遇到系统减少动效时是 `true/false`；流畅是 `true/true`；完整效果是 `false/false`。不要用一个布尔值抹掉这一区别。

Linux 默认选中的仍是“自动”，不要把保存的用户选择改成“流畅优先”。其实际结果固定流畅，不随硬件能力或历史评估升级；手动选择“效果优先”立即生效，切回“自动”立即恢复流畅。Linux 不执行本功能的硬件探测和运行采样，手动效果优先也不采样。设置可显示“当前：流畅，Linux 自动模式默认减少视觉效果”。

系统减少动效变化立即响应，优先于三态选择；系统关闭该偏好后恢复本次冻结结果或手动选择。这是用户主动改变辅助功能偏好，不属于自动性能调整。系统信息无法读取时按减少动效处理，显示原因；默认值不能把“未知”当“否”。

设置行下方只展示实际结果和影响，例如“当前：流畅，已减少视觉效果”“当前：效果优先；已遵循系统减少动效设置”。保存失败提示“本次已生效，但未能保存，重启后可能恢复原设置”。中文与英文至少明确翻译，其他现有语言使用既有回退方式，不能显示翻译键。

### 3.2 生效时机

- 手动选择：命令确认后立即应用所有已打开窗口，禁止靠重挂载整个应用实现切换。
- 新开窗口、隐藏后重新显示、窗口重新加载：取同一 GUI 进程的当前快照，不重新决定自动结果。
- “下次启动”：GUI 进程重新启动，包括退出轻量模式后重新创建 GUI 进程；重启 daemon 不算。
- 自动监测只改下次启动候选结果。本次选择从手动切回自动时，仍读取本次冻结结果。
- 不把低效果模式用于改变 Linux 系统边框、不透明窗口表面、快捷面板定位或粘贴焦点策略；这些平台正确性规则独立保留。

### 3.3 保存范围与隐私

由 GUI 壳层服务唯一写入 `uc_app_paths::app_data_root()` 下新增 `visual-effects.json`，沿用既有 profile 与便携路径解析。这里的“当前设备”指本机当前应用配置；不同 `UC_PROFILE` 隔离。不得加入业务设置、设置导入导出或设备同步。

拟保存的只有模式枚举、评估版本、下一启动结果及粗能力分类。样本计数仅在当前进程内存中。设备型号、CPU/GPU 原始描述、逐帧数据、用户输入、时间戳轨迹均不保存、不上传、不打日志。

该文件是非用户内容的界面配置，本文提出有限的明文例外，不将现有 localStorage 习惯视为自动授权。按 `VISION.md`，S1 的 PR 必须逐字段说明非敏感性并获批后才能合入持久化实现；评审前可以实现和测试。若不获批，应由维护者确定锁屏前可用的加密配置方案，执行者不能连接 Engine 密钥或自行绕过规则。

## 4. 所有权、数据与消息协议

### 4.1 最小模块边界

| 新增模块 | 职责 |
| --- | --- |
| `src-tauri/crates/uc-tauri/src/visual_effects.rs` | 纯决策、进程级状态、版本与汇总；变大后才按职责拆子模块 |
| `src-tauri/crates/uc-tauri/src/visual_effects_storage.rs` | 有界读取、校验、原子替换配置文件 |
| `src-tauri/crates/uc-tauri/src/visual_effects_probe.rs` | 启动等待上限、结果映射与未知处理 |
| `crates/uc-desktop/src/visual_capabilities.rs` 及同名目录 | 框架无关的 CPU/内存读取、Metal/Direct3D 能力查询和联合分类 |
| `src-tauri/crates/uc-tauri/src/commands/visual_effects.rs` | 类型化命令，薄转发，不复制策略 |
| `src/api/visual-effects.ts` | 命令及事件封装、错误反馈 |
| `src/lib/visual-effects-store.ts` | 每窗口只读快照、订阅和重连；无磁盘写入、无硬件分类 |
| `src/hooks/useVisualEffects.ts` | 用 `useSyncExternalStore` 暴露稳定快照 |
| `src/components/motion/VisualEffectsProvider.tsx` | 根部动画配置和公共效果上下文 |
| `src/lib/visual-effects-sampler.ts` | 前台交互期间轻量采样，提交汇总，不决定模式 |
| `src/components/setting/SmoothModeSetting.tsx` | 单独一行设置及该行的保存反馈 |

已有 `platform.ts` 最终仅负责平台事实。迁移后删除 `PlatformInfo.reduceVisualEffects`；`isLowEffectsEnabled` 迁到效果模块。全局样式标记只能由效果状态入口写入。

### 4.2 逻辑数据合同

下面使用 TypeScript 表达协议；实现时先写 Rust DTO，再由 Specta 生成，禁止手改生成文件。

```ts
type VisualEffectsMode = 'auto' | 'effects' | 'smooth'
type AutoResult = 'effects' | 'smooth'
type EvidenceClass = 'capable' | 'constrained' | 'unknown'
type SystemMotion = 'reduce' | 'allow' | 'unknown'

interface VisualEffectsSnapshot {
  sessionId: string // GUI-process lifetime, never persisted
  revision: number // monotonically increasing within session
  mode: VisualEffectsMode
  autoForSession: AutoResult
  nextAuto: AutoResult | null
  systemMotion: SystemMotion
  reduceMotion: boolean
  lowEffects: boolean
  reason: 'manual' | 'system' | 'platform_default' | 'hardware' | 'unknown' | 'runtime'
  persistence: 'saved' | 'session_only'
}

interface StoredVisualEffectsV1 {
  schemaVersion: 1
  policyVersion: number
  mode: VisualEffectsMode
  nextAuto: AutoResult | null
  evidenceClass: EvidenceClass
}
```

`evidenceClass` 是粗分类，不是设备指纹。策略版本变化或当前粗分类不同则丢弃旧 `nextAuto`，保留手动选择。字段缺失的旧版本按默认补齐；未知未来版本只读、当前会话运行，不覆盖；损坏或超过 4 KiB 的配置按默认运行并显示不能保存，保留原文件供恢复。没有文件视为首次使用，可正常创建。

命令和事件约定：

| 命令或事件 | 输入与结果 |
| --- | --- |
| `get_visual_effects` | 无输入，返回快照 |
| `set_visual_effects_mode` | 仅 mode，返回权威快照；串行更新，按服务处理顺序后写生效 |
| `report_visual_effects_environment` | sessionId、systemMotion；窗口身份由 Tauri 注入，不信任前端标签 |
| `report_visual_effects_sample` | sessionId、窗口内单调 sampleId、采样起点 revision、有效帧数、总时长、长帧数、最长间隔；返回快照 |
| `visual-effects://changed` | 完整快照，只通知，不要求收到事件才能读取状态 |

前端采样区间与计数必须校验：整数计数非负、长帧数不超过总帧、时长有限且最多 2 秒；重复 sampleId、旧 sessionId、旧模式 revision、非自动或非完整效果时的报告丢弃。每窗口仅保留最新 sampleId，无无限列表。

窗口先完成事件订阅再读取快照，按 sessionId/revision 接收新值，丢弃迟到的旧响应。订阅失败仍取快照并在下次显示时重试；窗口显示前重新读，覆盖隐藏时丢失的事件。首屏快照未就绪时使用静态低效果，不能短暂闪现完整动画。服务启动失败也保持界面可操作并显示原因。

状态变更先串行更新内存，再尝试保存，再返回与广播快照；磁盘失败标为 `session_only`，继续向所有窗口应用。文件写入需同目录临时文件、刷新、跨平台原子替换；禁止先删旧文件再写新文件。S1 用 Windows 替换已有文件用例验证所选 API；若需库，优先现有依赖，新增需记录理由。

系统偏好优先用每个 WebView 的 `matchMedia('(prefers-reduced-motion: reduce)')` 初始值及 change 事件。窗口值冲突时 GUI 服务取仍存活窗口报告的保守并集：reduce 优先，其次 unknown，最后 allow。销毁窗口删除报告；显示前刷新报告。新窗口上报前不改变已确定值，但自身静态启动。这样所有窗口最终使用同一输出。

## 5. 自动初判与运行修正

### 5.1 初次设备判断

Linux 直接冻结流畅结果，跳过原生探测。macOS/Windows 在 GUI 启动阶段后台执行一次探测；等待上限为 2 秒，超时本次取 unknown，迟到结果不得改变本次自动结果。首次原生检测在 M4 上曾达到约 723 ms，因此原来 500 ms 的上限过短。等待期间使用现有静态首屏，不阻塞界面主线程。通过 `sysinfo 0.38.4` 按需读取物理核数和物理内存，不刷新进程列表或 CPU 使用率；原始信息只留在本次调用中。禁止 `System::new_all()`、命令行探测和依赖 `navigator.deviceMemory`。

初始资源门槛是至少 4 个物理核心、8 GiB 物理内存，同时具备现代硬件图形能力。macOS 使用 `MTLCopyAllDevices` 枚举 Metal 设备，要求每个可用设备均支持 `MTLGPUFamilyMac2`，避免只看最强独显，也不为了探测切换独显。Windows 用 `D3D11CreateDevice` 的 HARDWARE 驱动验证默认适配器，要求 feature level 至少 12_0；旧运行时拒绝新特性级别时只重试旧硬件级别，不回退 WARP 软件渲染。原生图形能力代表设备支持范围，不声称当前 WebView 必定使用该设备；实际表现仍由后续交互采样验证。不通过型号名称、厂商字符串或操作系统名称推断性能。

冻结 `autoForSession` 的顺序：Linux → 流畅，忽略历史 nextAuto；其余支持平台：有效的下次结果 → 采用；设备 constrained/unknown → 流畅；设备 capable → 完整效果。Linux 快照的 nextAuto 为 null、原因为 platform_default（系统偏好或手动选择覆盖时使用对应原因）。系统偏好在其外层按第 3 节即时覆盖，不写进冻结值；否则启动时的系统减少动效会在关闭系统偏好后仍然错误保留。手动模式按第 3 节处理。macOS/Windows 的 `nextAuto` 是持续保存的启动覆盖值，读取不清空；不然降级只持续一次启动。

分类顺序：任一已知资源低于门槛或 GPU 只支持旧能力，判为 constrained；否则，物理核数/内存无法读取、值为零或图形能力无法获取，判为 unknown；三个条件均满足才是 capable。这是保守初判，不是由核心数或内存单项推断实际帧率。策略版本升为 2，升级时保留手动选择，清除旧策略的自动覆盖值。Linux 自动流畅不属于检测失败。

### 5.2 轻量采样的固定规则

仅自动模式且本次实际完整效果时采样。一次合格交互由根部可信 `pointerdown`、`keydown` 或 `wheel` 启动，监听使用 passive/capture 的适用组合，不读取输入内容。每次最多 2 秒，间隔至少 30 秒；每 GUI 会话最多 10 次，每次结束只报一条汇总。

启动后至少等待 10 秒且页面业务加载完成；主窗口用就绪门控，快捷面板在 `onShowPrepared` 后门控。采样期间要求 document 可见、原生窗口可见且未最小化、最近有可信交互。快捷面板是 nonactivating panel，不能用 `document.hasFocus()` 或 Tauri focused 作为唯一门槛。隐藏、最小化、休眠、重新加载或就绪状态丢失时丢弃整段，取消 rAF；恢复后重新等待 2 秒。

初始测试参数：至少 30 个有效间隔，帧间隔大于 50 ms 算长帧；长帧比例达到 20% 且至少 6 个算一次差样本。固定 50 ms 表示严重停顿，不声称测量刷新率的漏帧比例；30 Hz/60 Hz/120 Hz 输入均要测试。最长间隔大于 500 ms 的整段丢弃，避免休眠/调试暂停污染。此规则不能判断所有卡顿，局限写进报告。

GUI 服务串行授予一次采样许可，确保两个窗口不会重复采样；许可最多 2 秒、每次有唯一编号，报告必须匹配许可及采样 revision。上述命令表在 S4 增加 `begin_visual_effects_sample`，返回许可或拒绝；拒绝时前端不创建 rAF。交互事件不逐帧跨进程发送。

3 次独立差样本（至少间隔 30 秒）才设置 `nextAuto = smooth`，立即保存，下次 GUI 启动生效。一次差样本不变；正常样本不清空已积累的差样本；会话结束即清空内存样本计数，因此证据必须在同一次 GUI 会话达到门槛。切换手动、系统减少动效、硬件分类或策略版本改变时清空采样计数并结束许可。

### 5.3 恢复规则，避免错误推断

在流畅模式下采到正常帧，不能证明完整效果也流畅。第一版不凭流畅模式下的样本自动恢复，也不偷偷恢复一段完整效果做探测。

因运行表现降级后持续保留流畅结果；只有策略版本或粗能力分类变化，才清除旧降级结果并重新初判。当前分类必须是 capable 才可能重新启用完整效果。原来就是 capable 而运行降级的机器继续流畅，用户始终可以选择效果优先。不增加无助于决策的启动计数或冷却配置。

同一粗分类下的硬件升级不会自动识别，手动效果优先是这一版本的明确恢复途径。自动恢复完整效果的进一步试探不在本规格内；修改此行为须修订规格及测试，不能让执行者现场推断。

## 6. 统一效果覆盖合同

`VisualEffectsProvider` 读取统一状态；公共动画入口生成即时终态所需的 transition/layout/gesture 配置。业务页面继续只传目标状态，禁止硬件或模式条件散落到页面。

| 效果来源 | reduceMotion=true 的处理 | lowEffects=true 的附加处理 |
| --- | --- | --- |
| Motion 位移、透明度、颜色、尺寸、布局、退出、hover/tap | 公共封装明确零时长、零延迟，禁用布局插值；目标值仍照常应用 | 同左 |
| 公共组件内部 transition、variant、spring | 统一 helper 覆盖，不能被局部 spring 覆写回去 | 同左 |
| 命令式 `animate()`、Web Animations、主题切换 | 保存控制句柄；切换时结束到正确终态，清理句柄 | 同左 |
| CSS 动画/过渡/平滑滚动 | 立即切换到目标状态 | 同左 |
| blur、backdrop-filter、阴影、半透明背景 | 可保留静态效果 | 公共面板用不透明语义色，关重阴影和模糊 |
| 加载、进度、错误、选中、焦点 | 保留文字、进度值、轮廓等静态反馈，不能只剩冻结的旋转图标 | 同左 |

不得全局清除 transform、opacity 或 pointer-events；它们可能承担定位、显隐与交互职责。不得依赖 `animationend` 才执行业务关闭；零时长仍要完成退出、解除 inert、归还焦点。不能重挂载整个 Router/页面以更新 Motion 配置，否则输入和焦点丢失。

必须审计的现有公共组件：`ui/switch.tsx`、`ui/selection-indicator.tsx`、`ui/popover.tsx`、`motion/input.tsx`、`motion/action-swap.tsx`、`motion/expandable-action-bar.tsx`、`motion/animated-toast-item.tsx`、`motion/menu/MenuHighlight.tsx`、`motion/context-menu/root.tsx`、`motion/select/SelectTrigger.tsx`、`motion/select/SelectContent.tsx`、`motion/center-morph-modal.css`。统一替换其直接 `useReducedMotion` 读取；对剩余直接 Motion 使用通过审计清单逐项核实，不能假设清单穷尽所有动画。

## 7. 测试矩阵与可执行检查

| 编号 | 输入或操作 | 必须结果 |
| --- | --- | --- |
| P01 | 首次启动、无配置 | 自动；能力未知时流畅 |
| P02 | capable/constrained/unknown，三种系统各一套相同输入 | macOS/Windows 同输入同结果；Linux 自动始终流畅，探测和采样调用次数均为零 |
| P02L | Linux 自动 → 效果优先 → 自动，重启并注入历史 nextAuto=effects | 手动效果优先生效且遵循系统减少动效；切回自动和重启均流畅，不采用历史完整效果结果 |
| P03 | 全部模式 × reduce/allow/unknown | 严格符合第 3 节表格 |
| P04 | 手动选择后收到差样本或迟到探测 | 手动值不变 |
| P05 | 1、2、3 个差样本 | 前两次不变；第三次仅 nextAuto 改变 |
| P06 | 重开面板、刷新 WebView、重启 daemon、重启 GUI | 只有最后一种消费 nextAuto |
| P07 | 旧 revision、重复 sampleId、旧 session、事件早于读取返回 | 不回滚、不重复计数 |
| P08 | 缺文件、损坏、超大、未来版本、目录不可写、原子替换失败 | 不崩溃；保留旧文件；明确会话生效/无法保存 |
| P09 | 后台、最小化、启动、休眠恢复、非激活快捷面板真实输入 | 前四种不计；后者能合法采样 |
| P10 | 30/60/120 Hz 正常帧、单次 100 ms、重复长帧、超过 500 ms 暂停 | 正常及单次不降级；重复满足门槛才降级；暂停段丢弃 |
| P11 | 两窗口同时操作、事件发送失败、隐藏后显示 | 服务唯一顺序；显示重取后恢复一致 |
| P12 | 流畅期间帧正常、多次重启、策略更新 | 不误恢复；只有规定条件重新初判 |

真实界面每个场景都测完整效果、流畅、系统减少动效，覆盖浅色和深色：搜索展开/收起与中文输入；弹窗打开/关闭/Escape/焦点返回；开关按钮；页面导航；长列表滚动与选择；菜单及子菜单键盘操作；快捷面板显示/隐藏/搜索/粘贴焦点；加载与传输进度、错误提示；主题切换中改变模式。切换前输入内容、滚动位置、选中项须保留。

新增测试文件以 `visual-effects` 命名，便于独立执行：

```bash
npx vitest run src/lib/__tests__/visual-effects-store.test.ts src/lib/__tests__/visual-effects-sampler.test.ts
npx vitest run src/components/setting/__tests__/SmoothModeSetting.test.tsx
cargo test -p uc-tauri visual_effects
cargo test -p uc-tauri --test specta_export
bun run lint
bun run build
bun run doctor
git diff --check
```

上述是实施后目标命令，当前新增测试尚不存在。Rust 命令运行位置遵循 `docs/agent/rust-tauri-rules.md`；执行前检查本机 Cargo 配置是否注入外部工作区路径。组件测试不能证明 WebView 动画或跨窗口正确，S7 必须运行实际桌面程序。

## 8. 按切片实施

### S1：用户能切换并保存模式，主窗口立即变化

- 依赖：无；先读第 3、4、6 节。先实现一条从设置到实际显示的闭环。
- 修改：新增模式服务、存储、命令、API/store/hook/provider、`SmoothModeSetting.tsx`；注册到 `run.rs`、`specta_builder.rs`、`commands/mod.rs`；接入 `App.tsx`、`AppearanceSection.tsx` 和共享 CSS。
- 步骤：先写 P01/P02L/P03/P08 纯规则和存储测试；实现 Linux 自动固定流畅、其余平台未知能力默认流畅；添加三档选择和结果文案；连接主窗口根部；生成接口；更新中英文翻译。这片 macOS/Windows 自动模式只使用 unknown，明确是中间交付状态。
- 交付：主窗口手动切换影响一个真实共享效果（例如玻璃背景），重启保留；服务支持完整合同，后续直接填探测与评估。
- 验收：选择、键盘操作、重启、存储失败及系统偏好变化通过；文字与内容位置不跳动。
- 停止条件：失败反馈缺失或持久化会损坏原文件则修复；明文例外评审未通过不得合入该片持久化变更。

### S2：主窗口、快捷面板和独立窗口一致

- 依赖：S1。
- 修改：`window-ui.ts`、两个 `main.tsx`、`QuickPanelApp.tsx`、独立更新窗口入口及服务订阅。
- 步骤：实现先订阅后读取、revision 防回滚、显示前重取、系统报告并集；把快捷面板 Toaster 纳入 provider；实现初始化与销毁清理；原生服务按 GUI 生命周期只创建一次。
- 交付：双窗口真实切换演示和竞态测试；未就绪时静态首屏。
- 验收：P06/P07/P11；隐藏面板后切换、再显示；刷新窗口不改变自动结果；更新窗口不受意外重挂载影响。
- 停止条件：需用户重开窗口才能应用或丢失设置输入，修复后再往下。

### S3：流畅模式完整覆盖公共效果，操作保持正常

- 依赖：S2。
- 修改：第 6 节公共组件、主题切换、全局样式及新增公共 transition helper。
- 步骤：执行下列搜索形成清单；逐项记录来源、公共控制方式和验证场景；替换系统 hook；处理局部 transition 优先级和进行中动画；保留静态加载/错误反馈。只调整确实绕过公共控制的共享组件。
- 交付：新增 `docs/specs/2026-09-08-adaptive-smooth-mode-effects-audit.md`，每行对应一个效果来源，列出证据；本规格中链接该报告。
- 验收：第 7 节真实界面场景；至少证明已挂载的开关、弹窗、搜索在切换后立即停止非必要动画；结束时焦点和坐标正确。
- 停止条件：任何残余非必要动画、关闭卡住或反馈消失必须先修复。

```bash
rg -n 'useReducedMotion|MotionConfig|animate\(|layoutId|whileHover|whileTap|transition=' src
rg -n '@keyframes|animation:|transition:|backdrop|blur\(|box-shadow|drop-shadow' src
rg -n 'reduceVisualEffects|ucLowEffects|isLowEffectsEnabled' src
```

### S4：重复卡顿只影响下次启动

- 依赖：S3；测试注入 capable，生产初判仍保守。
- 修改：sampler、服务采样许可/汇总、窗口就绪与可见性接入。
- 步骤：先用可控时钟和预制帧间隔写 P04/P05/P09/P10/P12；实现可信交互触发、许可、限额、取消与汇总；写入 nextAuto；实现持续降级与恢复规则。切换模式立即终止已有采样。
- 交付：可重复的状态测试；测试专用能力输入不得在正式发布入口暴露。
- 验收：三次坏样本后当前画面不变；GUI 重启后两个窗口流畅；手动模式不被覆盖；隐藏没有持续 rAF 或定时轮询。
- 停止条件：单次卡顿降级、同次启动切效果、两窗口重复计数任一出现即失败。

### S5：设备探测与阈值校准

- 依赖：S3、S4；需要代表性较弱/较强设备或维护者提供可运行测试的机器。
- 修改：probe、唯一策略参数表、测试能力样本；按需增加同版本 sysinfo 直接依赖。
- 步骤：验证第 5.1 节的原生能力读取及 2 秒等待上限，记录能力结果和真实运行表现的差别；Linux 验证跳过探测。用同版本正式构建在每台设备运行相同 200 条合成历史的搜索、弹窗、菜单、滚动和快捷面板操作，各重复 5 轮。合成数据不得使用个人历史。
- macOS/Windows 每台测三组：完整效果、流畅、完整效果加监测；Linux 测自动流畅和手动效果优先两组，并确认没有产品采样。记录 WebView 版本、屏幕刷新率、缩放、供电状态、前台操作延迟和帧间隔。硬件资料仅经测试者明确同意放入脱敏报告，产品自身不记录。
- 校准质量门槛：完整效果每轮长帧比例低于 5%、操作到可见反馈 p95 不超过 100 ms 作为流畅样本；超过 20% 长帧作为明显受限样本。用这些实测结果调整第 5.1 节的初判规则，不把 API 能力级别当作已测帧率，不使用型号白名单。
- 开销门槛：监测回调累计耗时不超过采样时长 1%；包含 WebView 子进程的 CPU 相比不监测增加不超过 1 个百分点；隐藏时采样回调为零。低效果改善应在差机器上可重复观察，不能只比较平均数。
- 交付：新增 `docs/specs/2026-09-08-adaptive-smooth-mode-calibration.md`，含逐轮原始汇总、能力可得性矩阵、最后规则、阈值修改理由和留出轮次验证。不得只写“已实测”。
- 验收：P02/P02L；macOS/Windows 强机器自动完整、弱机器自动流畅、未知有说明；Linux 强弱机器自动均流畅且手动可覆盖；所有平台编译及各自适用的能力或跳过探测测试通过。
- 停止条件：拿不到图形能力或代表设备时明确报告缺项，保留 unknown；不能宣布自适应验收完成，也不能让初级执行者自行拍定阈值。

### S6：移除旧判断并对齐项目文档

- 依赖：S5 的生产规则冻结；S3 的效果审计通过。
- 修改：`platform.ts`、`usePlatform.ts`、旧平台测试、`window-ui.ts`、`VISION.md` 对应视觉适配条目、相关用户说明。
- 步骤：删除平台模块中选择低效果的旧字段和写入路径，将 Linux 自动流畅规则保留在唯一效果策略中；平台排版/窗口安全规则保留；将 VISION 中对应条目改为“Linux 自动模式默认流畅；macOS/Windows 自动模式按设备能力选择；所有平台允许手动选择并尊重系统减少动效，平台窗口正确性约束独立保留”；更新根配置使用方和测试。
- 交付：唯一效果入口；清单内没有未解释的模式分支。
- 验收：搜索旧字段仅剩已说明的历史文档；不再有平台模块写 `ucLowEffects`；全部相关测试、构建和 React Doctor 无新增问题。
- 停止条件：仍存在第二个效果事实来源，先收敛再验收。

### S7：逐项实际验收并交付

- 依赖：S1–S6。
- 修改：仅补必要修复及新增 `docs/specs/2026-09-08-adaptive-smooth-mode-verification.md`。
- 步骤：按 P01–P12 与界面矩阵逐条运行；真实 macOS/Windows/Linux 上检查双窗口；记录使用的构建提交、命令、平台、通过/失败和截图；修复后重测受影响项目。
- 交付：验收报告链接回本文和 issue，列出效果审计、校准报告、实际构建来源和剩余限制。
- 验收：issue 每一条验收条件（含本文记录的 Linux 最新修订）对应至少一条证据；所有必须项通过，未执行不计通过；macOS/Windows 监测开销满足 S5 门槛，Linux 不运行探测和采样。
- 停止条件：任何必测平台、强弱设备或交互缺证据，功能状态保持“验收未完成”。不以文档已写好替代运行证明。

## 9. 实施前检查清单

- [ ] S1 的非敏感配置明文例外在合入前获批。
- [ ] 三态、系统优先级、立即/下次启动、未知情况有明确测试。
- [ ] 所有模式写入仅通过 GUI 服务，前端没有第二份持久化偏好。
- [ ] 原生探测不拉入 Engine，不枚举无关数据。
- [ ] S3 审计覆盖 CSS、Motion、命令式动画和必要反馈。
- [ ] S5 校准报告冻结生产阈值，不能使用“四核/四吉”口头假设。
- [ ] S7 区分真机、模拟输入、未执行；不虚构代表设备结果。

实施进展见验收记录。复选框为最终交付门槛，不因已有代码而自动勾选。
