# 0031 · 移动端完整可用与生产交付执行计划

- Status: M1 implementation and build validation were recorded. M2 implementation and verification are tracked in the [M2 receipt](0031-mobile-m2-receipt.md). M3 implementation and acceptance are tracked in the [M3 receipt](0031-mobile-m3-receipt.md). M4–M5 remain pending.
- 日期：2026-11-24
- 基线快照：分支 `audit-mobile-engine-gaps`，HEAD `b2e8cd60393b5f02ce486822d6c2e34f3375e2da`
- 前序实现回执：[0031-mobile-engine-integration.md](0031-mobile-engine-integration.md)
- 关联决策：[0031 移动端集成 Engine](../decisions/proposed/0031-mobile-engine-integration.md)、[0011 v1 历史迁移](../decisions/proposed/0011-v1-history-migration.md)、[0014 客户端可观测性](../decisions/proposed/0014-client-observability-otlp.md)、[0018 独立产品构建与发布](../decisions/proposed/0018-independent-product-build-and-release.md)
- 活文档与契约参考：[仓库布局与依赖边界](../architecture/overview.md)、[流水线与本机历史](../architecture/pipeline.md)、[设备身份、信任与配对](../architecture/identity-and-trust.md)、[构建与发布](../architecture/releases.md)、[Engine 公共契约](../contracts/engine-contract.md)、[本机历史契约](../contracts/history-contract.md)、[历史搜索契约](../contracts/history-search-contract.md)、[流水线提交契约](../contracts/pipeline-contract.md)、[设备与后端关联契约](../contracts/device-contract.md)、[目录内容契约](../contracts/directory-contract.md)

---

## 1. 目标、范围与硬约束

### 1.1 目标

基于 [0031 执行回执](0031-mobile-engine-integration.md) 已落地的四层进程内集成骨架（`bindings/mobile` → `app/expo/modules/unic-engine` → `app/expo/src/engine` → Expo 原生 UI），系统性关闭全部 **P0（阻塞性契约与主链路缺陷）**、**P1（日常完整可用与平台体验缺口）** 与 **P2（包体/性能、去壳、真机验收、CI 与商店发布）** 缺口，使 iOS 与 Android 移动端达到可长期日常使用并具备生产发布条件的标准。

### 1.2 硬约束（不可突破）

1. **单向依赖边界**：根模块 `engine/`、`internal/`、`plugin/` 不得依赖 `bindings/` 或 `app/expo/`；`bindings/` 维持独立嵌套 Go module，仓库根目录不保留常驻 `go.work`。
2. **契约先行，不静默破坏公开契约**：所有修改严格遵守 [Engine 公共契约](../contracts/engine-contract.md) 与历史/设备/流水线契约；若排查中发现需变更公开契约或 wire 协议，必须先停下提交复现证据与提案，待确认后再实施。
3. **安全与隐私底线**：设备地址、配对票据、v1 旧口令均为秘密，严禁写入日志、事件流、错误文案或持久化存储；通过配对码或推荐发现的设备永远需人工核对指纹批准。
4. **平台合规边界**：严禁在 iOS App Extension 内启动第二个 Engine 实例；严禁将无障碍服务、ADB/Shizuku 或悬浮窗保活作为 Engine 契约前提；所有剪贴板与后台限制须通过 `UpdateHostCapabilities` 如实上报。
5. **诚实的分层验收**：TypeScript 校验、`expo prebuild` 与 `npm run verify` 仅证明前端打包边界，绝不替代原生编译、模拟器运行、真机跨端同步或商店签名验收；未执行项须明确标记为 `NOT RUN`。

### 1.3 本计划不做（明确排除项）

- 鸿蒙（HarmonyOS NEXT）原生桥接与分发。
- 移动端外部独立进程 Go 插件（`pluginproc`）与桌面端在线插件市场。
- 额外同步后端（如 0029 SyncClipboard）、外部渠道（0009）与受控 Lua 插件 UI（0004）默认不作为移动端 v2.0 首发阻塞项，按需在 M5 之后单独立项。
- 修改设备间 wire 协议或新增远端应用 ACK、断点续传。

---

## 2. 基线审计与 Gap 映射矩阵

以下 Gap 均经基线代码逐行核实，并按优先级分配至对应里程碑：

