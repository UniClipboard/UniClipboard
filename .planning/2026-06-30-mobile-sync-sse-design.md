# mobile-sync SSE 推送设计（前台事件驱动取代轮询）

状态：**grilling 逐分支拍板 + codex 10 轮对抗评审全部内嵌（2026-06-30）**——10 轮共 29 条评审意见独立核查后全部采纳、0 驳回（F-6 配置归属升级用户拍板），见各节「评审修正」标注 · 末轮仅剩 2 条 MINOR 一致性项 (已修),评审已收敛至微观一致性 · **P0(服务端)+ P1(uc-mobile 客户端) 已实现 (2026-07-03，分支 `sse`,3 个 atomic commit)**,详情见 §8 分阶段实现计划的行内标注;P2/P3(Android/TS) 交接给 `uniclipboard-android` 仓库
关联：
- 服务端 mobile-sync HTTP 在 `crates/uc-webserver/src/mobile_lan/`（axum 0.7，固定端口 42720，Basic Auth）。
- 事件源出口已核实：`AdvanceActiveClipboardPort::advance(state) -> Ok(bool)`（issue #1017 ActiveClipboardState LWW 寄存器），adapter 唯一实现在 `crates/uc-infra/src/db/repositories/active_clipboard_register_repo.rs:44`。
- 身份契约已核实：`ActiveClipboardState.snapshot_hash` 即 `"blake3v1:<hex>"` 内容身份（`crates/uc-core/src/clipboard/active_state.rs:19-22`），等于 `GET /SyncClipboard.json` 的 `contentId` 字段，**不是** `hash` 字段（后者随服务字节变，见 `crates/uc-webserver/src/mobile_lan/routes/sync_doc.rs:56-72`）。
- contentId 去重契约：`.planning/2026-06-23-contentid-mobile-dedup-design.md`。
- 客户端：`crates/uc-mobile-proto/src/sync_engine.rs` reducer + `crates/uc-mobile/src/client.rs`（reqwest，经 UniFFI），TS 侧 `uniclipboard-android`（`src/services/SyncEngine.ts`、`src/services/ClipboardMonitor.ts`）。
- 上游协议背景：mobile-sync 对外是 SyncClipboard 兼容协议（`GET/PUT /SyncClipboard.json`、`/file/{name}`），SSE 是 `/api` 命名空间下的私有扩展。

> **一句话**：SSE 是「门铃」，`GET /SyncClipboard.json` 才是「门」。SSE 只送信号，不送内容；内容与去重决策仍归现有 reducer。

---

## 0. 决策记录

| # | 决策点 | 结论 | 依据 |
|---|---|---|---|
| 1 | 事件接入点 | 装饰 `AdvanceActiveClipboardPort`，内层 `advance()→Ok(true)` 时 publish | 三个生产调用点全汇入此 port；`true` ⟺ register 真前进 ⟺ 本机 OS 剪贴板此刻就是这条；LWW-loser 返回 `false` 天然滤掉 |
| 2 | 事件负载 | `update` 携带 `{content_id, server_time_ms}`；`content_id` 供短路（比 `last_synced_content_id`），收到即 `getLatest` | **评审修正 F-1**：`snapshot_hash` 是 `blake3v1` contentId，不是手机的 `last_synced_hash`；用 contentId 短路且跨重编码稳定。**评审修正 F-9**：删 `seq`（非严格单调，无重放语义） |
| 3 | 落点分层 | 装饰器 + `broadcast::Sender<ActiveClipboardState>` 在 uc-infra；wire 映射在 uc-webserver；`Sender` 在 `wiring/wire.rs` 构造 clone | 装饰 infra repo adapter + 用 tokio broadcast → infra；app 层 §15.3 禁 channel 上浮 |
| 4 | 客户端 FFI | UniFFI callback interface：`SseListener`(`on_hello`/`on_update`/`on_disconnected`) + `SseHandle`(cancel) | SSE 解析放 Rust 层，绕开 RN/WKWebView 对 EventSource/header/stream 的限制 |
| 5 | 重连责任 | Rust 管单次会话（读流 + 心跳超时检测，断即 `on_disconnected`，不自动重连）；TS 管退避重连 + feature-detect 回退轮询 + 生命周期 + 并发协调 | 状态/策略集中 TS（SyncEngine 是状态中枢），Rust 保持薄、跨平台一致 |

