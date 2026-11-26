# 0035 · 混合同步传输：P2P 优先、E2EE 临时 HTTP 中转与分块接力续传

- 状态：进行中（Phase 0 与 Phase 1 已实现并验证；Phase 2 大载荷 P2P 中途接力续传与 Phase 3 托管上线/移动端后台演进待实施）
- 日期：2026-11-26
- 关联：[0007 多同步后端与分层策略](0007-multiple-sync-backends.md)、[0014 客户端可观测性](0014-client-observability-otlp.md)、[0020 Tailcat 同步可观测性](0020-tailcat-sync-observability.md)、[0029 可扩展同步后端集成](0029-extensible-sync-backend-integration.md)、[0031 移动端集成 Engine](0031-mobile-engine-integration.md)、[0034 同步发送进度上报与对端接收完成回执](0034-sync-transfer-progress-and-receipt-ack.md)
- 落地目标：[流水线与本机历史](../../architecture/pipeline.md)、[设备身份、信任与配对](../../architecture/identity-and-trust.md)、[混合中转同步契约](../../contracts/hybrid-relay-contract.md)、[同步后端业务契约](../../contracts/sync-backend-contract.md)、[Engine 公共契约](../../contracts/engine-contract.md)
- 协议契约草案：[混合中转同步契约](../../contracts/hybrid-relay-contract.md)

## 背景与问题

在当前架构下，设备间原生同步仅依赖 `builtin.tailcat`（`peer_sessions` 长连接）：

1. **移动端无法长期维持 P2P 连接**：iOS 禁止应用后台常驻网络进程，Android 受 Doze 与后台网络策略限制。`app/expo` 退后台时会调用 `SetRuntimeState(suspended)` 关闭会话层（见 `internal/session/engine.go`）；冷启动或由系统分享入口（iOS Share Extension / Android Share Intent）唤起主 App 后，`builtin.tailcat` 需依次完成 DERP 区域发现、WireGuard 拨号与 Ed25519 三步身份握手，通常耗时 2–8 秒。
2. **现有发送路径在未建连时直接拒绝**：`engine/peersend.go` 中的 `Submit` 仅向当前处于 `connected` 状态的 `liveTargets()` 派发任务；用户分享内容并点击发送时，若 P2P 尚未建连，Engine 立即返回 `no_targets`，移动端发送弹窗只能提示“设备离线”，无法像普通 HTTP 上传那样**立刻开始传输**。
3. **仅靠多后端盲目双发无法满足混合体验**：[0007](0007-multiple-sync-backends.md) 的多后端派发是对全部启用后端独立并发发送。若将 HTTP 中转简单并列为第二个独立后端：
   - 当 P2P 已经在线时仍会重复上传中转，白白消耗中转带宽、流量费用与移动端电量；
   - 当大文件（例如 10–100 MiB 的视频/多图）在冷启动第 0 秒开始走 HTTP 上传、第 3 秒同局域网 P2P 恢复时，两个互不知情的后端无法结算已上传字节并由 P2P 接力续传剩余部分。

## 目标与非目标

### 目标

1. **0 等待启动传输**：目标设备 P2P 已在线时直接走 P2P；未连接时 **0 等待**立即走 HTTPS 启动端到端加密（E2EE）上传，同时异步唤醒 P2P 建连供后续或中途接力使用。
2. **官方托管开箱即用 + 支持自托管**：提供默认官方托管 E2EE 临时中转服务（如扩展现有 `rendezvous.uniclipboard.app` 同生态基础设施，采用 Cloudflare Workers + Durable Objects + R2），同时定义极简、无供应商锁定的 Relay HTTP API v1，允许用户通过配置切换为自托管 URL 或完全禁用。
3. **零知识 E2EE 与无账号信任模型**：复用 [统一设备身份与配对协议](../../contracts/identity-protocol.md) 的 Ed25519 设备密钥与已批准信任关系，不引入账号体系，不向中转服务器暴露明文内容、文件名、MIME 类型或来源应用信息，服务器既不能解密也无法伪造来源。
4. **大载荷统一分块与中途 P2P 结算续传（Level 3）**：中转与 P2P 接力共享同一套固定分块加密格式；大载荷在 HTTP 上传中途若目标 P2P 恢复，双方通过位图结算已收/已传分块，剩余分块无缝切至 P2P 续传并在接收端统一组装、校验、提交一次历史与接收记录。
5. **零无关开销与依赖边界不变**：关闭中转时不对普通捕获、历史、P2P 同步产生额外 I/O、内存或网络开销；保持 `engine/boundary_test.go` 单向依赖边界。