| 级别 | 编号 | 问题 / 缺口摘要 | 代码定位 | 目标阶段 |
|---|---|---|---|---|
| **P0** | G01 | `applyHistoryToClipboard` 未先建立 `ObserveClipboard(baseline)` 即传固定 `expectedEpoch`，命中 `engine/history.go` `CodeConflict`；且未释放 `ticket.lease_id`，异常时卡死 `e.ticket` | `app/expo/modules/unic-engine/ios/UnicEngineModule.swift`、`UnicEngineModule.kt`、`engine/history.go`、`engine/clipboard.go` | M1.1 |
| **P0** | G02 | TS ↔ Go DTO 多处错配：`setBindingSync` 传 `sync_enabled` 而 Go 读 `enabled`；`getPairingCode` 未判 `exists`；`pairCandidate` 缺 `request_id` 且误按 JSON 解析裸 `opID`；`EngineStateSnapshot.settings` / `exportHistoryImagePreview` / `countHistorySearches` 类型错位 | `bindings/mobile/engine.go`、`app/expo/src/engine/{nativeModule,store,mapper,types}.ts` | M1.2 |
| **P0** | G03 | 无自动系统剪贴板监听，且原生模块未暴露 `ListReceived` / `DismissReceived` / `BeginApply`，Home 接收入口为 `0` / `showUnavailable`；Android 剪贴板仅支持纯文本且无 `lastAppliedSequence` 去重 | `UnicEngineModule.swift`、`UnicEngineModule.kt`、`app/expo/src/screens/useHomeController.tsx` | M1.3 |
| **P0** | G04 | 详情页与选词页共用 512 B 列表预览导致长文本截断；前端 `matchesHistoryFilter` 二次过滤误杀 512 B 之后的 Engine 搜索命中，日期/来源筛选未下推 Engine；历史写死 `limit: 100` 且 `loadMoreItems` 为空函数 | `app/expo/src/engine/store.ts`、`app/expo/src/hooks/useHistoryItemText.ts`、`app/expo/src/screens/useHomeController.tsx` | M1.4 |
| **P0** | G05 | `openSendTo(_item)` 丢弃参数导致无法转发已有历史条目；`bindings/mobile` 未绑定 `Engine.RetainHistoryForSend` | `bindings/mobile/engine.go`、`app/expo/src/screens/useHomeController.tsx`、`app/expo/src/shell/DemoSendSheet.tsx` | M1.4 |
| **P1** | G06 | 设置页仍修改内存 `useShellStore`（未调用 `updateSettings`），Engine 核心配置（设备名、同步策略、大小限额、缓存配额、自动清理、Rendezvous）无 UI，多个子设置页仍是 `showUnavailable` | `app/expo/src/shell/settings.ts`、`app/expo/src/engine/store.ts` | M2.1 |
| **P1** | G07 | 详情页与快捷动作多处 `showUnavailable`（打开链接、保存图片、导出文件、系统分享、拍照、相册选图、选词复制）；文件条目缺失真实文件名（统一显示 `"File"`）；目录未绑定 `ListHistoryFolder` | `app/expo/src/screens/useHomeController.tsx`、`app/expo/src/hooks/useWordPicker.ts`、`app/expo/src/engine/mapper.ts`、`bindings/mobile/engine.go` | M2.2 |
| **P1** | G08 | 任意 `onEngineEvent` 触发全量 `refreshAll()`；`caches/previews/*.img` 磁盘缓存与 JS 预览 `Map` 无淘汰清理 | `app/expo/src/engine/store.ts`、`UnicEngineModule.swift`、`UnicEngineModule.kt` | M2.3 |
| **P1** | G09 | 无二维码展示与扫码配对；`joinByCode`/`joinByTicket` 仅轮询 3 秒；`renameDevice`/`ignoreCandidate`/`togglePin`/自定义标签/本机指纹无 UI；新增页面大量硬编码简体中文 | `app/expo/src/screens/DevicesScreen.tsx`、`app/expo/src/shell/DemoSendSheet.tsx`、`app/expo/src/screens/setup/MigrationSetupScreen.{ios,android}.tsx` | M3.1 / M3.4 |
| **P1** | G10 | Android 迁移缺少 Keystore + `BiometricPrompt` 凭据读取（`NoStoredSecretProvider` 直接报错）；Android SAF `tryResolveTreeUriToLocalPath` 在 Android 11+ Scoped Storage 与非 primary 卷不可靠；`beginLaterMigration` 重复注册进度监听 | `UnicEngineModule.kt`、`app/expo/src/engine/store.ts` | M3.2 |
| **P1** | G11 | `HostCapabilities` 写死 `available` 未随前后台/焦点动态上报；缺少 Android `ACTION_SEND`、iOS Share Extension（App Group 暂存）及 Android 可选前台同步服务 | `UnicEngineModule.swift`、`UnicEngineModule.kt`、`app/expo/app.json` | M1.1 / M3.3 |
| **P2** | G12 | 原生产物体积偏大（XCFramework 299 MB、Android Debug APK 309 MB），未启用 Release 裁剪与符号分离，缺真机冷启与大文件 RSS 压测基线 | `bindings/scripts/build-mobile.sh`、`app/expo/modules/unic-engine/android/build.gradle` | M4.1 |
| **P2** | G13 | 未执行真机双端跨网/DERP、切网重连、挂起恢复、真实 Keychain/BiometricPrompt 与迁移五终态交互验收 | [0031 执行回执](0031-mobile-engine-integration.md) | M4.2 |
| **P2** | G14 | `app/expo/app.json` 仍为隔离壳配置（`app.uniclipboard.shell` / `0.0.0`）；未接入 0014 诊断可观测性；CI 仅跑前端 bundle 校验，无 Go/gomobile/原生/Maestro 流水线与 0018 商店签名发布 | `app/expo/app.json`、`.github/workflows/check-expo.yml`、`release/products.toml` | M5 |