**参数默认**（无结构分叉，实现期可微调）：
- 心跳 25s comment ping；客户端 >2× 心跳无字节判死。
- **SSE 用独立 reqwest 客户端**（无 read idle timeout 或显著大于心跳），不复用现有 10s idle 客户端（**评审修正 F-2**）。
- broadcast 容量 64；`Lagged` 时给该连接补发一个无内容 `resync` 帧促客户端立即 `getLatest`（**评审修正 F-3**），不静等兜底。
- **每设备连接数上限进 P0**（按稳定 `device_id`、每设备 1–2 条流，新连接挤掉最旧；评审修正 round 4 F-2 + round 8/9 F-1），不再是可选——避免单凭据放大周期性 password 校验。
- **无桌面端开关**：服务端无条件注册端点（向后兼容无害）；是否启用 SSE 由手机端 feature-detect + RN 本地设置决定（**评审修正 F-6**）。
- 前台兜底 tick 30s；`background` 拆 SSE，后台逻辑不动（本期范围）。

---

## 1. 要解决的问题

手机端当前用定时 tick 感知桌面新内容，但 **上下行已解耦**（已核实）：

- **上行**（手机复制 → 推桌面）已是事件驱动：`ClipboardMonitor` 1s 轮询 **本地** 剪贴板，检测到本地写 → `SyncEngine.notifyDeviceChanged` → `forceTickNow()`（`SyncEngine.ts:276`）。不靠 1Hz 周期 tick。
- **下行**（桌面复制 → 拉到手机）才是 `scheduleNextTick` 那个 1Hz 周期 tick 的核心职责：定期 `getLatest` 探服务器。

代价集中在下行：**前台每秒一次 `getLatest`**，每次唤醒无线射频（含 tail energy），空转占大头；下行延迟最高一个周期（≤1s）。

目标：**用桌面主动通知替代下行的周期轮询**，下行延迟近即时 + 消掉空转唤醒。上行与本地剪贴板轮询不在改造范围（OS 无剪贴板变化推送 API，本地检测必须轮询）。

---

## 2. 不变量与边界

- **notify-then-pull 是红线（决策 2）。** SSE `update` 只携带 `{content_id, server_time_ms}`。手机收到后 **仍走现有 `getLatest` → `plan_after_server_get`** 拉权威 snapshot 并决策。SSE 通道 **不得** 重新实现 LWW / 去重 / ServerNew·Converged·Push 路由——那套唯一活在 `sync_engine.rs` reducer 里。
  - **评审修正 F-1**：`content_id` 取自 active register 的 `snapshot_hash`（本就是 `"blake3v1:<hex>"`）。手机用它 **与 `last_synced_content_id` 比**（不是 `last_synced_hash`！后者是随服务字节变的另一套哈希，二者永不相等）。一致则连 `getLatest` 都省；不作为内容真值，也不写 reducer 的 `last_synced_*`。
  - 用 contentId 短路天然吃下「桌面 resurface 重戳 ts → `advance→true` → 发 SSE，但 content_id 不变」这种场景：手机短路忽略，不重复拉。
- **SSE 不是内容传输通道。** 文本/图片/文件本体仍走 `GET /SyncClipboard.json` 与 `GET /file/{name}`。SSE 帧恒为几十字节量级。
- **无 `Last-Event-ID` 重放，无 `seq`（评审修正 F-9）。** register 是单行 LWW 寄存器，无事件历史，服务端无从、也无需重放。客户端重连后 **无条件 `getLatest` 一次**。`seq=activated_at_ms` 非严格单调（跨设备时钟 / 同毫秒 / LWW tie-break），不能表达顺序，删除；单连接内事件本就有序。
- **后台不在本期范围（已拍板）。** `background` 断 SSE，后台同步逻辑（5s tick + sync-on-resume）原样保留。OS 回收后台长连接是平台限制；后台实时唯一省电路径是系统推送（APNs/FCM，需出网），后续独立议题。
- **向后兼容。** SSE 是纯新增端点：旧手机不调用 → 桌面零影响；旧桌面无端点 → 手机 feature-detect 失败回退轮询；第三方 SyncClipboard 客户端不受影响。
- **鉴权复用 Basic Auth，但连接需持续有效（评审修正 F-8 + round 2 强化）。** 建连时过 middleware Basic Auth；长连接建立后凭据可能失效（注销设备 / 改密码 / 改用户名），故 handler 须在连接级 **周期性重跑 Basic Auth 校验**（**mobile-sync 主 / LAN 开关的关闭不在此列——只由 lifecycle cancel 信号处理，评审修正 round 10 F-1**）——仅查「设备是否存在」不够，改密码后设备记录仍在、"存在"仍为真，必须重跑校验才能识别凭据轮换（见 §4.4）。
- **不引入 SignalR。** `uniclipboard-android` 的 `@microsoft/signalr` 从未使用；落地后清理。