### 非目标

- 不以 HTTP 中转取代 `builtin.tailcat` 作为常驻与超大文件主通道（中转设有大小上限与短 TTL）。
- 不承诺无限期离线可靠投递、云端历史备份或多设备云端同步仓库（中转对象默认 10 分钟 TTL，消费确认后即删，最长保留不超过 30 分钟）。
- 不在本阶段修改既有 `unic-pair` 配对码语义，也不要求已配对设备重新配对即可启用中转。
- 不在 Phase 1/2 将 iOS Share Extension 改造为独立直传进程（遵守 [0031](0031-mobile-engine-integration.md) 不在扩展内启动第二 Engine 的约束；但为 Phase 3 可选只读快照直传预留演进边界）。

## 核心决策

### 0. 术语与部署边界：两类独立中继程序

为避免概念混淆与架构耦合，明确区分 `unic` 体系中可能并存的**两类完全独立的中继程序**：

| 维度 | Tailcat P2P 中继节点（DERP Relay） | 混合传输 HTTP 中转服务（E2EE Mailbox / `unic-relay`） |
|---|---|---|
| **作用层次** | 传输/隧道层（协助 `tailcat` NAT 穿透与 WireGuard 流量转发） | 应用/分块层（0 等待端到端加密分块存储转发与 SSE 推送） |
| **是否存储载荷** | 否，纯在线实时字节转发；两端必须先完成隧道与握手 | 是，短 TTL（默认 10 分钟）暂存密文分块，领取确认即删 |
| **程序与归属** | 独立 DERP 服务程序（如 Tailscale `derper`），归属 `builtin.tailcat` / `internal/tcnet` 传输栈 | 独立 HTTP 服务程序（官方 Workers+DO+R2 或自部署 `unic-relay`），归属 `0035` 混合中转协议 |
| **自部署关系** | 与混合 HTTP 中转**完全正交、互不依赖**：用户可单独自部署其一、同时自部署两者（或经反代挂于同一宿主机的不同路径/端口），协议与进程生命周期严格解耦 | 同左；`0035` 不修改或耦合 DERP 节点程序 |

下文如无特别说明，“中转”均指后者（混合传输 E2EE 临时 HTTP 中转服务 `unic-relay`）。

### 1. 客户端架构归属：Engine 统一混合调度 + 独立内置中转模块（非外部插件，非硬塞进 `tailcat`）

客户端不将中转做成外部子进程插件（[0031](0031-mobile-engine-integration.md) 已明确移动端不支持 `pluginproc`），不将其做成与 `tailcat` 平级、需要二次配对审批的独立 `sync_backend`，也不把 E2EE 逻辑硬塞进 `builtin.tailcat`（违背设备私钥不出 Engine 的边界且阻碍未来复用到其他 P2P 后端）。

采用与现有 `internal/rendezvous` 一致的**三层协同归属**：

- **Engine 核心 + `internal/relaycrypto`**：遵循 [identity-protocol.md](../../contracts/identity-protocol.md)，设备 Ed25519 私钥永不出 Engine；由 Engine 统一负责逐目标路由判定（P2P 优先 / 离线 0 等待回退）、信封加密/解封与签验、对端信任与单路径策略校验（**直接复用既有已批准设备，无需用户二次配对或二次审批**），以及共享接收分块暂存组装器（`TransferAssembly`）。
- **独立内置中转模块（`internal/relayclient`）**：纯标准 HTTPS 客户端实现 Relay HTTP API v1，不依赖 `tailcat`/`tcnet`；由宿主（桌面 `app/service` 与移动端 `bindings/mobile`）按配置显式装配进 Engine，支持官方默认 URL、自部署 URL 或一键禁用。
- **`builtin.tailcat`（`internal/session`）最小配合**：仅在已认证 P2P 会话上新增轻量控制帧（交换 `relay_profile`、响应快速重连提示 `WakePeer`，以及 Phase 2 搬运接力控制帧与密文块），不持有设备私钥，不自行决定中转回退。
- **共享接收组装器（`TransferAssembly`）**：对走分块路径的同一笔 `(SourceDeviceID, TransferID)`，HTTP 拉取与 P2P 接力帧共同写入同一个受 `MaxStoredBytes` / `InboxDir` 配额约束的有界暂存对象；全块集齐并验签、验哈希后，统一走一次既有的 `admitReceive` → `received` 处理器 → 历史提交与跨后端去重。