---

## 3. 分阶段实施计划（M1 → M5）

### M1 · P0 契约修复与核心闭环（最高优先级）

目标：修复全部阻塞缺陷，使 iOS/Android 在模拟器与真机上跑通「自动/手动捕获 → 加密历史分页/全文/搜索 → 双向同步 → 接收与复制应用 → 原条目转发」完整闭环。

#### M1.1 剪贴板基线、防回环与动态能力上报（覆盖 G01、G11）

- [x] **建立合规基线初始化**：
  - 在引擎启动后首次获焦/进入前台（以及每次从后台回到前台）时，由原生宿主同步系统剪贴板状态：
    - 若当前处于受限状态（后台、Android 10+ 无焦点、或 iOS 策略禁止静默读粘贴板），立即调用 `UpdateHostCapabilities` 如实上报 `observe: "unavailable"` 或 `"permission_required"`，不静默触发 iOS 粘贴授权弹窗。
    - 仅在允许观察剪贴板时记录并上报 `ObserveClipboard(mode="baseline")`；当尚未建立基线（`e.obs.epoch == ""`）执行手动 `BeginApplyHistory` 时，严格按 [Engine 公共契约](../contracts/engine-contract.md) 对齐 `expected_epoch = ""` 或先刷新基线，避免误触发 `CodeConflict`。
- [x] **修复 `applyHistoryToClipboard` 资源与异常安全**：
  - Swift 与 Kotlin 在获取 `ticket` 后，必须使用 `defer` / `try-finally` **无条件调用 `eng.releaseContent(leaseID)`**。
  - 若读取内容或写入系统剪贴板失败/抛异常，必须在退出前调用 `eng.reportApplyResult({ token, outcome: "failed", error_code: ... })`，确保 `e.ticket` 绝不卡死。
  - 写入剪贴板前获取并核对最新 `sequence`，写入成功后读回新 `sequence` 并上报 `outcome: "applied"`；若发生 `CodeConflict`（写入瞬间用户外部复制了新内容），原生层明确返回冲突或重试一次基线同步后提示，不静默吞错。
- [x] **补齐 Android 图片剪贴板读写与防重**：
  - Android 写入图片历史时，通过应用私有 `FileProvider` 导出带正确 MIME/扩展名的临时图片 URI 并写入 `ClipData.newUri(...)`，同时在 `AndroidManifest` / Expo Config Plugin 中声明 `FileProvider`。
  - Android 读取剪贴板时支持 `image/*` MIME 的 `ClipData` URI（流式导入 `ImportFileSource` 或 `ImportBytes`），并记录 `lastAppliedSequence` / 内容指纹防止自写回环。

#### M1.2 TS ↔ Go 绑定层契约统一（覆盖 G02）

- [x] 修正 [nativeModule.ts](../../app/expo/src/engine/nativeModule.ts) 与 [store.ts](../../app/expo/src/engine/store.ts)：
  - `setBindingSync`：请求 JSON 字段统一为 `enabled`（与 `bindings/mobile/engine.go` 一致）。
  - `getPairingCode`：显式定义 `{ exists: boolean; code: string; expires_at_ms: number }`，当 `!res.exists` 时向 store 写入 `null`。
  - `pairCandidate`：补传必填 `request_id`（幂等 UUID），改用 `callString` 接收 `OperationID`，并接入操作状态跟踪。
  - `EngineStateSnapshot` 与 `updateSettings`：精确映射 Go 的 `SettingsState { revision: string; value: EngineSettings }`（包含 `history`、`file_cache`、`limits`、`sync`、`backend_policies`、`device_name`、`rendezvous_url`）以及 `received`、`active_apply`。
  - `exportHistoryImagePreview`：统一解析 `part.mime` / `part.name` 与 `uri`。
  - `countHistorySearches`：将返回的 `counts: string[]` 显式转为 `number[]`。
- [x] 移除 `beginLaterMigration` 中冗余的 `addMigrationProgressListener`（统一由 `startMigration` 管理订阅生命周期）。
- [x] 在 `bindings/mobile/mobile_test.go` 与 `app/expo` 增加契约回归测试（覆盖 `setBindingSync(true/false)`、`getPairingCode(empty/active)`、`pairCandidate`、无基线 `BeginApplyHistory` 与租约释放检查）。

#### M1.3 自动剪贴板捕获与远端接收队列闭环（覆盖 G03）

- [x] **自动捕获（受设置开关与平台合规策略控制）**：
  - Android：在 `UnicEngineModule.kt` 注册 `ClipboardManager.OnPrimaryClipChangedListener`，并在 Activity 获焦时（`onWindowFocusChanged` / `OnActivityEntersForeground`）在启用 `sync.automatic` 且能力为 `available` 时读取变化并调用 `ObserveClipboard(mode="change")`。
  - iOS：在 `UnicEngineModule.swift` 监听 `UIPasteboard.changedNotification`；结合 iOS 16+ 粘贴权限提示限制，提供「回前台检测变化（`hasStrings`/`hasImages` 轻探针 + 按用户设置或一键确认读取）」机制，避免无意义弹窗。
