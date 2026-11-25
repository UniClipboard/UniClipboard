# 0032 · 异步历史富化能力（Enricher）与链接预览首个闭环

- Status: accepted. Engine, RPC, local service, Desktop link preview UI, and built-in Smart Preview (`builtin.preview`, enabled by default and user-disableable) are implemented. External `enricher` plugin extensibility is verified via `plugins/examples/linkpreview/`. Future built-in preview providers (e.g. verification code preview) and the OCR follow-up remain open.
- 日期：2026-11-25
- 关联：[0004 剪贴板流水线与插件](0004-clipboard-pipeline-lua.md)、[0010 插件设置 Schema](0010-plugin-settings-schema.md)、[0027 文叔叔临时分享 Action](0027-wenshushu-share-action.md)、[0028 桌面端在线插件市场](0028-desktop-plugin-marketplace.md)
- 落地目标：[插件契约](../../contracts/plugin-contract.md)、[插件 RPC 核心协议](../../contracts/plugin-rpc-contract.md)、[本机历史契约](../../contracts/history-contract.md)、[历史搜索契约](../../contracts/history-search-contract.md)、[本地服务契约](../../contracts/local-service-contract.md)、[流水线与本机历史](../../architecture/pipeline.md)

## 背景与问题

剪贴板历史除了原始内容本身，存在一类共性极强的「派生富化（Enrichment）」需求：

1. **链接预览**：复制一条 URL 后，拉取网页标题、简介与站点图标，在桌面/移动端展示语义化链接卡片，并支持按网页标题/简介检索链接；
2. **图片 OCR 与预览划选文字**（后续演进目标）：识别图片中的文字写入搜索索引，并在图片预览中提供类似 macOS Live Text 的直接划选复制；
3. **其他同类扩展**：PDF/文档/音视频内容提取检索、图片二维码解析、EXIF 提取、代码语言识别、本地 VLM 摘要等。

对照现有架构，当前 3 种普通插件能力均无法承载这类需求：

- **`processor`**（见 [engine/pipeline.go](../../../engine/pipeline.go)）：同步阻塞在 `commit` 与 `sync` 之前，硬限制 200 ms 超时，且 `plugin.Item` 不提供二进制流读取，只允许修改标签/过期/持久化/同步策略；网络抓取或模型推理绝对不能放进同步捕获路径。
- **`observer`**（见 [plugin/plugin.go](../../../plugin/plugin.go)）：仅接收无正文的 `ItemID` 只读通知，无法读取内容，也不能回写任何派生元数据或搜索文档。
- **`action`**（见 [Action 契约](../../contracts/clipboard-action-contract.md)）：由用户显式触发，且强语义为“创建一条新的 `origin=action` 文本历史记录”，而非为既有历史条目补充只读派生元数据、搜索索引与预览叠加层。

同时，[历史搜索契约](../../contracts/history-search-contract.md) 目前明确禁止抓取 URL 或执行 OCR，并假设每条历史的搜索索引都在 commit 事务内同步完成（`ready=1`），任意未就绪条目都会使全文搜索返回 `unavailable`。

## 核心决策

### 1. 基础设施内置 + 富化提供者插件化（`builtin` / `external` 同契约）

坚持既有的宿主与插件职责边界：

- **Engine 与各端宿主拥有**：派生元数据与小附件的统一存储、级联删除与配额管理、搜索索引集成、异步调度队列与并发限流、以及各端原生预览 UI 渲染（桌面 `mygo`/BoardUI、移动端 Expo）。插件**不自行维护第二份数据库或搜索索引，也不注入任何可执行 UI 代码**。
- **富化提供者（Enricher）通过统一插件契约装配**：无论是未来随宿主进程内装配的原生实现（例如 macOS/iOS Vision、Windows WinRT、Android ML Kit 等 `builtin.ocr`），还是通过插件市场独立分发的外部 Go 插件（例如首个链接预览插件，以及未来的 Linux RapidOCR/VLM OCR 插件），均遵守同一套 `enricher` 能力契约。
- **零无关开销**：当未安装或未启用任何声明 `enricher` 能力的插件时，普通剪贴板捕获、历史存储、搜索与跨设备同步不产生额外文件读取、解码、内存占用或网络请求。