### 2. 混合路由策略：P2P 在线直连优先，离线 0 等待回退

每次提交（自动同步与显式发送）针对每个授权目标设备 `D` 独立评估路由：

1. **优先直连**：若 `D` 在 `builtin.tailcat` 上已有有效且授权匹配的活跃会话 → **直接走 P2P**，完全不触碰 HTTP 中转。
2. **0 等待立即回退中转**：若 `D` 当前无活跃 P2P 会话，且同时满足：
   - 全局同步启用、本机中转已启用；
   - `D` 为 `trusted`、关联已授权且 `sync_enabled=true`（自动同步同时遵守单路径 `DeviceBinding.Policy`）；
   - 已持有 `D` 有效的中转路由信息（`mailbox_id` + `delivery_token` + 允许的 `relay_url`）；
   - 本次发送视图总字节数 ≤ 中转单次上限（官方默认 **25 MiB**，自托管可配置至不超过 `limits.max_total_bytes`）；
   则 **0 等待立即启动 HTTP 中转上传**，同时向 `builtin.tailcat` 发出非阻塞重连提示（重置退避并立即拨号），为大载荷中途接力或后续新提交预热 P2P。
3. **超限且离线处理**：若载荷超过中转上限且 `D` 当前 P2P 不在线，不走中转，返回明确原因（如 `relay_size_exceeded` / `no_targets`），不在后台悄悄无限排队。
4. **策略与撤销一致性**：中转不构成绕过单路径策略的后门。`sync.enabled=false`、`Suspend`、暂停关联同步、移除关联或 `UnpairDevice` 立即取消在途中转上传/下载并触发撤销屏障。

### 3. 零知识 E2EE、无账号信箱与防滥用鉴权

- **密钥体系（零重新配对）**：
  - 复用每台设备既有的 Ed25519 身份密钥对；通过标准 Edwards→Montgomery 双有理映射（`filippo.io/edwards25519` + `crypto/ecdh` X25519）派生加密密钥，现有已配对设备升级后无需重新配对即可端到端加密。
  - 每笔传输生成随机 256-bit 内容密钥 `CEK` 与一次性临时 X25519 密钥对，经 `HKDF-SHA256` 派生密钥封装密钥（Wrap Key）并用 `AES-256-GCM` 封装 `CEK`，AAD 绑定完整规范化信封头；载荷分块使用 `AES-256-GCM` 加密，每块唯一 nonce 绑定 `chunk_index` 与 `is_final`。
  - 外层信封由发送方 Ed25519 私钥签名，**仅包含路由与分块完整性元数据**；剪贴板正文、文件名、MIME 类型、part 数量与来源应用信息全部封在密文流内部，中转服务端完全不可见。
- **信箱路由与双向鉴权**：
  - 接收端信箱标识 `mailbox_id` 由 `(relay_origin, recipient_pubkey)` 经域分离哈希派生，服务端不保存明文设备名。
  - **读信箱鉴权**：接收端订阅事件流（SSE）或下载/确认删除对象时，必须使用本机 Ed25519 私钥对带时间戳与 nonce 的请求签名，服务端校验签名与 `mailbox_id` 一致性，第三方无法窃读或恶意删除。
  - **投递防滥用（Delivery Token）**：设备间在 P2P 认证会话建立时自动交换轻量控制帧 `relay_profile`，向已信任对端签发该对端专属的投递令牌 `delivery_token`（可由接收方随时按设备轮换/吊销，阻止已解除配对的设备向信箱灌入垃圾流量）。该令牌与 `mailbox_id` 视为秘密，仅存本机受保护存储，不进普通 DTO、日志或事件。
  - **SSRF 防护**：客户端仅允许向本机配置的官方或显式允许的自托管 `relay_url` 发起请求，绝不盲目请求对端自报的任意内网或未知地址。