- [x] **远端接收队列（Received）与应用闭环**：
  - Swift 与 Kotlin 原生模块暴露 `listReceived()`、`dismissReceived(receiptID)`、`applyReceivedToClipboard(receiptID)`（内部走 `BeginApply` → 写剪贴板 → `ReportApplyResult` + `ReleaseContent`）。
  - 订阅 `EventReceived`：若用户开启「自动应用远端内容」且当前前台可写剪贴板，自动触发 `applyReceivedToClipboard`；否则更新 `received` 列表计数，打通 Home 顶栏接收徽章（`receiveEntryCount`）与接收列表弹层（支持一键应用或忽略）。

#### M1.4 历史全文按需加载、搜索下推、分页与原条目转发（覆盖 G04、G05）

- [x] **按需全文加载与租约管理**：
  - 列表页保持轻量预览（512 B 文本预览 / 缩略图），不把大正文塞进列表状态。
  - 原生模块与 `bindings/mobile` 暴露按需全文读取接口（`readHistoryFullText(historyID, maxBytes)` 或 `retainHistoryContent` + `readChunk` + `releaseContent`），改造 `useHistoryItemText` 在详情页与分词选词页挂载时按需加载完整正文，退出时清理内存引用。
- [x] **修复搜索与筛选口径**：
  - `live` 模式下移除 `useHomeController` 对 Engine `SearchHistory` 结果的客户端关键词二次过滤，完全信任 Engine 返回的 `hits` 与高亮预览。
  - 将 `selectedDateFilter`（转本地自然日 `created_from_ms` / `created_before_ms`）和 `selectedSourceFilter`（转 `sources: [{ kind: "local" }]` 或远端设备）下推给 `SearchHistory` / `ListHistory`。
- [x] **真实游标分页**：
  - `useEngineStore` 维护 `nextCursor`、`hasMore`、`loadingMore`；列表触底时调用 `loadMoreItems()` 按 `next_cursor` 追加历史与搜索结果，并处理 `history_revision` 变更后的游标失效重置。
- [x] **原条目转发（Resend / SendTo）**：
  - `bindings/mobile/engine.go` 新增 `RetainHistoryForSend(historyID string)` 与 `ResendHistoryItem(token, historyID, deviceIdsJSON)`（内部 `RetainHistoryForSend` → `CommitContent` → `defer ReleaseContent`）。
  - Swift / Kotlin / TS 暴露 `resendHistoryItem(historyID, deviceIds)`；`useHomeController.openSendTo(item)` 将选中条目传入发送面板，支持对任意历史文本/图片/文件定向发送给一台或多台已信任设备，同时保留新建文本/文件发送入口。

**M1 退出门控**：
- [x] `cd bindings && go test -race -count=1 ./...` 全部通过（含新增的防回环/租约/DTO/转发/全文/分页测试）与根模块边界测试通过。
- [x] `./bindings/scripts/build-mobile.sh`、`cd app/expo && npm run verify`、iOS Simulator `xcodebuild` 与 Android Debug APK 编译全部通过。
- [ ] iOS 模拟器与 Android 模拟器交互实测与跨桌面端实测（`NOT RUN` — 环境未安装 Maestro，尚未执行交互与双端实机/模拟器联调）：冷启后不点「读取系统剪贴板」直接复制历史成功；连续复制 20 次无租约泄漏错误；设备同步暂停与恢复双向生效；>10 KB 文本在详情页全文可见且后段关键词可搜出；>120 条历史可正常下拉翻页；与本机桌面端完成文本、图片、文件双向互传及已有条目定向转发。

---

### M2 · P1 真实设置、详情动作、媒体文件与性能治理

Implementation, process acceptance, simulator acceptance and iPhone results are recorded in the [M2 receipt](0031-mobile-m2-receipt.md). iPhone document import and Android physical-device acceptance remain pending.

目标：彻底移除 live 模式下的壳占位与 `showUnavailable`，完善媒体/文件处理与缓存生命周期，使日常核心操作无断层。

#### M2.1 真实设置体系落地（覆盖 G06）

- [x] 在 `useEngineStore` 实现 `updateSettings(mutator)`（基于当前 `snapshot.settings.revision` 提交 `UpdateSettings`，遇 `conflict` 自动刷新快照提示或重试）。
- [x] 重构 `app/expo/src/shell/settings.ts`（拆分生产设置模块 `src/screens/settings/`，仅在 `mode === "unavailable"` 时回落 shell 提示）：
  - **同步设置**：本机设备名（可编辑）、全局同步开关（`sync.enabled`）、自动同步（`sync.automatic`）、恢复历史时同步（`sync.on_restore`）、文件同步（`sync.files`）、设备推荐（`sync.introductions`）、本地自动应用/自动捕获开关（持久化至本机配置）。
  - **历史与清理**：自动清理开关（`history.auto_cleanup`）、保留天数（`retention_days`）、最大条数（`max_items`）、排除标签（`excluded_tags`，默认保留 `starred`/`pinned` 选项）。
  - **存储与传输限额**：单文件上限（`limits.max_file_bytes`）、总内容上限（`limits.max_total_bytes`）、文件缓存配额（`file_cache.max_bytes`）、实时展示 `GetFileCacheState` 占用与「清理本地预览缓存」动作、保留「从 v1 迁移」入口。
  - **高级网络**：Rendezvous URL 配置（含重置默认）与当前 Tailcat 连接状态概览。
  - **能力与状态页**：剪贴板访问状态页如实展示 `capabilities.effective` 与 `restrictions`（引导前往系统设置，不伪造能力）。

