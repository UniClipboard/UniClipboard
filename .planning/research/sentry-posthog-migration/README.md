# 观测接入整理与 PostHog 迁移参考

核对日期：2026-09-06。用户已调整方向：保留 Sentry，先隔离厂商依赖；不执行下面历史盘点中的 Sentry 删除计划。

## 当前实施范围

- 前端统一从 `src/observability/diagnostics.ts` 调用诊断能力，`types.ts` 定义应用自己的数据类型；`sentry.ts` 保持为唯一实际接入，不新增运行时厂商选择器。
- 错误、日志、操作上下文、反馈、启动、设置、页面错误保护、路由追踪及 Redux 集成都不再让业务调用方直接访问 Sentry SDK。现有设置门控、脱敏、采样、回放和发布上传保留。
- 操作结束必须指定本次操作，内部持有厂商对象；并发操作不再覆盖彼此。多个操作重叠时不猜测普通日志属于哪一个，命令自身的错误和操作记录仍保留明确编号。这不是跨前端、后台和 Engine 的完整分布式追踪实现。
- Rust 厂商接入已经位于 `crates/uc-bootstrap/src/observability/`，本次不修改 Rust、Engine、依赖或构建上传；也不安装 PostHog SDK、不连接 Jaeger。
- 新增源码边界测试，禁止业务代码重新直接导入 SDK 或私有接入模块；保留 Sentry 专用测试，并验证公共入口使用方、操作并发和反馈失败行为。

后续切换时，替换前端接入模块及对应组合入口，并单独迁移 Rust 接入和构建上传。以下盘点作为未来评估参考，不是当前待执行清单。

本次验证：15 个测试文件、85 项测试通过；类型检查、完整前端构建及 macOS 兼容性检查通过。浏览器加载了真实反馈组件并确认其在公共错误保护组件中显示，未提交外部反馈；成功等待与失败保留内容由组件测试验证。浏览器主应用无法获得 Tauri 后台连接，停留在初始化等待，因此不作为完整产品流程证明。React Doctor 给出的扫描结果不完整；所列提醒位于既有页面复杂度、状态和动画代码，没有指向新增诊断边界，不能据此宣称全仓评分通过。未做 Sentry 云端接收验证或 Rust 重编译。

## 结论与范围

原调研目标是移除 Sentry；Jaeger 用于开发诊断，未来可自部署；PostHog 承担产品分析和远程诊断。当前只建设可替换的接入边界，保留 Sentry。

PostHog 当前官方支持 React 错误、Rust 错误与 panic、日志、通用分布式追踪、操作回放和自定义调查反馈。通用追踪仍为 beta。最明确的不等价项是 Windows 原生调试符号：官方明确表示 PDB 暂不支持。

本次核对当前 Desktop 代码、锁定的 Engine 源码、新版 Engine 工作区及 PostHog 官方文档。没有读取 PostHog 项目数据，没有验证生产到达率，没有发送测试事件、上传符号或触发崩溃。文档中的“有实现”不代表已经完成设备端验收。

Desktop 当前锁定 Engine `229edc7fdf23cacc45b5d3516f29d37e2326b719`；本次检查的相邻 Engine 工作区 HEAD 为 `a82f566a`。后者的观测能力不能直接当成桌面已有能力。下文 Desktop 路径相对本仓根目录，`Engine:` 路径相对 Engine 仓根目录。

## 1. 当前实际运行路径

- 配对：`src/hooks/useSetupFlow.ts` → `src/api/daemon/setupV2.ts` → `src/api/daemon/generated-bridge.ts` → `crates/uc-webserver/src/api/v2/setup.rs` → `Engine::execute(Operation::JoinSpace)`。
- Tauri：`src/lib/ipc.ts` / `src/lib/tauri-command.ts` → `src-tauri/crates/uc-tauri/src/commands/`，负责实际需要本机窗口、系统等能力的调用。配对请求不必经过 Tauri。
- 配对结果：`src/hooks/useJoinAdmission.ts` 接收通知并重新查询；等待期间每 1000ms 查询一次；`useSetupFlow.ts` 再刷新设置状态并更新界面。邀请方 `src/components/device/AddDeviceDialog.tsx` 也根据通知重新确认配对完成。
- 产品事件：前端已有后台转发路径；`crates/uc-bootstrap/src/wiring/desktop_host.rs` 注入 `DesktopHostAnalytics`，`apps/daemon/src/daemon/host.rs` 初始化事件上下文。
- `crates/uc-bootstrap/src/wiring/analytics.rs::build_analytics_sink`：debug 构建使用 `StdoutSink`；release 有 `POSTHOG_PROJECT_KEY` 时使用 `PosthogSink`，否则为 Noop；外层保留产品分析开关。

历史记录中“PostHog 尚未注入”的情况已经不适用于当前代码。当前只能确认发送路径已经接好，不能据此证明远端实际收到。