### 4. 统一分块模型与 Level 3 中途 P2P 结算续传

为同时满足“小内容极致 1-RTT 秒发”与“大文件 P2P 恢复零浪费续传”，所有混合传输采用同一逻辑打包流与分块定义：

- **统一逻辑流**：`u32_be(len(clip_header_json)) || clip_header_json || part[0] || part[1] || ...`（复用现有 `wire.Clip` 元数据与 `clip.Snapshot.Hash` 语义）。
- **单块快路径（≤ 2 MiB）**：`total_chunks = 1`，单次 HTTPS 请求直接携带签名信封 + 唯一密文块（1 RTT 完成上传，几乎所有文本、链接、截图直接走此路径）。
- **固定分块路径（> 2 MiB）**：按默认 **1 MiB** 明文块切片独立 AEAD 加密；先 `POST` 提交签名信封（含 `chunk_hashes[0..N-1]`），再以有界并发（默认 2–3）`PUT` 上传分块；每块独立校验、独立记入 `relay_uploaded` 位图。
- **Level 3 结算与 P2P 续传（Phase 2）**：
  - 沿用 [0034](0034-sync-transfer-progress-and-receipt-ack.md) 在 `wire v1` 确立的 `Hello.Caps` 能力协商与无死锁异步控制写队列：Phase 1 协商 `relay_profile`，Phase 2 协商 `relay_handoff`（`wire.Version` 保持为 `1`）。
  - 当大载荷处于 `relay_uploading` 且目标 P2P 会话变为 `connected`（且双方已协商 `relay_handoff`），且剩余未上传量 ≥ 切换门槛（默认 **2 MiB** 或 ≥ 2 块）时：
    1. 发送端停止派发新的 HTTP 分块上传，在 P2P 会话发送 `relay_handoff_offer`（含 `transfer_id`、信封与 `relay_uploaded_bitmap`）；
    2. 接收端核查本地共享 `TransferAssembly` 状态并回复 `relay_handoff_ack`（含 `have_bitmap` 与 `want_p2p_bitmap`）：接收端已落盘校验的块绝不重传；未上传到中转的块全部走 P2P 推送；已上传到中转但接收端尚未拉完的块，接收端可按本机策略继续从 HTTP 拉完或直接纳入 `want_p2p_bitmap` 走 P2P 补齐；
    3. P2P 通过新增的可选控制帧 `relay_chunk_data` 传输**与 HTTP 中转逐字节一致的密文块**，接收端用同一份 `chunk_hashes[i]` 校验并解密写入同一暂存对象；
    4. 若 P2P 在续传中途意外断开，发送端与接收端可平滑回落继续通过 HTTP 补齐剩余缺失块，已收块全部保留；
    5. 全块集齐后提交一次历史/接收，由接收端向中转发送 `ACK/DELETE` 立即释放云端临时分块；若该 P2P 会话同时协商了 `clip_ack`，接收端经 P2P 异步队列回传 `clip_ack(received|discarded|failed)`，将发送端回执精确升级至 `remote_received` / `remote_discarded`。

### 5. 服务端架构、用户自部署与成本控制

- **官方托管形态**：建议采用 **Cloudflare Workers + Durable Objects（每信箱轻量事件协调/SSE/限流与令牌校验）+ R2（分块密文存储，配置短生命周期兜底清理）**，与现有 `rendezvous.uniclipboard.app` 同架构栈：
  - 边缘就近接入降低移动端首包延迟；
  - R2 免出口流量费，且每个对象默认 10 分钟 TTL、接收端 `ACK` 即刻删除，存储成本极低；
  - 单信箱并发未消费对象数、单次上传大小与单位时间请求速率均设硬上限，防止滥用。
- **用户自部署（Self-Hosting）**：客户端只绑定公开、无供应商锁定的 **Relay HTTP API v1**（HTTPS JSON + 二进制分块 + SSE），与 Tailcat P2P/DERP 中继节点程序完全独立。提供两种官方参考自部署形态：
  1. **单二进制 Go 服务 / Docker 镜像（`unic-relay`）**：内存或本地临时目录暂存短 TTL 分块，零外部数据库依赖，适合 VPS / NAS / 软路由一键运行；
  2. **Cloudflare Workers + Durable Objects + R2 模板**：支持一键部署到用户个人 Cloudflare 账号。