#### M2.2 详情页动作、媒体/文件导出与目录只读浏览（覆盖 G07）

- [x] **真实文件名与元信息**：
  - 在 `bindings/mobile/engine.go` 扩展历史摘要辅助（或在 `HistoryItem` 列表水合时通过轻量接口返回文件 part 的真实 `name`、`mime`、`size`、`count`），使单文件显示真实文件名与扩展名徽章，多文件显示首文件名 + 总数。
- [x] **关闭全部 `showUnavailable` 详情/卡片/加号菜单动作**：
  - **打开链接**：校验 `http://`/`https://` 后调用 `Linking.openURL`。
  - **保存图片到相册**：导出原图临时文件（保留真实扩展名）→ 写入系统相册（iOS `PHPhotoLibrary` / Android `MediaStore.Images`，妥善处理权限拒绝提示）。
  - **导出/保存文件**：iOS 调 `UIDocumentPickerViewController(forExporting:)`，Android 调 `ACTION_CREATE_DOCUMENT` 流式写入用户选定位置，导出后清理临时文件。
  - **系统原生分享**：将「发送到（UniClipboard 设备）」与「系统分享（OS Share Sheet）」解耦；系统分享导出临时文件/文本并拉起 `UIActivityViewController` / Android `Intent.ACTION_SEND`（经 `FileProvider`）。
  - **拍照与相册选图**：`handleTakePhoto` 接入原生相机捕获，`handleUploadImage` 接入原生相册选择器（iOS `PHPickerViewController` / Android Photo Picker），保留 `handleUploadFile` 走文档选择器。
  - **分词选词复制**：修复 `useWordPicker.ts` 的 `copySelected`，走防回环纯文本写入系统剪贴板并弹出已复制反馈。
- [x] **目录只读浏览**：
  - `bindings/mobile/engine.go` 绑定 `ListHistoryFolder(historyID, limit)`；移动端详情页对 `directory` / `directory_reference` 条目展示文件树摘要（路径、大小、是否截断）。

#### M2.3 事件增量刷新与预览缓存治理（覆盖 G08）

- [x] 改造 `useEngineStore` 事件分发：解析 `Event.Kind` 做分类节流刷新：
  - `history` → 仅刷新历史与标签；
  - `devices` / `pairing_code` → 仅刷新设备、候选与配对码；
  - `received` / `apply` → 仅刷新接收列表与状态快照；
  - `settings` / `capabilities` / `runtime` / `lifecycle` → 刷新快照与能力；
  - 仅在 `onEngineResync` 或回到前台对账时执行全量 `refreshAll()`。
- [x] **缓存上限与清理**：
  - `textPreviewCache` 与 `imagePreviewCache` 改为有界 LRU（例如最多 200 条文本、60 张图）。
  - 磁盘 `caches/previews/` 使用真实扩展名并设置容量/数量上限（例如 100 MB），启动与删除历史条目时同步清理孤立预览文件。

**M2 退出门控**：
- live 模式下主界面、详情页、长按菜单、选词页、设置页实现**零 `showUnavailable`**。
- 修改全部 Engine 设置项并强杀重启 App，验证设置持久化与生效。
- iOS/Android 模拟器与真机验证：链接跳转、图片存相册、文件导出、系统分享、相册选图、拍照上传、选词复制、目录清单展示全部通过。

---

### M3 · P1 配对体验、v1 迁移闭环、系统入口与四语收口

The [M3 receipt](0031-mobile-m3-receipt.md) records implementation, contract changes and remaining acceptance.

目标：补齐移动端高频交互闭环（扫码配对、系统分享、Android 真实 Keystore/SAF 迁移、多语言一致性）。

#### M3.1 配对与设备管理体验升级（覆盖 G09）

- [x] **二维码配对与扫码**：
  - 导出配对票据弹层支持渲染二维码 + 一键复制（关闭弹层或超时后立即从内存与屏幕清除敏感票据）。
  - 加入设备页新增扫码入口（申请相机权限，识别票据或 6 位配对码后自动提交）。
- [x] **长时配对操作状态追踪**：
  - 取代固定 3 秒轮询：通过 `EventOperations` + `GetOperation` 实时展示 `resolving` → `connecting` → `handshaking` → `waiting_approval` / `succeeded` / `failed`，支持用户主动取消等待并展示明确错误原因。