## 2. Sentry 功能与替换矩阵

| 功能 | 当前证据 | PostHog 对应能力与迁移处理 |
| --- | --- | --- |
| 前端初始化、自动错误 | `src/main.tsx`、`src/observability/sentry.ts` | 官方 React 接入支持未处理错误和 Promise 拒绝捕获；需要保留启动期关闭规则及多窗口覆盖。 |
| 页面渲染失败保护 | `src/main.tsx` 的 `Sentry.ErrorBoundary` | 官方 `PostHogErrorBoundary` 可承接；保留错误时的可见提示。 |
| 主动报告错误、预期错误过滤 | `src/observability/errors.ts`、两条 IPC 包装路径 | 使用 `captureException`；密码错误等预期业务结果继续不报为故障。 |
| 用户主动反馈 | `src/components/feedback/FeedbackDialog.tsx` 的 `captureMessage` / `captureFeedback` | 可用 PostHog 自定义 survey 保留现有表单；反馈与具体错误关联需单独设计，不是直接改函数名。 |
| 前端日志 | `src/lib/logger.ts` 的 Pino → `Sentry.logger`，以及 console integration | 迁到统一日志入口，再发送 PostHog Logs；避免一条日志通过两条路径重复发送。 |
| 交互上下文 | `src/observability/breadcrumbs.ts`、各业务调用点 | 保留必要、脱敏的操作上下文；不能假定 Sentry breadcrumb 数据结构可直接迁移。 |
| Redux 状态附带 | `src/store/index.ts` 的 `createReduxEnhancer` | 本轮没有确认等价适配器；建议改为有限的诊断字段，不复制整份业务状态。 |
| 前端页面和动作耗时 | Router tracing、`src/observability/trace.ts` | 改为与厂商无关的 OpenTelemetry 记录；开发发 Jaeger，远程诊断可发 PostHog。 |
| 操作回放 | `src/observability/sentry.ts`：常规抽样为 0，错误抽样为 1；开关控制缓冲和停止 | PostHog 有回放与遮罩能力，但“出错前缓冲”、Tauri 各平台窗口和遮罩效果未验证，不声明完全等价。 |
| Rust 错误、panic | `crates/uc-bootstrap/src/observability/tracing.rs`、`Cargo.toml` 的 panic/backtrace/debug-images | 官方 `posthog-rs` 支持错误与可选 panic hook；需保留错误分类、脱敏、去重和有界退出。不能把普通 ERROR 日志一律转换成重复异常。 |
| Rust 日志和耗时 | `crates/uc-bootstrap/src/observability/tracing.rs` 的 Sentry layer | 统一进程采集，使用 OTLP 输出；不叠加两套全局初始化。 |
| 版本健康相关能力 | Sentry 依赖启用 `release-health`，发送出口考虑 session 等载荷 | 启用 feature 不等于当前正在完整统计无崩溃会话；本轮未确认 PostHog 的等价分母、退出判定或现有云端报表，不能承诺平移。 |
| 前端错误定位到源码 | `vite.config.ts` 的 Sentry 插件和 hidden sourcemap | PostHog 官方 Vite 插件支持；同一产物生成、注入、上传、发布须对齐。 |
| Rust 发布版错误定位 | `.github/workflows/build.yml`、`alpha-build.yml` 中 `sentry-cli debug-files upload` | PostHog 支持 Linux、macOS 调试符号；Windows PDB 官方暂不支持，是完整替换的明确差异。 |
| 用户设置、采样、过滤 | `sentry.ts`、`sentry_gate.rs`、`telemetry_gate.rs`、`redact.rs` | 保留其产品语义，替换厂商实现。关闭后不能仅停止创建新记录，还要阻止已排队数据继续发出。 |