### 2. 以「链接预览」作为首个闭环切片，先跑通再做 OCR

不直接以高复杂度的跨平台图片 OCR（涉及 OS 原生桥接、模型分发、`mygo` 画布级文字划选与手势消歧）作为首个验证对象，而是先以 **独立外部 Go 插件「链接预览」（`unic.linkpreview`）** 完成全链路闭环：

- 桌面端（[app/desktop/internal/desktopui/history.go](../../../app/desktop/internal/desktopui/history.go) 与 [app/desktop/internal/desktopkit/grid_grouped.go](../../../app/desktop/internal/desktopkit/grid_grouped.go)）已具备详情页 `link-preview` 与网格 `KindLink` 语义卡片骨架，目前仅有域名、通用图标和裸 URL，接入后可直观验证 UI 升级与无插件降级体验；
- 链接预览涉及对外网络请求、耗时异步执行、结构化卡片元数据（含小图标附件）与派生搜索文档入库，能完整压测 `enricher` 的异步调度、超时取消、RPC 契约、搜索解耦、缓存配额、隐私边界与热启停生命周期；
- 闭环验收完成后，图片 OCR 仅需在同一底座上新增 `text_overlay.v1` 注解协议、原生预览选字画布和 OCR 提供者，无需推翻核心设计。

---

## 通用 `enricher` 能力与 Hook 设计

### 1. 插件能力声明

在 [插件契约](../../contracts/plugin-contract.md) 中新增能力 `CapEnricher = "enricher"`（首版支持 builtin Go 与外部 Go 进程插件；Lua 沙箱暂不开放，待需求明确后另定）。

声明 `enricher` 的插件须在 `Spec`（及清单 `spec.toml`）中静态声明 `Enricher *EnricherSpec`：

```go
type EnricherSpec struct {
	Accepts      []string // 非空唯一子集: "text", "image", "files"
	RequiredTags []string // 可选：仅当条目包含全部指定标签时调度（如 ["builtin:link"]）
	Triggers     []string // 非空唯一子集: "on_commit", "on_demand", "manual"
	Annotations  []string // 可选：声明会产出的标准注解类型（如 ["link_preview.v1"]）
	TimeoutMS    int64    // 0 使用默认 15000 ms；上限 120000 ms
}
```

关键约束：
- **`RequiredTags` 预过滤**：允许 Engine 在调度前直接跳过不匹配条目（例如链接预览仅关注 `builtin:link`），避免对普通剪贴板文本发起无意义 RPC。
- **不改变既有阶段顺序**：`enricher` 仅在条目成功 commit 入历史（`persist=true`）之后运行；`persist=false` 的临时提交不进入富化队列。

### 2. 调用上下文与流式输入