- [ ] **设备、候选与标签 UI 补齐**：
  - 设备详情底部表单补齐「重命名设备」（`renameDevice`）；候选设备列表补齐「忽略」（`ignoreCandidate`）。
  - 设备页顶部展示本机名称与安全指纹（方便与对端人工核对）。
  - 历史条目菜单/详情页补齐「置顶」（`togglePin`）与自定义标签编辑/按标签筛选。

#### M3.2 v1 迁移双端真机闭环（覆盖 G10）

- [ ] **Android Keystore + `BiometricPrompt` 凭据提供器**：
  - 调研并核对 v1 Android 实际 Keystore alias / 加密封装格式；在 `UnicEngineModule.kt` 用真实 `BiometricSecretProvider` 替换 `NoStoredSecretProvider`（支持生物识别与设备锁屏凭据回落，拒绝或未找到密钥时返回 `ErrUnlock` 并在当前流程内切至旧口令输入）。
- [x] **Android SAF 目录兼容（Android 11+ Scoped Storage）**：
  - 改造 `selectSetupCustomSource`：不再依赖 `Environment.getExternalStorageDirectory()` 强转路径；使用 `DocumentFile.fromTreeUri` 校验 `vault/keyslot.json` 存在性后，通过 `ContentResolver` 将只读备份目录镜像到应用私有临时暂存目录（校验总大小与路径穿越边界，跳过符号链接），将私有暂存目录交由 `MigrationSession` 读取，迁移完成/取消/切换源时彻底删除暂存目录。
- [ ] **iOS Keychain / 覆盖升级核验**：
  - 核对 v1 iOS 实际 Keychain `kSecAttrService` / `kSecAttrAccount` / Access Group 与沙盒目录，确保覆盖升级场景下自动检测与 Face ID / Touch ID 解锁可用。

#### M3.3 移动端系统入口与后台策略（覆盖 G11）

- [x] **Android 分享接收（`ACTION_SEND` / `ACTION_SEND_MULTIPLE`）**：
  - 通过 Expo Config Plugin 注册 `IntentFilter`（`text/plain`、`image/*`、`*/*`），收到外部内容后唤起主 App，展示「直接保存到历史 / 发送给指定设备」快捷确认并经 `ImportText` / `ImportFileSource` 提交。
- [x] **iOS Share Extension（App Group 暂存投递）**：
  - 规划独立 Share Extension Target（Expo Config Plugin 注入），仅将共享的文本/图片/文件写入 App Group 共享暂存队列并唤起主 App（或等待主 App 消费），主 App 启动/回前台时由唯一 Engine 消费入库并清理暂存。**严禁在扩展内链接或启动 Engine。**
- [x] **Android 可选前台同步服务（Foreground Service）**：
  - 在「设置 › 后台运行」提供明确开关（默认关闭）：开启时注册带低干扰常驻通知的 Foreground Service（Android 13+ 请求 `POST_NOTIFICATIONS`、Android 14+ 声明合规 `foregroundServiceType`），退后台保持 `RuntimeActive` 以持续接收远端内容；关闭时保持退后台 `suspended` 省电策略。

#### M3.4 国际化与生产代码清理（覆盖 G09）

- [x] 将 `DevicesScreen`、`SendSheet`（重命名替代 `DemoSendSheet`）、`MigrationSetupScreen.{ios,android}` 及 `EngineError` 错误码提示全部迁入 `app/expo/src/i18n/locales/{zh,en,ru,pt-BR}/`，遵循 Apple / Material 各语言区标准术语，消除硬编码中文。
- [x] 将 `useHomeController` / `ThemeContext` / `useHistoryDisplaySettings` 中的 `@uniclipboard-shell:*` 本地存储键统一为生产命名空间。

**M3 退出门控**：
- 二维码配对、6 位码配对与慢网 DERP 握手状态展示通过；设备改名/忽略候选/置顶/标签全通。
- Android 11+ SAF 外置目录与 `BiometricPrompt` 迁移、iOS Keychain 迁移在真机通过。
- Android/iOS 系统分享接收文本/图片/文件通过；四语界面切换走查无硬编码文案。

---

### M4 · P2 包体瘦身、内存压测与真机跨端验收

The [M04 simulator receipt](0031-mobile-m4-simulator-receipt.md) records the
binding size comparison and simulator preflight. The physical-device gate
remains open.

目标：将移动产物优化到可接受体积与内存水位，并产出完整的双端真机验收回执。

#### M4.1 产物瘦身与性能基线（覆盖 G12）

- [ ] **构建瘦身**：
  - 更新 [bindings/scripts/build-mobile.sh](../../bindings/scripts/build-mobile.sh)：为 `gomobile bind` 增加 `-ldflags="-s -w"` 与 `-trimpath`；分析 `UnicCore.xcframework` 与 `libgojni.so` 符号占比（评估是否可按 build tag 裁剪移动端不需要的包或调试信息）。
  - Android Release 构建采用 AAB（App Bundle）与按 ABI 分包（`arm64-v8a` 主包），分离 native debug symbols；iOS Release 开启 Bitcode-off + 符号 Strip 并归档 dSYM。