对应官方资料：[React 错误](https://posthog.com/docs/error-tracking/installation/react)、[Rust 错误与 panic](https://posthog.com/docs/error-tracking/installation/rust)、[Rust 调试符号](https://posthog.com/docs/error-tracking/upload-source-maps/rust)、[Vite 源码映射](https://posthog.com/docs/error-tracking/upload-source-maps/vite)、[回放隐私](https://posthog.com/docs/session-replay/privacy)、[自定义调查](https://posthog.com/docs/surveys/implementing-custom-surveys)、[Rust 日志](https://posthog.com/docs/logs/installation/rust)、[通用追踪](https://posthog.com/docs/distributed-tracing)。

## 3. PostHog 能力边界

- 通用追踪使用 `/i/v1/traces`，日志使用 `/i/v1/logs`；不要误用仅面向 AI 的追踪入口。产品事件、异常、回放仍有各自的采集语义，统一关联不等于全部塞进 OTLP。
- 通用追踪当前为 beta。可纳入方案，但需实际验证项目可用性、数据到达、查询与关联；本次没有进入项目后台验收。
- Rust 官方文档目前要求支持服务器符号解析的 SDK 至少为 `posthog-rs 0.16.0`；macOS/Linux 的符号上传仍需与实际发布产物对应。Windows PDB 明确不支持，不能用“错误能收到”冒充“错误能准确定位到源码”。
- Rust panic 自动捕获为显式开启，发送是尽力而为；不能推导为支持所有原生崩溃、系统强杀或可靠保存崩溃转储。当前 Sentry panic 路径也不能自动当作覆盖这些情况的证据。
- React/Web 支持不等于 Tauri 的 WKWebView、WebView2 和 WebKitGTK 均已验收；主窗口、快捷面板、更新窗口需分别检查。
- 自定义反馈表单可使用 surveys；用户主动提交反馈与被动诊断上报需要分别定义行为，避免用户关闭使用统计后反馈入口失效。
- 本轮没有找到可直接等价替换 Redux 状态附带、Sentry 版本健康统计的充分官方证据。保留为验收差异，不声明 PostHog 永久不支持。

## 4. 追踪串联的现有断点

1. `src/observability/trace.ts` 用单一 `currentTrace` 保存操作，并用 `endTrace()` 结束当前操作；并发任务可能覆盖彼此。应改为每次调用持有自己的记录。
2. Tauri `TraceMetadata` 只有 `trace_id` 与时间；`record_trace_fields` 只写普通字段，没有承接分布式父关系。日志字段名相同不是追踪已经贯通。
3. 后台请求中间件自行生成 `request_id`；未提取前端追踪信息；跨域请求当前仅允许 `authorization, content-type`。
4. 新版 Engine 的 `SpaceAdmissionObservation::begin` 显式 `parent: None`。配对在后台延续时需保留首次操作的关联；恢复后应开新记录并关联原流程，不把一次持续多天的记录永远挂起。
5. Engine 发送过滤还要求配对生命周期是根节点，并拒绝 links 等元数据；不能只给它设置父节点，否则会被过滤。该约束及 Collector 规则必须按最终关联模型同步调整。
6. 桌面和新版 Engine 都安装全局 subscriber，直接同时初始化会冲突。Engine 当前默认仅允许自己的有限诊断记录，不能直接替换桌面全部本机日志输出。
7. Engine runtime 和 Collector 当前限定 `uc-engine`；增加桌面服务名称需要配置资源与受控允许规则，而不是绕过隐私过滤。
8. 前端“请求返回”和“界面显示结果”是不同时间点。通知触发的再次查询、轮询和界面状态更新都要计入用户动作，且不能把 WebSocket 长连接本身作为整次配对的父操作。

证据：`src/api/daemon/generated-bridge.ts`、`src-tauri/crates/uc-tauri/src/commands/mod.rs`、`crates/uc-webserver/src/api/server.rs`；Engine: `crates/uc-observability-contract/src/diagnostics/mod.rs`、`crates/uc-observability-runtime/src/runtime.rs`、`filter.rs`、`remote_health.rs`、`telemetry.rs`、`tests/observability/collector/collector.yaml`。

## 5. 建议的职责与数据去向

这是后续实施建议，不是已落地设计。

| 数据 | 采集责任 | 开发查看 | 产品运行 |
| --- | --- | --- | --- |
| 产品使用事件 | 保留后台唯一发送入口和既有事件定义 | 本机日志，沿用现有 debug 行为 | PostHog 产品分析 |
| 一次操作的耗时 | 前端持有用户动作；各进程记录实际工作；Engine 记录内部步骤 | 本机 Collector → Jaeger | 开启远程诊断后可发 PostHog Tracing |
| 诊断日志 | 前端日志入口、每个 Rust 进程统一入口 | 本机文件；独立日志查看入口 | PostHog Logs |
| 错误与 panic | 各发生端捕获一次，保留上下文和分类 | 本机记录、隔离的验证环境 | PostHog Error Tracking |
| 回放 | 前端，经明确遮罩与设置控制 | 合成数据验证 | PostHog Session Replay |
| 主动反馈 | 当前反馈表单 | 隔离验证 | PostHog 自定义调查或明确的反馈接收流程 |

Jaeger 不充当完整日志库。未来自部署以标准接收地址为边界，不让业务代码认识具体后端。开发本地采集与产品远程发送分别控制，不能因为开发启用了 Jaeger 就默认把内容发到云端。

service.name 建议表示实际应用服务，而不是为每一层代码建立一个服务。桌面界面与后台可分别识别；Engine 作为后台内的模块，独立测试仍可保留 `uc-engine`。Tauri 记录其实际参与的调用。c/d 用实例信息区分，远程不使用真实业务设备身份。具体名称在统一资源配置时一次确定。

“统一采集”指操作关联、字段规则、采样和进程初始化具有明确负责人；并非用一个 SDK 取代所有用途，也不是要求错误、回放和产品事件全部走同一种协议。

## 6. 必须保留的设置与隐私行为

- `telemetryEnabled` 控制远程诊断，`usageAnalyticsEnabled` 控制产品使用统计，不能因两者都去 PostHog 就合并为一个开关。
- 首次启动、上次关闭、运行中关闭、关闭前已排队、退出时 flush 都需验证；同时检查前端、GUI 进程、daemon。
- 当前产品事件已有独立 analytics 身份；新前端 SDK 不应再自动生成不相关的一套用户，也不能使用真实 DeviceId、空间标识或配对材料连接产品分析。
- 当前 Sentry 的 `sendDefaultPii`、device scope、Redux 状态和通用字符串错误不能机械复制到新实现；按 VISION.md 的允许字段重新审查。
- 剪贴板正文、标题、预览、图片、文件名/路径、搜索内容、配对码、口令和令牌不得进入远程事件、异常、日志或回放。遮挡输入框不足以保护历史列表和图片。
- 反馈正文/邮箱是用户主动提交的数据，需独立于自动采集约束；不要沿用现有反馈邮箱明文 localStorage 持久化而不核对项目默认加密要求。
- PostHog 默认自动点击、页面和其他事件不能直接开启，否则既可能重复现有后台统计，也可能扩大采集范围。

## 7. 替换与删除清单

| 区域 | 替换内容 | 保留内容 |
| --- | --- | --- |
| 前端依赖 | `@sentry/react`、`@sentry/vite-plugin` 及锁文件中的不再需要依赖 | React、Pino、现有业务事件接口 |
| 前端公共入口 | `src/observability/sentry.ts`、`trace.ts`、`breadcrumbs.ts`、`errors.ts`、`src/lib/logger.ts` | 错误分类、脱敏规则、设置语义 |
| 前端调用点 | `main.tsx`、`App.tsx`、`store/index.ts`、两条 IPC 包装路径、反馈表单、元数据和设置绑定 | 窗口启动行为、业务流程、可见错误提示 |
| Rust 组合入口 | `crates/uc-bootstrap/src/observability/tracing.rs` 中 Sentry 初始化和输出、`sentry_gate.rs`、`correlation.rs` 的厂商类型 | 本机日志、过滤、退出保障、共享关联字段 |
| Rust 依赖和测试 | bootstrap 的 `sentry`、Tauri 的 Sentry 测试依赖与专用 mock/断言 | 对用户设置、错误分类、脱敏和真实效果的测试 |
| 构建发布 | `vite.config.ts`、`.github/workflows/build.yml`、`alpha-build.yml` 中 Sentry 上传步骤 | hidden sourcemap、准确版本标识、每个发布产物的符号材料 |
| 配置 | `SENTRY_DSN`、`VITE_SENTRY_DSN`、`SENTRY_AUTH_TOKEN`、`SENTRY_ORG`、`SENTRY_PROJECT`、`VITE_SENTRY_PROJECT` | 现有 `POSTHOG_PROJECT_KEY`；新增上传凭据只能用于构建发布，不能打包进应用 |
| 文档与生成物 | Sentry 专用说明、类型注释、测试说明和生成物 | 与厂商无关的追踪规范、API 契约、隐私规则 |

先完成新路径的验证，再一次切换并删除旧路径；不长期双发。云端旧项目、历史问题、告警规则与凭据不在本次修改范围，停用时间需与新版本覆盖和历史保留安排分开考虑。

## 8. 实施顺序与验收门槛

1. **验证替换差异**：用发布产物验证前端错误、Rust 错误与 panic、macOS/Linux 符号解析；记录 Windows 缺口的明确处置。用真实 Tauri 窗口验证回放遮罩和关闭设置。没有这些证据，不宣称完整替代。
2. **确定统一入口**：对齐 Engine 版本；明确每个进程唯一采集负责人；采用标准追踪传递；保留本机日志与现有 PostHog 产品事件通路，移除全局单一当前操作。
3. **完成真实配对闭环**：前端点击、后台接收、Engine 配对、状态查询、最终界面显示能够关联；验证成功、拒绝、取消、超时、并发、重连和进程重启恢复。
4. **替换剩余 Sentry 功能**：错误、日志、回放、反馈、构建上传与设置绑定逐项通过测试，随后完成切换和删除，不留下长期并行路径。
5. **确认交付**：发布包中不再初始化 Sentry；没有重复事件；两个设置独立生效；远程服务不可达不阻塞主流程；三类桌面窗口及 macOS/Windows/Linux 都有对应证据。

本轮交付是以上盘点和验收清单。尚未执行 SDK 安装、代码迁移、编译发布、真实 PostHog 接收验证、设备 UI 验证或旧云端资源停用。