- **自部署配置与公网防白嫖**：
  - 桌面 `config.toml`（及环境变量 `UNIC_RELAY_URL` / `UNIC_RELAY_ACCESS_KEY`，对齐 `UNIC_RENDEZVOUS_URL`）与移动端设置提供 `sync.relay.enabled`、`sync.relay.url`、`sync.relay.max_bytes` 与可选 `sync.relay.access_key`（秘密字段，不进普通日志与事件）；
  - 自部署服务端支持两类可选公网准入控制（不影响零知识 E2EE）：(a) 服务端配置 `allowed_pubkeys` 公钥白名单，仅允许指定设备注册/读取信箱；(b) 服务端配置静态接入密钥（客户端在请求头携带 `X-Unic-Relay-Key`）；
  - 多设备指向同一自部署 `url` 时自动互通；若配对双方配置了不同中转 URL，默认遵循防 SSRF 约束不盲连陌生域名，并在设备连接状态中提示中转端点不一致。

### 6. 进度、回执、接收放弃与可观测性（对齐 0034）

- **复用 0034 进度与回执模型**：
  - 发送侧直接复用 `TargetStatus.Progress`（`TransferProgress`）、`Operation.HistoryID`、`ListOperations` 与 200 ms 节流 `EventOperations`；在既有 `ReceiptStage`（`local_written` / `remote_received` / `remote_discarded`）基础上仅扩展 **`relay_stored`**，并新增正交字段 `TransportRoute`（`p2p` / `relay` / `hybrid`）。
  - 纯中转上传完成时进入 `TargetSent` 且 `ReceiptStage="relay_stored"`（表示“已加密暂存至中转，等待对方设备领取”，绝不伪装为“对方已接收或已写入剪贴板”）；Phase 2 触发 P2P 接力且协商 `clip_ack` 时，接收完成后可经 P2P 升级为 `remote_received` / `remote_discarded`。
- **复用 0034 入站进度与中途放弃（`OperationReceive` / `DiscardReceive`）**：
  - 经 HTTP 中转拉取以及 `TransferAssembly` 跨通道拼装的入站传输，统一登记为 `OperationReceive` 并上报单调 `ReceiveProgress`，直接复用桌面与移动端的接收进度 Toast；
  - 用户中途触发 `DiscardReceive`（点击 `×` 放弃接收）时，立即停写盘、清暂存、释配额，向中转发送 `DELETE` 清理分块；若处于 P2P 接力则同步发送 `clip_ack(discarded)` / 拒绝接力，使发送端及时止损并落入 `TargetDiscarded`（`remote_discarded`）。
- **诊断指标**：复用 [0014](0014-client-observability-otlp.md) / [0020](0020-tailcat-sync-observability.md) 的有限枚举、无隐私脱敏约束：新增传输路径枚举 `relay` / `hybrid_handoff` 与中转建连/上传/拉取/接力结算指标，绝不记录 `relay_url`（自定义域名）、`mailbox_id`、`delivery_token`、`access_key`、`transfer_id` 或正文。

## 备选方案与取舍

| 备选方案 | 评估与结论 |
|---|---|
| **A. 纯本地待发队列（等待 P2P 建连再发）** | 否决。无法实现“0 等待立即开始传输”；移动端分享后用户立即切回原 App 时 iOS 会挂起进程，导致长期发不出去。 |
| **B. 纯用户自备 WebDAV / S3（不提供官方中转）** | 否决作为唯一方案。配置门槛过高，违背移动端开箱即用体验，且纯对象存储缺乏低延迟 SSE 通知，只能靠轮询。本设计通过开放 Relay HTTP API v1 同时兼容自托管。 |
| **C. 仅局域网 HTTP 直连 + P2P** | 否决作为主方案。蜂窝网络或跨网段完全失效，无法覆盖手机在外分享给家里/办公室电脑的核心场景。 |
| **D. Level 1（按单次传输切换，不支持中途续传）** | 仅作为 Phase 1 中间里程碑，不作为最终上限。若协议一开始不按统一分块设计，后续支持大文件 P2P 接力续传必须推翻协议重来；从 Phase 1 即固化统一分块格式，可在几乎不增加前期复杂度的前提下平滑演进到 Phase 2 的 Level 3 续传。 |
| **E. 服务端托管账号与密钥（云同步剪贴板）** | 否决。引入账号系统、隐私合规风险与中心化信任，违背 `unic` 零知识本地优先原则。 |