- [ ] **预设性能与资源门槛（在真机实测前冻结目标）**：
  - 单架构安装下载体积（App Store 预估下载 / Play AAB 交付体积）：记录基线并制定瘦身目标（目标压缩至 ≤ 45 MB/ABI，若 `tailscale.com` 依赖占比超预期需书面记录原因与取舍）。
  - 冷启动到历史首屏可交互（已有 500 条历史）：≤ 1.5 s（中端真机）。
  - 空载常驻内存（RSS）：≤ 150 MB；100 MB 单文件流式收发峰值 RSS 增量：≤ 60 MB（验证无全量内存拷贝）。
  - SQLite WAL 强杀恢复：写入中强杀 App 重启，无数据库损坏、无锁残留。

#### M4.2 真机端到端验收矩阵（覆盖 G13）

准备至少一台 iOS 真机、一台 Android 真机（Android 11+）及一台桌面端（macOS/Linux/Windows），在真实网络下逐项执行并留痕至新回执 `docs/exec/0031-mobile-real-device-receipt.md`：

| 编号 | 场景 | 必须验证的断言 |
|---|---|---|
| R01 | 全新安装与跳过 Setup | 首次启动进入 Setup（`w-clean`），跳过后写入 `complete:true` 并进入历史；强杀重启直接进历史不再弹 Setup |
| R02 | 覆盖升级 / 自定义目录 v1 迁移（五终态） | 分别验证 `done`、`partial`、`cancelled`（保留已导入记录 + 幂等续跑）、`conflict`（目标忙碌）、`failed`（错口令重试 + 生物识别回落）；迁移后历史文本/图片/文件/标签完整可读 |
| R03 | 跨网与局域网配对（iOS ↔ Android ↔ Desktop） | 6 位配对码 + 二维码票据双路径；强校验必须核对指纹批准后才连接；未批准前无内容同步 |
| R04 | 文本 / HTML / 图片 / 100 MB 文件双向同步 | 局域网直连与蜂窝/跨网 DERP 中继均通过；接收后可正常预览、复制、存相册/导出、原条目转发 |
| R05 | 防回环与动态能力 | 复制历史写入系统剪贴板后不产生回环重复条目；退后台后自动 `suspended` 且能力上报准确；回前台自动重连并同步 |
| R06 | 切网与弱网恢复 | Wi-Fi ↔ 4G/5G 切换触发 `NotifyNetworkChanged`，连接自动恢复且无需重启 App |
| R07 | 设置、清理与撤销 | 修改设备名/同步开关/清理策略重启保留；撤销设备（`UnpairDevice`）后连接立即断开且对方无法再发内容 |
| R08 | 系统入口与多语言 | 系统分享文本/图片/文件入库；中/英/俄/葡四语 + 深浅色模式 + iPad 分栏布局走查无严重错位 |

**M4 退出门控**：R01–R08 在真机全部通过并形成含构建 SHA、设备信息、包体/内存实测表与截图/日志证据的验收回执。

---

### M5 · P2 去壳、可观测性、CI 与商店发布（衔接 0018）

The [M05 preparation receipt](0031-mobile-m5-receipt.md) records production
configuration, diagnostics, candidate workflows, and remaining activation checks.

目标：完成产品正式化配置、自动化构建验收与商店分发闭环，并将 0031 落地归档。

#### M5.1 正式产品标识与权限配置（覆盖 G14）

- [ ] 更新 [app/expo/app.json](../../app/expo/app.json)（并移除 `UI Shell` 过渡命名）：
  - 正式应用名称（`UniClipboard`）、生产 iOS `bundleIdentifier` 与 Android `package`（核对 v1 覆盖升级策略，保持与已有商店应用一致或按 0018/0025 明确定义替换策略）。
  - 配置生产 App Group、Keychain Access Group、URL Scheme / Associated Domains。
  - 补全隐私权限声明：iOS `NSLocalNetworkUsageDescription`、`NSPhotoLibraryAddUsageDescription`、`NSPhotoLibraryUsageDescription`、`NSCameraUsageDescription`、`NSFaceIDUsageDescription`；Android 相机/通知/前台服务/网络权限。
  - 版本号与 build number 与 [release/products.toml](../../release/products.toml) 联动（通过构建脚本或 Expo `app.config.ts` 单源读取）。

#### M5.2 客户端可观测性与诊断（衔接 0014）

- [ ] 在 `bindings/mobile/engine.go` 接入 `engine.Options` 可观测性配置（遵循 [可观测性契约](../contracts/observability-contract.md)：默认启用、可被设置开关关闭、仅含 ingestion-only token、使用独立随机 profile ID、严禁泄露剪贴板/地址/票据/路径）。
- [ ] 在「设置 › 诊断」页提供本机脱敏诊断日志导出与遥测开关。

#### M5.3 CI 自动化与商店发布流水线（衔接 0018）