---

## 3. 事件源（已核实：单一收敛出口存在）

register 前进的生产调用点只有 3 处，全部汇入 `AdvanceActiveClipboardPort::advance`：

1. `crates/uc-application/src/clipboard_write/active_register.rs:67` — `LocalActiveRegisterAdvancer.advance_local`（本机 capture / restore）。
2. `crates/uc-application/src/usecases/clipboard_sync/active_state/apply_inbound.rs:529` — 入站 0xC3 state apply。
3. `crates/uc-application/src/usecases/clipboard_sync/apply_inbound/usecase.rs:203` — 入站 0xC1 内容 apply。

`advance() -> Ok(true)` 即「register 真前进 ⟺ 本机 OS 剪贴板此刻就是这条」，LWW-loser 返回 `Ok(false)`。无需任何收敛重构。

> `advance_local` 注释已预埋扩展点：*"Returns the `ActiveClipboardState` ... so the caller can hand it on (e.g. to a broadcaster) ..."*

**时序自洽（已核实）**：入站 `advance` 在 detached spawn 的写成功分支（#1017 D1/cp-1）；本机 `advance_local` 在 `ClipboardWriteCoordinator::write` 成功后；`GET /SyncClipboard.json` 读源已切 active register（commit `f01f77cce`）。故「`advance→Ok(true)` → publish → 手机 `getLatest` 读 register」无竞态。

---

## 4. 服务端设计

### 4.1 装饰器（uc-infra，决策 1 + 3）

```rust
// uc-infra：装饰真实 register repo，advance 转发；内层 Ok(true) 时广播。
struct BroadcastingAdvance {
    inner: Arc<dyn AdvanceActiveClipboardPort>,        // 真实 repo adapter
    tx: broadcast::Sender<ActiveClipboardState>,       // tokio，容量 64
}

#[async_trait]
impl AdvanceActiveClipboardPort for BroadcastingAdvance {
    async fn advance(&self, state: &ActiveClipboardState)
        -> Result<bool, ActiveClipboardRegisterError>
    {
        let advanced = self.inner.advance(state).await?;
        if advanced {
            let _ = self.tx.send(state.clone());           // fire-and-forget
        }
        Ok(advanced)
    }
}
```

`broadcast::Sender<ActiveClipboardState>` 在 `wiring/wire.rs` 构造一次，clone 两份：① 注入 `BroadcastingAdvance`；② 交给 mobile_lan server 的 `MobileLanState`。负载用 core 的 `ActiveClipboardState`（其 `snapshot_hash` 即 content_id），wire 映射在 webserver 做。

### 4.2 新端点（uc-webserver）

```
GET /api/sse/clipboard
Authorization: Basic <base64(user:pass)>
Accept: text/event-stream
```
响应头：`200`，`Content-Type: text/event-stream`，`Cache-Control: no-cache`，`X-Accel-Buffering: no`。路由注册在 `routes.rs`，落在 Basic Auth middleware 之后。

### 4.3 SSE handler

建连流程（**评审修正 F-4：顺序保证**）：
1. middleware 过 Basic Auth。
2. **先** `sender.subscribe()` 拿 `Receiver`（确保 subscribe 之后的任何更新都进 buffer，作为 `update` 送达）。
3. **再** 发 `hello`。
4. 合并 `Receiver` 流 + 心跳 `interval`(25s) + 连接级重校验 `interval`，交 `axum::response::Sse` 返回。

事件格式：

`hello`（= 「连上了」信号，不承诺重放）：
```
event: hello
data: {"server_time_ms":1750000000000}
```
> 客户端收到即无条件 `getLatest` 一次。配合步骤 2 先于 3 的顺序，**建连窗口的任何更新要么在 receiver buffer（→ update 帧），要么被这次无条件 pull 读到**，不依赖 30s 兜底。