复用 Go Action 已验证的二进制流读取机制（见 [Action 契约](../../contracts/clipboard-action-contract.md#go-action-source-content)），向插件暴露只读 `EnrichContext`：

- `Trigger() string`：本次调用来源（`on_commit` / `on_demand` / `manual`）；
- `Item() Item`：条目只读元信息与文本快照（同 `ProcessContext.Item()`）；
- `ActionSource`（`Parts()` / `OpenPart(ctx, index)`）：面向图片/文件类 enricher 的受控只读流，仍由 Engine 持有内容租约并在调用结束/取消时回收，绝不暴露宿主文件路径。
- 产出结果通过单次 `SetResult(EnrichmentResult)` 返回，不可原地修改条目正文、过期时间或同步策略：

```go
type EnrichmentResult struct {
	Status      string                 // "ok" 或 "skipped"（条目不符合插件内部条件时返回 skipped，不计错误）
	SearchDocs  []EnrichmentSearchDoc  // 可选：写入搜索索引的派生文本
	AddTags     []string               // 可选：追加标签（不可删除既有/用户标签）
	Annotations []EnrichmentAnnotation // 可选：结构化派生元数据与小附件
}

type EnrichmentSearchDoc struct {
	Kind string // 如 "link_title", "link_desc", "ocr"（受限标识符）
	Text string // 有效 UTF-8，单条上限 64 KiB，单次总上限 256 KiB
}

type EnrichmentAnnotation struct {
	Type        string            // 版本化标准类型，如 "link_preview.v1"、"text_overlay.v1"
	PayloadJSON []byte            // 严格符合该类型 Schema 的 JSON，上限 256 KiB
	Assets      []EnrichmentAsset // 可选有界二进制附件（如 favicon PNG/WebP）
}

type EnrichmentAsset struct {
	Key  string // 注解内逻辑键，如 "favicon"、"thumbnail"
	MIME string // 允许列表：image/png、image/webp、image/jpeg
	Data []byte // 受严格上限控制（见配额约束）
}
```

### 3. Engine 异步调度与生命周期

- **完全非阻塞**：`commit` 事务提交后向内存有界富化队列投递任务，绝不阻塞剪贴板观察、本地落库、事件发布或跨设备同步。
- **优先级与并发限流**：
  - `on_demand`（用户正在预览该条目）与 `manual` 优先于 `on_commit` 后台队列；
  - 默认每插件并发度为 1（后续可按上限放宽至 2），全局有界队列防背压与内存膨胀；队列满时丢弃最旧未开始的后台任务并记为待按需补跑，不阻塞主路径。
- **幂等与去重**：
  - 数据库按 `(item_seq, plugin_id)` 记录富化状态（`status`、`plugin_version`、`updated_ms`）。
  - 同一条目在同一插件版本已处于 `ok` 或 `skipped` 终态时，不重复触发；用户显式 `manual` 重跑或插件版本升级后可刷新。
- **取消与一致性**：
  - 富化运行期间若目标历史条目被删除或到期，写回事务因外键/存在性校验直接丢弃结果并释放附件，不复活记录；
  - Engine `Stop` / `Suspend` 立即取消所有在途富化 context，释放内容租约并等待有界回收；
  - 富化成功写入新注解、标签或搜索文档后，递增 `HistoryRevision` 并发出标准 `history` 更新事件（保持无正文），驱动桌面/移动端无轮询刷新当前视图。

---

## 搜索与存储契约边界

### 1. 基础搜索就绪态与派生富化状态彻底解耦

对 [历史搜索契约](../../contracts/history-search-contract.md) 与 [internal/store/search.go](../../../internal/store/search.go) 做最小但关键的扩展：

- `history_search_docs` 新增来源列：`source`（基础索引为 `'base'`，富化文档为 `'enricher:' + pluginID`）与 `doc_kind`（如 `'text'`、`'filename'`、`'link_title'`、`'link_desc'`、`'ocr'`）。
- **就绪态隔离（硬不变量）**：`history_search_state.ready` **仅表示 commit 事务内同步完成的基础索引**。任何异步富化任务处于 `pending`、`failed`、`skipped` 或超出派生预算时，**绝不置 `ready=0`，绝不导致 `SearchHistory` 返回 `ErrSearchIncomplete`**。
- **配额独立与退让**：派生搜索文档计入既有搜索总预留配额（或在其内设独立子上限，如默认不超过搜索预算的 50%）；预算不足时跳过写入派生文档并记录状态，既不破坏基础搜索，也不驱逐历史正文。
- **搜索命中预览**：当 `SearchHistory` 命中派生文档时，保留既有 512 rune 有界预览与半开高亮偏移，并附带 `MatchKind`（如 `base` / `link_preview` / `ocr`），使 UI 能准确提示“命中网页标题/简介”或“命中图片文字”。
- **卸载与停用清理**：停用或卸载某个 `enricher` 插件后，不强制破坏历史记录；提供清理该插件来源的派生搜索文档与注解的能力（卸载时可选择清理或随历史自然淘汰），且禁用期间新记录零触发。

### 2. 派生注解存储与访问边界

- 新增私有表 `history_annotations(item_seq, plugin_id, ann_type, plugin_version, status, payload_json, updated_ms)` 与 `history_annotation_assets(annotation_id, asset_key, mime, blob_ref, byte_size)`，外键关联 `clip_items(seq) ON DELETE CASCADE`。
- 附件字节计入 Engine 历史存储配额 `MaxStoredBytes`，删除、过期或容量淘汰历史记录时同步级联释放关联注解、附件 blob 与 FTS 文档。
- **仅本机派生，不同步**：与标签和搜索索引一致，派生注解与派生搜索文档**不跨设备同步**（避免同步载荷膨胀、隐私外泄及多端版本冲突；各端按自身配置与能力本地派生）。
- **读取边界**：
  - `ListHistory` 与事件流继续保持 content-free 与轻量化，不内联大段注解 JSON 或附件字节；
  - Engine 新增按需读取与手动触发接口：`GetHistoryAnnotations(ctx, historyID)`、`OpenHistoryAnnotationAsset(ctx, historyID, pluginID, annType, assetKey)` 与 `EnrichHistory(ctx, historyID, pluginID)`；本地服务暴露对应 `/v1/history/{id}/annotations` 路由；
  - 桌面端列表/网格视口预取机制（`live_details.go`）可按现有可见窗口有界拉取轻量注解摘要，保持既有内存与并发上限。
- **多插件槽位仲裁**：
  - `SearchDocs` 与 `AddTags` 允许多个 `enricher` 并存（按 `plugin_id` 隔离）；
  - 同一种展示型注解（如 `link_preview.v1`、`text_overlay.v1`）若存在多个启用提供者，宿主按 `plugins.order` 优先级采用首个有效结果，防止同一界面槽位叠加渲染。

---

## 标准化注解 Schema（首批与演进预留）

### 1. 首批落地：`link_preview.v1`

由 Engine 严格校验字段长度、UTF-8 合法性与 URL scheme（仅允许 `http://` / `https://`）：

```json
{
  "canonical_url": "https://example.com/article",
  "site_name": "Example",
  "title": "网页标题",
  "description": "页面摘要描述",
  "favicon_asset": "favicon",
  "image_asset": "thumbnail"
}
```

硬上限：
- `canonical_url` ≤ 2048 字节；`site_name` ≤ 128 字符；`title` ≤ 256 字符；`description` ≤ 512 字符；所有文本字段拒绝控制字符；
- 至少提供非空 `title` 或 `description` 之一才视为有效预览；
- `favicon` 附件 ≤ 64 KiB（安全解码并缩放重编码为 ≤ 64×64 PNG/WebP）；`thumbnail`（网页 `og:image` / `twitter:image` 封面图）默认开启抓取，下载受限于 ≤ 4 MiB 原始响应与安全像素上限，经缩放重编码后存储上限 ≤ 256 KiB（长边 ≤ 640 px，去除 EXIF 与非光栅脚本格式）。

### 2. 演进预留：`text_overlay.v1`（二期图片 OCR 复用同一底座）

二期接入图片 OCR 时新增该注解类型，不在本期实现，但本期数据结构与存储边界提前保证兼容：
- 基于 **EXIF 旋转校正后** 的图像归一化坐标 `[0, 1] × [0, 1]`；
- 层级结构为 `blocks -> lines -> optional words/char_boxes`（含文本、bbox/四点 `quad`、阅读顺序与语言标签）；
- 行级坐标必填、词/字符级坐标可选，使宿主在 Apple Vision 等原生引擎下可提供逐字高精度划选，对仅输出行框的外部 OCR 插件自动按字宽插值降级。

---

## 首个闭环插件：内置「智能预览」（`builtin.preview`）与外部扩展开放

### 1. 形态与演进定位

- **内置默认开启、用户可关闭的通用预览插件 `builtin.preview`（Smart Preview / 智能预览）**：
  - 随服务与移动端进程内装配（`internal/plugins/builtinpreview/`），默认开启，用户可在桌面端「设置 → 插件 → 内置功能 → 智能预览」或 `config.toml`（`[plugins.instances."builtin.preview"]`）中一键关闭整项能力，或精细关闭单个子预览（`link_preview`、`fetch_favicon`、`fetch_thumbnail`）并配置 `excluded_domains`。
  - 内部按模块化 provider 组织：首版提供网页链接预览（`link_preview.v1`，默认抓取 `og:image` / `twitter:image` 封面图与 `favicon`）；未来可持续扩展验证码预览（`otp_preview.v1`）等更多内置预览，而无需拆分碎片化插件。
- **预览能力同样完全开放给第三方插件**：
  - `builtin.preview` 与外部 Go 插件完全共享同一套 `CapEnricher` 能力、派生搜索与版本化注解契约（`link_preview.v1` 等），不享有任何私有通道；
  - 第三方外部 `enricher` 插件既可新增新的预览注解类型，也可通过 `plugins.order` 排在 `builtin.preview` 之前覆盖同类型预览（或在设置中关闭对应内置子预览）；
  - 仓库内保留 `plugins/examples/linkpreview/`（`unic.linkpreview`）作为外部 Go `enricher` 插件的参考实现与真实子进程 RPC 端到端测试夹具。

### 2. 抓取策略与安全护栏

- **严格匹配范围**：
  - `spec.toml` 声明 `capabilities = ["enricher"]`、`accepts = ["text"]`、`required_tags = ["builtin:link"]`、`triggers = ["on_commit", "on_demand", "manual"]`、`annotations = ["link_preview.v1"]`。
  - 插件内部仅处理“去除首尾空白后整段文本为单一合法 `http://` 或 `https://` URL”的条目；对于夹杂链接的多行段落、代码或聊天记录，直接返回 `status = "skipped"`，不发任何请求。
- **SSRF 与内网防护（默认强制）**：
  - 禁止非 `http`/`https` 协议及含内嵌凭据（`user:pass@host`）的 URL；
  - 解析 DNS 后校验全部目标 IP，默认拒绝回环地址（`127.0.0.0/8`、`::1`）、私有地址（`10.0.0.0/8`、`172.16.0.0/12`、`192.168.0.0/16`、`fc00::/7`）、链路本地地址（`169.254.0.0/16`、`fe80::/10`）、未指定/组播地址以及 `.local` / `.internal` 主机名；
  - 每次 HTTP 重定向（上限 5 次）均重新执行相同校验，并对同一目标使用已校验 IP 建连以防止 DNS Rebinding。
- **有界无状态请求**：
  - 仅发送无 Cookie、无授权头、无 Referer 的 `GET` 请求；
  - 仅读取 `text/html` / `application/xhtml+xml` 响应的前 512 KiB 解析 `<title>`、`og:title`、`twitter:title`、`og:description`、`meta[name=description]`、`og:site_name`、`og:image` / `twitter:image`、`link[rel=canonical]` 与 favicon 链接；
  - 拉取 `favicon` 时沿用相同的 SSRF 校验、≤ 256 KiB 响应上限与安全图片解码/重编码为有界 PNG（最大 64×64），剔除 SVG 脚本与异常元数据；
  - 默认拉取网页封面图（`og:image` 优先，回退 `twitter:image`），沿用相同的 SSRF 校验、≤ 4 MiB 原始下载上限与像素上限校验，安全解码后按比例缩放至长边 ≤ 640 px 并重编码为 ≤ 256 KiB 的 JPEG/PNG/WebP；封面图抓取失败或超限时降级忽略图片，不影响标题与摘要落库；
  - 遵循配置化的单请求超时（默认 8 秒）。
- **Schema 设置项（复用 [0010](0010-plugin-settings-schema.md)）**：
  - `auto_fetch`（bool，默认 `true`：新复制链接自动抓取；设为 `false` 时仅在预览/手动触发时抓取）；
  - `fetch_favicon`（bool，默认 `true`）；
  - `fetch_thumbnail`（bool，默认 `true`：默认抓取网页 `og:image` / `twitter:image` 封面图，可手动关闭以节省流量与存储）；
  - `excluded_domains`（string 数组，支持用户排除公司内网域名、网盘或敏感站点，如 `*.corp.example.com`）。

### 3. 桌面端 UI 闭环

直接复用并升级既有组件，不引入新的独立窗口或插件专用页面：

- **详情页语义卡片**（[app/desktop/internal/desktopui/history.go](../../../app/desktop/internal/desktopui/history.go)）：
  - 当存在 `link_preview.v1` 注解时：顶部展示有界比例的网页封面图（若存在 `thumbnail` 附件，无封面图则自然折叠不占空白），站点栏用真实 `favicon` 替换通用 `IconLink` 并优先展示 `site_name`（回退为域名），正文突出展示网页 `title` 与两行有界 `description` 摘要，底部保留原有等宽 `url-box` 与显式打开按钮；
  - 当正在按需加载或无注解（插件未安装/禁用/离线/抓取失败）时：无缝保持现有域名 + URL 卡片降级样式，绝不出现空白骨架卡死或错误弹窗。
- **网格模式卡片**（[app/desktop/internal/desktopkit/grid_grouped.go](../../../app/desktop/internal/desktopkit/grid_grouped.go)）：
  - `KindLink` 卡片在存在 `link_preview.v1` 时展示可选封面缩略图、真实 favicon、网页标题与域名/路径，命中搜索关键词时延续现有高亮渲染。
- **历史搜索体验**：
  - 用户在搜索栏输入网页标题或描述中的任意子串，即可直接搜出对应链接条目，搜索预览摘录高亮显示命中的网页标题/摘要。

---

## 从链接预览到图片 OCR 的演进路线

本决策落地后，后续推进图片 OCR 需求只需按以下增量步骤叠加，核心契约保持向后兼容：

1. **阶段 A（本决策范围 — 闭环验证）**：落地 Engine `enricher` 调度、派生搜索/注解存储、RPC 扩展、本地服务 API、桌面链接卡片渲染与首个外部插件 `unic.linkpreview`。
2. **阶段 B（图片 OCR 骨架与原生预览选字）**：
   - 新增标准注解 Schema `text_overlay.v1`（归一化多边形/文本框）；
   - 在桌面端 [QuickLook](../../../app/desktop/internal/desktopkit/quicklook.go)、详情页图片预览与 [Quick Panel 图片预览](../../../app/desktop/internal/desktopui/quick_panel_image.go) 中实现宿主内置的文本叠加划选层（I-beam 指针、跨行选区、双击选词、全选、`Cmd/Ctrl+C` 复制选区、与 `actualSize` 平移手势消歧）。
3. **阶段 C（开箱即用原生引擎 + 社区扩展）**：
   - 新增进程内 `builtin.ocr`（macOS/iOS 接 Apple Vision、Windows 接 WinRT OCR、Android 接 ML Kit），沿用同一 `enricher` 契约注册；
   - 面向 Linux 与高阶用户通过插件市场提供独立外部 Go OCR 插件（如 `unic.rapidocr`、`unic.vlm-ocr`），可与 `builtin.ocr` 按优先级切换或同时注册 `action`（一键整图转文字）。

---

## 否决的备选方案

- **放宽 `processor` 超时并开放图片/网络读取**：会拖慢剪贴板捕获、落库和跨设备同步，破坏 `processor` 200 ms 有界确定性模型，且无法处理按需触发与历史补跑。
- **在 Engine 内置 URL 抓取或硬编码 OCR 模型**：违反宿主/核心无无关网络请求与轻量化发布原则，会导致 Linux 依赖膨胀、隐私边界模糊，也无法让用户自由替换引擎。
- **允许插件直接维护私有数据库或注入自定义渲染脚本/HTML**：违背 [历史搜索契约](../../contracts/history-search-contract.md) 的唯一索引所有权与原生 `mygo`/Expo 跨端呈现边界，无法统一支持组合搜索、配额回收与多端一致渲染。
- **把 `enricher` 失败记为 `history_search_state.ready=0`**：会导致断网或单个站点不可达时整个历史全文搜索返回 `unavailable`，不可接受。
- **跨设备同步派生注解与 OCR 结果**：会放大同步传输体积、引发多端覆盖冲突并泄露隐私派生文本，不符合本地标签/索引仅本机生效的一贯设计。

---

## 失败与隐私模型

实现与测试前需明确覆盖以下故障与隐私边界：

- **故障与竞态**：目标站点超时、HTTP 4xx/5xx、超长响应、非法 UTF-8/畸形 HTML、非图片或损坏 favicon、SSRF/内网/DNS 重定向拦截、插件进程崩溃或超时被杀、队列满载丢弃、富化进行中历史条目被删除或过期、搜索/存储配额耗尽、Engine `Stop`/`Suspend` 中止在途任务。
- **隐私红线**：URL、网页标题/描述、图标字节与原始插件错误文本严禁写入日志、事件、遥测或错误消息；`ListHistory` 保持 content-free；仅在显式搜索预览与按需注解接口中返回有界派生内容。

---

## 实施顺序与验收边界

1. **契约先行**：修订 [插件契约](../../contracts/plugin-contract.md)、[插件 RPC 核心协议](../../contracts/plugin-rpc-contract.md)、[本机历史契约](../../contracts/history-contract.md)、[历史搜索契约](../../contracts/history-search-contract.md) 与 [本地服务契约](../../contracts/local-service-contract.md)，补齐 `enricher` 帧/DTO 机器可读 Schema 与互操作向量。
2. **Engine 与存储层**：实现 `history_annotations` / `history_annotation_assets`、来源隔离的 `history_search_docs`（与基础 `ready` 解耦）、有界异步富化调度器、配额/级联删除与进程内/外部 Go RPC 代理。
3. **本地服务与 CLI**：暴露 `/v1/history/{id}/annotations`、附件读取与显式触发端点；`unic plugin` 与市场元数据支持 `enricher` 能力展示。
4. **首个闭环插件 `unic.linkpreview`（先主仓闭环，后拆仓发布）**：先在 `plugins/examples/linkpreview/` 实现受限 HTML 元信息、favicon 与默认 `og:image` 封面图抓取、SSRF 防护与 Schema 配置，接入本地假 HTTP 服务的真实子进程端到端测试（覆盖正常抓取、中文/编码解析、封面图缩放重编码、重定向、SSRF 拦截、超时、大包截断、禁用/未安装零网络请求）；全链路验收后再拆出 `UniClipboard/unic-plugin-linkpreview` 发布预编译包并登记插件市场。
5. **桌面端 UI 闭环**：升级详情页与网格 `KindLink` 卡片消费 `link_preview.v1` 注解（含封面图、favicon、标题与摘要），并验证：
   - 插件启用时：复制链接 → 后台自动富化 → 卡片实时更新封面图/标题/摘要/图标 → 搜索网页标题命中并高亮；
   - 插件未安装与已安装但禁用时：普通复制、历史列表、搜索与同步零额外读取/网络调用，链接卡片保持原有优雅降级。

本决策完成实现与端到端验收后，将现状与不变量合并进对应架构页与契约，再移入 `archive/`。