- [ ] 扩展 `.github/workflows/`：
  - 新增 `bindings/mobile` Go 测试作业（`cd bindings && go test -race ./...`）及根模块边界测试联动。
  - 新增移动端原生构建校验工作流（`gomobile bind` + iOS Simulator `xcodebuild` + Android `assembleRelease`），替换仅跑前端 bundle 的单一检查。
  - 升级 `app/expo/e2e/flows/` Maestro 用例覆盖真实 live Engine 路径（Setup、历史读写、搜索、配对页、设置），并在 macOS runner 上定期/按需执行产出 `receipt.json`。
- [ ] 补齐独立移动发布工作流（写入 `release/products.toml` 的 `android`/`ios` targets）：
  - iOS：代码签名（Provisioning Profile / App Store Connect API Key）→ IPA 构建 → TestFlight / App Store 上传。
  - Android：发布 Keystore 签名 → AAB 构建 → Google Play 内部测试轨道 / 正式轨道上传 + GitHub Release 校验和清单。

#### M5.4 活文档落地与决策归档

- [ ] 将最终落地的移动端架构、能力边界、构建/发布路径与验收结果合并进：
  - [docs/architecture/overview.md](../architecture/overview.md)
  - [docs/architecture/releases.md](../architecture/releases.md)
  - [docs/contracts/engine-contract.md](../contracts/engine-contract.md) / [docs/contracts/history-contract.md](../contracts/history-contract.md)（如有补充约束说明）
  - [app/expo/README.md](../../app/expo/README.md)、[app/ios/README.md](../../app/ios/README.md)、[app/android/README.md](../../app/android/README.md)、[AGENTS.md](../../AGENTS.md)
- [ ] 运行 `make docs-check` 零错误后，按 [文档生命周期](../decisions/README.md) 流程将 0031 归档。

---

## 4. 主要技术风险与应对预案

| 风险 | 影响 | 预案 / 缓解措施 |
|---|---|---|
| **iOS 16+ 剪贴板粘贴弹窗** | 若回前台自动读 `UIPasteboard.general.string`，每次切回 App 都弹系统「允许粘贴」横幅，体验极差 | 仅在用户点击 `UIPasteControl` / 显式触发「读取系统剪贴板」或设置允许时读取；用 `changeCount` / `hasStrings` / `hasImages` 做无感探测并提示操作，不静默强读 |
| **Android 10+ 后台剪贴板与后台断连** | 后台不可读剪贴板，且系统可能冻结后台网络 | 后台严格通过 `UpdateHostCapabilities` 上报 `unavailable`；提供可选 Foreground Service 满足需要后台收消息的用户，默认省电 `suspended` |
| **v1 生产包名 / Keychain / Keystore 历史兼容** | 若 v2 签名证书、Team ID、Keychain Group 或 Android Keystore alias 与 v1 不一致，覆盖升级无法自动无感解锁旧库 | 在 M3.2 启动前核实 v1 实际已发布的 iOS/Android 签名与凭据元数据；无论凭据是否可读，旧口令回落与手动选择导出目录路径必须 100% 可用 |
| **Go + Tailscale 移动端包体与内存** | `tailscale.com` 与 `modernc.org/sqlite` 导致 `.so`/XCFramework 体积偏大 | M4.1 先落 `-s -w` + `-trimpath` + AAB 分包；若单 ABI 下载体积或 RSS 仍超阈值，再评估按 build tag 裁剪无用子包或切移动端 CGO 系统 `libsqlite3` |
| **`gomobile` 与 Go 版本耦合** | Go 工具链升级可能导致 `gomobile` 失效 | 保持 `bindings/go.mod` 与 `build-mobile.sh` 锁定工具链版本，并在 CI 中实跑 `gomobile bind` 及时暴露回归 |

---

## 5. 标准验证命令清单

每完成一个里程碑，须按分层执行对应验证并记录到回执（不跳级替代）：

```sh
# 1. 根模块边界与核心检查
go test -race -count=1 ./engine -run TestPublicEngineHasNoDesktopDependencies
go test -race -count=1 ./internal/v1migration/...

# 2. Go 移动端绑定层竞态测试
(cd bindings && go test -race -count=1 ./...)

# 3. 构建 iOS XCFramework 与 Android AAR
./bindings/scripts/build-mobile.sh

# 4. Expo 前端边界、TypeScript、prebuild 与双端 Bundle 校验
(cd app/expo && npm run verify)

# 5. 原生编译校验（iOS Simulator Release / Android Release）
(cd app/expo && npm run prebuild && npx pod-install && \
  xcodebuild -workspace ios/UniClipboard.xcworkspace -scheme UniClipboard \
  -configuration Release -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath /tmp/unic-ios-build CODE_SIGNING_ALLOWED=NO)
(cd app/expo/android && ./gradlew assembleRelease -PreactNativeArchitectures=arm64-v8a)

# 6. Maestro 原生 UI E2E（需指定真实模拟器/真机设备 ID 与产物路径）
(cd app/expo && node e2e/run.mjs --platform ios --device <sim-udid> --app <path-to-app>)
(cd app/expo && node e2e/run.mjs --platform android --device <adb-serial> --app <path-to-apk>)

# 7. 文档与链接检查
make docs-check
```