`update`：
```
event: update
data: {"content_id":"blake3v1:<hex>","server_time_ms":...}
```

`resync`（**评审修正 F-3**，`Lagged` 时补发）：
```
event: resync
data: {"server_time_ms":...}
```
> 无 content_id，语义 = 「你可能漏了，立即 `getLatest`」。

心跳（25s）：`: ping`（comment，仅保活 + 死连接检测）。

`SseUpdateWire`（webserver 内部，从 `ActiveClipboardState.snapshot_hash` 映射）：
```rust
struct SseUpdateWire { content_id: String, server_time_ms: i64 }
```

### 4.4 lagged / 连接有效性 / 资源

- **Lagged（评审修正 F-3）**：接收端 `RecvError::Lagged(n)` 时记 warn（含 n），**不断开**，并向该连接 **立即发 `resync` 帧** 促客户端 `getLatest`，不静等下一个真实事件或 30s 兜底。
- **连接级重校验只管凭据（评审修正 F-8 + round 2 强化 + round 8 F-3 收窄）**：handler 缓存建连时的 `Authorization` header，每 N 个心跳周期（如每 ~30s）**重跑 `AuthenticateBasicAuthUseCase(cached_header)`**——一次性覆盖「设备被删（username 查不到）」与「密码/用户名轮换（password hash 不匹配）」，**不只是查设备存在**。失败 → 结束流。
- **mobile-sync 开关由 lifecycle cancel 信号管，不进重校验（评审修正 round 8 F-3）**：mobile-sync 受两个开关控制（主 `enabled` + LAN 子开关 `lan_listen_enabled`）。关任一 → listener 停 / 重建 → cancel token 触发 → SSE 流 `select!` 退出（见下「优雅关闭」）。故 **不在重校验里查开关**——职责切清：cancel 信号管「listener 该不该活」，重校验管「这个连接的凭据还有效吗」。
- **SSE 连接注册表（评审修正 round 5 F-1 + round 8 F-1）**：为支持「连接数上限 + 挤掉最旧」，server state 持一个 SSE 连接注册表，每条流登记一个 cancel handle；新连接到来时若已达上限，主动关闭最旧的流；流结束时注销。**注册表必须按认证设备的稳定 `device_id` 索引（Basic Auth 认证后从 `AuthenticatedDevice` 拿），不是 username**——username 可编辑，改名后新旧流会落入不同 bucket 使 cap 失效；username 仅用于日志。（这取代早先「无连接表」的设想——连接数上限要求共享状态。）每连接仍是一个 axum task + 一个 `Receiver`。
- **每设备连接数限制（评审修正 round 4 F-2，纳入 P0、非可选）**：每个 `device_id` 最多 1–2 条 SSE 流，新连接到来时关闭最旧的流。配合 §4.4 周期性 password hash 校验（argon2 较重），unlimited 连接会让单个有效凭据开 N 条流、放大 N× 重校验开销，故连接数上限是 P0 必须。
- **优雅关闭 / 即时生效（评审修正 round 3 F-1）**：SSE 是「永不结束的请求」（持续心跳），若不主动响应取消，axum `with_graceful_shutdown` 会 **无限等待** 它，导致「禁用 mobile sync / 改端口重建 listener」（mobile-sync 配置即时生效、无需重启）在有手机连着时 hang。故 SSE 事件流必须把取消信号作为一个 **主动终止分支**——在 stream 实现里 `select!` 合并 ① broadcast receiver、② 心跳 interval、③ 连接级重校验、④ `cancel.cancelled()`，cancel 触发立即结束流。server cancel、listener 重建、mobile-sync 关闭都应汇入这同一个取消信号。

---

## 5. 客户端设计

### 5.1 Rust `uc-mobile`（决策 4 + 5：管单次会话）

```rust
// uniffi callback interface
trait SseListener: Send + Sync {
    fn on_hello(&self, server_time_ms: i64);
    fn on_update(&self, content_id: String);   // 评审修正 F-1：content_id，非 hash
    fn on_resync(&self);                        // 评审修正 F-3
    fn on_disconnected(&self, reason: String);
}

// SSE 作为现有 client 的方法（评审修正 round 4 F-1）：复用其 runtime、
// ServerConfig 解析与 Basic Auth 凭据，不另起 owner / runtime / 第二条长生命周期路径。
impl MobileSyncClient {
    // server 随请求传入，匹配现有 getLatest / putClipboard 等「每请求传 server」
    // 的风格（client 不持有当前 server）；评审修正 round 5 F-2。
    fn start_sse_subscription(&self, server: ServerConfig, listener: Box<dyn SseListener>) -> SseHandle;
}
// SseHandle::cancel() — 前台→后台 / 切服务器 / 登出时调用
```