## 分阶段实施与验收路线

> 实施进度与验收回执后续统一记录在 `docs/exec/0035-hybrid-e2ee-relay-and-p2p-handoff.md`，不写入本决策正文。

1. **Phase 0（契约冻结与密码学向量）**：评审冻结 [混合中转同步契约](../../contracts/hybrid-relay-contract.md)，生成确定性跨语言测试向量（Ed25519→X25519、CEK 封装、分块 AEAD、信封签名、请求鉴权签名），并交付用于本机集成测试的 Go 内存/进程内 Relay HTTP v1 参考实现。
2. **Phase 1（0 等待基础闭环：P2P 优先 + E2EE HTTP 中转）**：
   - 落地 `internal/relaycrypto`、`internal/relayclient` 与 Engine 混合调度及单/多分块中转收发，接入 `TargetStatus.Progress`/`ReceiptStage="relay_stored"` 与 `OperationReceive`/`DiscardReceive`；
   - `builtin.tailcat` 通过 `Hello.Caps` 协商 `relay_profile` 并在会话建立时自动交换可撤销 `delivery_token`；
   - 桌面 `unicd` 与移动端 `bindings/mobile` + `app/expo` 接入中转端点配置与即时发送状态展示；
   - 验收：多进程 E2E 验证 P2P 在线零中转请求、P2P 断开时 0 等待 HTTP 完成收发、中途 `DiscardReceive` 清理中转与暂存、篡改/重放/跨目标/已撤销设备拒绝、全局关闭与撤销屏障、中转关闭零开销。
3. **Phase 2（Level 3：大载荷中途 P2P 结算与接力续传）**：
   - 在 `wire v1` 通过 `Hello.Caps` 协商 `relay_handoff`，扩展可选分块接力控制帧（`relay_handoff_offer` / `relay_handoff_ack` / `relay_chunk_data`）与 Engine 共享 `TransferAssembly` 暂存拼装器；
   - 实现 HTTP 分块上传中途 P2P 恢复自动位图结算、剩余块 P2P 续传、P2P 中途断线反向回落 HTTP、接收完成后即时 `ACK/DELETE` 清理中转分块，以及经 `clip_ack` 升级 `remote_received` / `remote_discarded`；
   - 验收：故障注入与限速 E2E 验证已传分块不重传、双通道拼装后单次入库且字节/哈希完全一致、中途断网/取消/`DiscardReceive` 无泄漏、移动端真机分享大文件实测。
4. **Phase 3（移动端后台韧性、托管上线与可选扩展直传评估）**：
   - 部署官方托管 Relay 服务（Workers + Durable Objects + R2）并完成自托管部署指南；
   - 补强 iOS `beginBackgroundTask` / 后台上传会话与桌面/移动端自托管 URL 设置界面；
   - 评估 iOS Share Extension 通过只读快照免跳转后台直传的必要性与安全边界（若推进另立子计划）。

## 开放待确认项（实施前冻结）

以下参数在契约草案中给出推荐默认值，待评审确认后即锁定：

1. **官方中转默认单次载荷上限**：推荐 **25 MiB**（覆盖全部文本、高清图与短视频；自托管可配至 `limits.max_total_bytes`）。
2. **默认中转对象 TTL**：推荐 **10 分钟**（接收端确认领取后即刻删除；未领取最长保留 10 分钟，自托管上限 30 分钟）。
3. **单块快路径阈值与分块大小**：推荐单块阈值 **2 MiB**、多分块大小 **1 MiB**、P2P 接力切换门槛 **剩余 ≥ 2 MiB**。
4. **官方中转域名规划**：独立子域 `https://relay.uniclipboard.app` 还是复用 `https://rendezvous.uniclipboard.app/v1/relay/*`（契约按 Base URL 抽象，两者无协议差异）。