- **SSE 专用 reqwest 实例（评审修正 F-2 + round 4 F-1 调和）**：复用 `MobileSyncClient` 的 runtime 与 server/auth 配置（不另起 owner），但 SSE 用 client 内部一个 **单独配置的 reqwest 实例**——无 read idle timeout（或远大于心跳），**不复用** `client.rs:75` 那个 `REQUEST_IDLE_TIMEOUT=10s` 的一次性请求实例（否则首个 25s 心跳前就 idle 超时 → 持续重连）。该 SSE 实例 **必须继承现有 client 的 TLS / trust-insecure-cert 设置（评审修正 round 5 F-3）**，否则正常同步能连自签证书端点而 SSE 静默失败/回退。⚠️ 实现注意（评审修正 round 8 F-2）：现有 client 只存 **已构建** 的 HTTP client，trust 设置不可单独读取。需把 trust 设置 **存为可单独读取的 config 值**（在 `set_trust_insecure_cert` 同步更新），SSE client 从该值重建；P1 测试须在 **toggle 设置之后** 验证（不只构造时）。
- 在长期 tokio 任务里逐行解析 SSE 帧（`event:` / `data:` / 注释），经 callback 回 TS。
- **心跳超时检测在 Rust**：>2× 心跳无字节 → `on_disconnected{reason}`。
- **不自动重连**（策略归 TS）。`SseHandle` 持 cancel + join。
- ⚠️ 现有 client 用 `current_thread` runtime；SSE 长任务生命周期需独立管理。真机（尤其 iOS）需验证 callback 跨线程 marshalling + 长任务线程归属。

### 5.2 TS `SyncEngine.ts`（决策 5：管策略 + 并发协调）

**并发协调状态机（评审修正 F-7）**——SSE 事件、兜底 tick、本地剪贴板变化、网络/服务器切换可能并发，需明确规则：
- 每个 SSE 订阅绑定一个 **服务器 epoch**，且 epoch 绑定到一份 **确切的 server config**（base URL + 凭据，评审修正 round 5 F-2）；`on_*` 回调只对 **当前 epoch** 生效，旧 epoch 回调一律丢弃。
- 切服务器 / 退后台 → `SseHandle::cancel()` 旧 handle，递增 epoch，丢弃在途旧回调。
- `on_update` / `on_resync` / `on_hello` / 本地写 / 兜底 tick 多源并发 → 合并成 **单次 pending pull**（一个在跑 + 至多一个 pending），不叠加并发 `getLatest`（复用 `forceTickNow` 已有的「等待正在执行的 tick」语义）。

事件处理：
- `on_update(content_id)` → `content_id == last_synced_content_id` 则忽略（评审修正 F-1 短路）；否则触发一次下行 tick（pending-pull 合并）。
- `on_resync` → 无条件触发一次下行 tick。
- `on_hello` → 无条件 `getLatest` 一次。
- `on_disconnected` → 指数退避（1s→…上限 30s）重连；连续失败超阈值 → feature-detect 回退现有轮询，周期性重试 SSE。
- **保留 ~30s 低频兜底 tick**（SSE 在线也跑）。
- **上行不动**：`ClipboardMonitor` 1s 本地轮询 + `notifyDeviceChanged` → `forceTickNow` 原样保留。
- **开关（评审修正 F-6）**：是否尝试 SSE 由 RN 本地设置 + feature-detect 决定，不经桌面 `settings.mobile_sync`。

### 5.3 生命周期（`syncEngineStore.ts:215` 一带）

| AppState | 行为 |
|---|---|
| `active` | 建立/恢复 SSE 订阅（新 epoch）+ 立即 `getLatest` 一次（sync-on-resume） |
| `inactive` | 维持（短暂态） |
| `background` | **断开 SSE**（cancel + 递增 epoch），回退后台现状逻辑——本期不改后台 |

- **网络路由变化（评审修正 round 6 F-3）**：WiFi ↔ 蜂窝等路由切换 **等价于 server epoch 变化**——cancel 当前 `SseHandle`、bump epoch、按需清重连 backoff、对当前选定 server 重连。**不要干等心跳超时** 才恢复（否则延迟 + stale 回调）。复用现有 NetInfo 监听触发。

---

## 6. 省电分析（诚实版）

| 通道 | 现状 | 本设计后 |
|---|---|---|
| 下行（前台） | 每秒 `getLatest`，每次唤醒射频 | 空闲长连接，仅 25s 一次心跳；有内容才有流量 |
| 下行延迟（前台） | ≤ 1s | 近即时 |
| 上行（前台） | `ClipboardMonitor` 1s 本地轮询 | **不变**（OS 无剪贴板推送） |
| 后台 | 5s 轮询 | **不变**（SSE 不连） |

结论：**省电收益 = 消掉前台「每秒拉服务器」这半个轮询**（下行）+ 下行延迟即时。上行本地轮询、后台逻辑都不改善——不夸大收益。

---

## 7. 兼容与灰度

- 桌面：纯新增端点，**无条件注册**（评审修正 F-6），旧手机不调用 → 零影响；第三方 SyncClipboard 客户端不受影响；**不改桌面 settings 模型**（省去 DTO/OpenAPI/UI/迁移）。
- 手机：feature-detect 自动回退轮询；是否启用 SSE 由 RN 本地设置控制，可平滑灰度。
- 清理项：移除 `uniclipboard-android` 未使用的 `@microsoft/signalr` 依赖。

---

## 8. 分阶段实现计划

> **实现状态 (2026-07-03，分支 `sse`)**:P0 + P1 均已完成并有测试覆盖，详见下方各条内联标注。两处与本节原设想的偏差:①装配链实际只有 `apps/daemon/src/daemon/mobile_lan_lifecycle.rs` + `host.rs` 两处 (`app_assembly.rs` / `app_facade_assembly.rs` 是过时引用，实际不存在);② P1 曾以为 `client.rs:497` 已有可读的 `trust_insecure_cert` 存值字段，核实后那其实是函数参数名——已补加 `AtomicBool` 字段修正。P1 的自签证书 TLS 集成测试与真机 callback 线程模型验证未完成，留待交接 (见下方标注)。
>
> **code review 后修订 (2026-07-03)**，以下四点实现与正文原始设计不同，以实现为准：
>
> 1. **事件线格式收编进 `uc-mobile-proto::sse_event`**:事件名、`SseHello`/`SseUpdate`/`SseResync` payload、帧解析（`find_frame_end`/`parse_sse_frame`，兼容 LF 与 CRLF）、心跳周期常量 `SSE_HEARTBEAT_INTERVAL_SECS` 均为 daemon 序列化端与 uc-mobile 解析端的单一真相源。§4.3 的「`SseUpdateWire`（webserver 内部）」已不存在。
> 2. **连接级重校验不再重跑 `AuthenticateBasicAuthUseCase`**（§4.4 F-8 原案）:改为 `MobileSyncFacade::is_device_credential_current`——建连时缓存设备的 Argon2 PHC 串，每 30s 只做「设备仍存在 && 存储 PHC 与建连时逐字节一致」的 repo 读 + 字符串比对，零 Argon2 开销。覆盖面不变（注销 + 凭据轮换），§9 风险 6 的「argon2 较重但 30s 一次可接受」成本论据随之作废（连接数上限仍保留，理由退为一般资源约束）。
> 3. **客户端帧缓冲加 64KB 硬上限**（`MAX_SSE_BUFFER_BYTES`）:无帧终止符时字节持续到达不会触发心跳超时，无上限即无界内存增长（jetsam 红线）。超限 → `on_disconnected("sse buffer overflow…")`。
> 4. **连接注册表用 `std::sync::Mutex`**（临界区不跨 `await`），`unregister` 为同步方法，RAII guard 的 `Drop` 直接调用，不再 `tokio::spawn` 清理任务。

- **P0 服务端端点（本仓库可独立验证）** ✅ 已完成
  - uc-infra：`BroadcastingAdvance` + `broadcast::Sender<ActiveClipboardState>`。
  - **装配链（评审修正 F-5 + round 6 F-1/F-2，按真实代码路径列全）**：真实 listener 不是从 `wire.rs` 直连 `start_mobile_lan_server`，而是经 daemon lifecycle：
    1. `uc-bootstrap`（`wiring/wire.rs`）构造 `broadcast::Sender`，注入 `BroadcastingAdvance`。
    2. daemon 装配（`apps/daemon/src/daemon/app_assembly.rs` / `app_facade_assembly.rs`）把 `Sender`**与 listener cancel token** 带入 `MobileLanLifecycleController`（`apps/daemon/src/daemon/mobile_lan_lifecycle.rs`）/ `AppFacadeListenerSpawner`——**这层正是 mobile-sync no-restart 即时生效 / rebind 的归属**，漏掉它会按错误的简化路径实现。
    3. lifecycle controller 调 `start_mobile_lan_server(bind, cancel, facade, file_transfer, sse_source)`（签名加广播源参数）。
    4. `start_mobile_lan_server` → `build_router` → `MobileLanState` 持有 `Sender` **和 listener cancel token**（或其 child token）。当前 `cancel` 只到 `with_graceful_shutdown`（`server.rs:70`），route handler 看不到，**必须显式下放到 `MobileLanState`**，否则 SSE 流无法 `select!` cancel（评审修正 round 6 F-1）。
    5. SSE handler 从 `MobileLanState` 取 cancel token：流 `select!` 在它上面（§4.4）、连接注册表驱逐也用它派生的 per-stream child token。
    6. 测试 helper（`mobile_lan/test_support.rs`）提供 fake/真实 source + 可手动触发的 cancel token。
  - uc-webserver：`GET /api/sse/clipboard` handler（subscribe→hello→流；update/resync/心跳；连接级重校验；**每 `device_id` 连接数上限 1–2、新连接挤掉最旧，F-2**；流 `select!` 主动响应 cancel）。
  - **验收（评审修正 F-10，补边界）**：
    - `curl -N -u user:pass http://<host>:42720/api/sse/clipboard` 见 `hello` + 周期 `: ping`。
    - 桌面各来源（OS / P2P / 本地编辑 / mobile-apply）改剪贴板 → curl 即时收到 `update{content_id}`；LWW-loser **不** 触发。
    - **mobile PUT（新上传或 duplicate resurface）**（评审修正 round 10 F-2）：可能因 capture-commit 与 activation-announce 两条 advance 路径产生 **≥1 个 `update` 帧**；允许多帧，但 **都必须携带同一 `content_id`**，客户端靠 pending-pull coalesce + content_id 短路吸收，**无额外可见同步工作**（不重复建卡 / 不重复拉取）。
    - **闸门行为**：只有真正的 disabled / locked 闸门阻止 register advance → 从而不 publish。
    - **headless 模式 ≠ 禁用（评审修正 round 3 F-2）**：headless 模式在 mobile sync 启用时与正常 daemon 模式一致地提供 SSE（共用 mobile LAN gateway），不可与 disabled/locked 混为一谈、不应被当成「不触发」。
    - `advance` 存储失败时不 publish（内层返回非 `Ok(true)`）。
    - **开着 SSE 时轮换凭据**（改密码 / 改用户名而设备记录仍在，或注销设备）→ 下一个重校验周期内流结束（评审修正 round 2）。
    - **关闭 / 重建 listener 不 hang（评审修正 round 3 F-1）**：`curl -N` 开着，cancel 或禁用 listener（或改端口触发重建）→ 流在有界时间内结束、server task 退出，不被 SSE 长连接阻塞。
    - **关 mobile-sync 主开关 或 LAN 子开关（评审修正 round 8 F-3）**：开着 SSE，关 `enabled` 或 `lan_listen_enabled` → listener 停/重建 → 流经 cancel 信号结束（**两个开关都要测**，不只主开关）。
    - **同设备多流 + 改名驱逐**（评审修正 round 5 F-1 + round 8 F-1）：注册表按稳定 `device_id` 索引；同一 device 开第 3 条流时最旧被关、活跃数不超上限；改 username/password 后用新凭据重连，同 device 旧流 **立即** 被驱逐（不必等重校验周期）。
    - 重连（断开后重新 GET）→ 收到 `hello` → 客户端语义上应立即拉到当前内容。
    - （注：自签证书 trust 测试属客户端侧、移至 P1——P0 桌面 LAN 是 HTTP-only，见评审修正 round 7。）
- **P1 客户端 Rust FFI（uc-mobile）** ✅ 已完成 (`crates/uc-mobile/src/client.rs`),遗留 2 项交接
  - `start_sse_subscription` + `SseListener`(`on_hello`/`on_update`/`on_resync`/`on_disconnected`) + `SseHandle`；SSE 帧解析；心跳超时 → `on_disconnected`。
  - **独立 reqwest 客户端无 read idle timeout（评审修正 F-2）**——写进验收：连接能稳定撑过多个心跳周期不被 idle 超时打断。 ✅ `build_sse_http_client` 只设 connect timeout，不设 read timeout;`heartbeat_timeout` 做成显式参数 (测试传短值验证超时路径，不必真等 50s)。
  - **自签证书 SSE 测试（评审修正 round 7，从 P0 移入；round 8 F-2 强化）**：mock HTTPS SSE server，验证 SSE reqwest 实例从 **可读 trust config 值** 重建、继承 trust-insecure-cert 设置、能连上不静默失败。须在 **toggle `set_trust_insecure_cert` 之后** 测，确认重建用更新后的值（不只构造时）。P0 桌面 LAN 是 HTTP-only，该测试属客户端侧。 ⚠️ **未完成**——已修 `trust_insecure_cert` 可读性 (新增 `AtomicBool` 字段随 `set_trust_insecure_cert` 同步) 并有单测 (`trust_insecure_cert_flag_tracks_the_toggle`) 钉死该值本身的同步，但没有起真正的自签证书 TLS mock server 做端到端握手验证——需要 `rcgen` 之类依赖搭 TLS listener，评估后判断超出本轮剩余精力，交接给下一轮。
  - 真机验证 callback 线程模型（尤其 iOS）。 ⚠️ **未完成**——本仓库无法自动化，交接给移动端团队 (§9 风险 4)。
- **P2 客户端 TS 接入（uniclipboard-android）**
  - epoch 绑定的并发协调状态机（F-7）；`on_update` content_id 短路（F-1）；`on_resync`/`on_hello` 无条件拉；退避重连 + feature-detect 回退；30s 兜底 tick；生命周期 active/background 建/拆。上行原样不动。RN 本地 SSE 开关（F-6）。
- **P3 清理**：移除 `@microsoft/signalr`。

P0（服务端）+ P1（`uc-mobile` 客户端，`crates/uc-mobile`）都在 **本仓库** 闭环；P2–P3（Android/TS 集成）在 `uniclipboard-android` 仓库（评审修正 round 7）。

---

## 9. 风险与待评审点

1. ~~事件源是否有单一出口~~ — 已消解（§3）。
2. ~~事件 hash 身份错配~~ — 已修（F-1，§2，改 content_id）。
3. ~~客户端 idle 超时 vs 心跳~~ — 已修（F-2，独立客户端）。
4. **UniFFI callback 的线程模型** — `current_thread` runtime 下长任务 + foreign callback 的真机行为（尤其 iOS）需实测。**当前最大未知。**
5. **心跳间隔 25s** — LAN 直连偏保守，需真实 NAT/AP 环境验证不被中间设备断连。
6. **连接级重校验频率（F-8 + round 2 + round 9 F-2）** — ~30s 一次重跑 Basic Auth 校验 **（仅凭据；不查 mobile-sync 开关——开关由 cancel 信号管，见 risk 7）**，需确认成本可忽略（password hash 校验比纯存在性查询略重，但 30s 一次可接受）。
7. **SSE 流必须主动响应取消（round 3 F-1）** — 实现期务必确认 stream 的 `select!` 真的把 `cancel.cancelled()` 作为终止分支，且 listener 重建 / mobile-sync 关闭都接入同一取消信号；否则 graceful shutdown / 即时生效会 hang。需 P0 测试守住。

---

## 10. 明确不做（本期范围外）

- 后台 SSE / 后台实时（OS 限制，需系统推送另案）。
- 上行通道改造（本地剪贴板轮询是 OS 限制，保留）。
- SSE 携带内容本体 / 在 SSE 通道做去重决策（违反 §2 红线）。
- `Last-Event-ID` 事件重放 / `seq` 字段（单行 register 无历史，改无条件 pull；F-9）。
- 桌面 `settings.mobile_sync` SSE 开关（F-6，改手机端本地控制）。
- 用 SSE 取代兜底轮询（保留兜底是健壮性要求）。
- 启用 SignalR。
