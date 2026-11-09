---
paths:
  - engine/peerauth.go
  - engine/pairing.go
  - engine/devices.go
  - engine/candidates.go
  - engine/tailcat_host.go
  - engine/peerproc.go
  - internal/identity
  - internal/rendezvous
  - internal/store/devices.go
  - internal/store/candidates.go
  - app/desktop/internal/host/node.go
  - app/desktop/internal/api/api.go
  - app/desktop/cmd/unic/cli.go
  - app/desktop/cmd/unic/gui.go
  - internal/session/engine.go
  - internal/plugins/tailcat
  - plugin/rpc/identity.go
verified: a19152214865e4393e781b627244e39b3e6c1c2a
---
# 设备身份、信任与配对

Engine 统一管理“对端是谁、是否被信任、经哪条后端路径可以收发”：设备身份、配对（6 位码 / 完整票据）、按后端实例的设备关联（binding）和设备介绍。

## 现状

- **身份**：每台设备一把 Ed25519 密钥，首次启动由 Engine 生成，存 `<DataDir>/identity/device.key`（0600，目录 0700）；公钥登记在库的 `meta.identity.public`。DeviceID 由公钥导出（26 字符 base32）。密钥缺失、损坏或与登记不符时启动失败，不会重新生成。与任何传输密钥（如 tailcat 节点密钥）无关。
- **握手**：插件后端只搬运字节，Hello/转录/签名/验证都在 Engine（`engine/peerauth.go`：`IdentityHello`→`IdentityProve`→`CheckPeer`）。证明绑定后端提供的连接上下文与双方新鲜 nonce；nonce 记忆防重放，握手 60 秒过期、单次使用。同一 ID 配不同公钥、或同一公钥配不同 ID 为 `ErrIdentityConflict`，不覆盖记录。
- **设备与关联**：`devices` 一行一台设备（`pending`/`trusted`/`revoked` 墓碑）；`device_bindings` 一行一条“设备×后端实例”路径（`pending`/`authorized` + 独立 `sync_enabled`）。握手通过（`recordPeer`）只会建立 `pending` 设备与 `pending` 关联；待批准设备上限 64（`tailcat_host.go`，仅限新设备与回流的墓碑；超限 `CheckPeer` 返回 reject，此时配对码/票据已被消耗）。有效权限 = 设备 `trusted` ∧ 关联 `authorized`（`allowedPeerOn`、`trustRevision`）。
- **批准**：`TrustDevice` 必须带用户看到的指纹，不符即 `identity_mismatch`；它不授权任何路径，每条关联另需 `AuthorizeBinding`。桌面 `Node.Approve` 使用 Engine `ApproveDevice`，必须带用户核对的指纹、设备修订和完整待批准关联的 ID/修订；一个 SQLite 事务内检查整个集合后信任设备并授权这些关联。任一关联新增、消失、修订或上下文变化、设备修订变化，整个审批均失败，不留下部分批准；已信任设备新增关联也检查指纹。
- **撤销**：`RemoveBinding` 只撤一条路径；`UnpairDevice` 一个事务内置 `revoked` 并删除全部关联，事务提交后另行丢弃该设备的担保（`DropIntroducer`，非原子，错误被忽略）。桌面 reject 等价于 unpair（留墓碑），被拒设备再拨入会以 `pending` 回来并占用名额。已有会话在 `PeerEvent`/状态屏障（`pruneSessions`）里按信任修订失效。被撤销设备再次出现只回到 `pending`，需重新批准。`store.AddBinding` 在 `remote_context` 变化时会把关联重置为 `pending`，但当前唯一调用路径（`recordPeer`）总是传空上下文，此路径不会触发。
- **指纹**：`identity.Fingerprint(a, b)`，两端公钥按字节排序，对 `"unic/fingerprint/v1\x00"‖a‖b` 做 SHA-256 取前 16 字节，显示为 8 组 4 位大写十六进制，双方各自计算；比对时忽略空白与大小写。DeviceID 为 `SHA-256("unic/device-id/v1\x00"‖公钥)` 前 16 字节的小写 base32（26 字符）。
- **配对码**：`CreatePairingCode` 向 rendezvous（`/v1/web-pairings`，地址由配置 `rendezvous_url` 或环境变量 `UNIC_RENDEZVOUS_URL` 覆盖（后者在桌面 `node.go` 应用））登记票据，得到 `NNN-NNN` 码，最长 300 秒、一次性；同一时刻只有一张活动票据，新建会替换并 consume 旧码。入码方 `JoinByCode` 解析码得到票据（kind `unic-pair`，含出码设备公钥、后端描述、128 位 token），经出票的后端实例拨号，并要求对端 DeviceID 等于票据所指。token 不上线路，入码方发送 `HMAC(token, 转录)`（PairProof）；出码方 `pairUsedBy` 验证后立即作废票据并 consume 码，把该关联标记 `ticket`（仅说明来历）。无兼容后端的票据返回 `unsupported`，不回退 tailcat。
- **完整票据**：`ExportPairingTicket` / `JoinByTicket`，文本形式 `unic-pair:…`，不经 rendezvous。桌面入口：`unic code|join|pair|requests|approve|reject|unpair|suggestions|suggest-*`；HTTP `/v1/code`、`/v1/join`、`/v1/pairing-ticket`、`/v1/peers`、`/v1/pending[/{id}/approve|reject]`、`/v1/candidates[/{id}/pair|ignore]`。
- **介绍**：全局 `sync.introductions`（默认开）经同步状态下发给后端；Engine 只把设备 `trusted`、关联 `authorized` 且有地址（≤8192 字节）的设备作为 peers 交给后端；`builtin.tailcat` 的 `internal/session` 读取 `SyncState.Introduce`，在认证会话建立、状态变化及每 24 小时把当前担保列表分批发送（每批 ≤64）；接收方使用本机会话 ID/token 调用 `PeerAuthority.Introduce`（外部后端对应 `sync_host.introduce`）。消息与内容共用串行写锁，发送失败关闭会话；关闭介绍开关后停止发送与接收。收到的 `Introduce` 须来自已连接、令牌与信任修订都匹配的会话；ID 须合法、非自身、非介绍方、有地址，名称非法则丢弃名称（仅格式校验）；每次 `introduce` 调用最多 64 个候选；已有设备记录的 ID 跳过。候选保留 7 天（未被重复担保即过期），每介绍方最多 128、总计最多 256。`PairCandidate` 发起一次普通拨号（无 token），之后对方不在线则在 24 小时内按 1→15 分钟退避重试；`IgnoreCandidate` 可忽略/恢复。介绍方被 Unpair 时丢它的担保（`RemoveBinding` 不丢），已建立的设备不受影响。

## 不变量

- 配对码/票据只用于找到地址并证明“持有本次票据”，不授信：出码方的设备记录只能经 `recordPeer` 以 `pending` 入库，只有 `TrustDevice` + `AuthorizeBinding` 能改（`engine/tailcat_host.go`、`internal/store/devices.go`）。`ticket` 标记不影响授权。
- 授权必须双方各自批准：本机 Engine 只在本机一侧批准（设备 `trusted` ∧ 关联 `authorized`）后才发放会话；对端是否批准由对端 Engine 决定，后端经 verdict 互换转告（`internal/session/engine.go`），本机 Engine 不独立验证（`remote_accepted` 仅作字段校验），`AwaitingRemoteApproval` 来自后端 `Connect` 结果。
- 批准绑定指纹：`TrustDevice` 与 `ApproveDevice` 比较 `ExpectedFingerprint` 与本机算出的值；HTTP 批准请求必须带 `fingerprint`、`revision`、`bindings:[{id,revision}]`，空值、坏 JSON、过期修订均不能批准。GUI 回传所展示的快照；CLI 使用 `unic approve <id> '<另一台设备显示的指纹>'`，获取本机待批准修订并提交用户提供的核对值，不能自动填充指纹。
- `AuthorizeBinding` 要求设备已 `trusted`；`trusted` 设备被 `UnpairDevice` 后不可经发现复活，只能作为新 `pending` 重来。
- 身份只由 Engine 验证：后端报告的“已验证”不被采信；私钥不出 Engine，后端没有任意载荷签名接口；插件只能得到 Engine 在握手中产生的 Hello/Proof 字节。
- 配对/介绍产生的新拨号只能由 Engine 创建 attempt（固定地址、期望 DeviceID、token，一次连接即作废，最多 16 个并发）；对已授权设备的常规重连由后端按下发的 peers 自行拨号，无 attempt，但除非 `CheckPeer` 返回 allow 否则没有会话。握手拒绝自己的公钥，PairProof 错误则整个握手失败。
- 票据 token 与地址（`attempt.address`、`bd.Endpoint`、票据文本、候选地址）是秘密：不进设备列表 DTO、状态、事件、错误信息（错误经 `mapNetErr` 脱敏为稳定码）。`Node.Status` / `GET /v1/status` 不签发或返回票据。桌面通过显式 `POST /v1/pairing-ticket`（`Cache-Control: no-store`）、`unic addr` 或 GUI「生成票据」导出，最长 300 秒有效且一次性。
- 票据只能在签发它的后端实例上兑现（`pairUsedBy` 比较 `backend`），且同一票据只能兑现一次。
- 介绍只是建议：`Introduce` 只写 `candidates` 表，不创建设备、不授信；候选里的名称未经认证；介绍方的会话须仍被批准且信任修订匹配。
- 设备用户名与自报名分开存储（`name` / `reported_name`），发现与握手只更新自报名。

## 代码地图

| 路径 | 职责 |
|---|---|
| `internal/identity` | 密钥文件、DeviceID、指纹、Hello/转录/Proof、PairProof；向量见 identity-v1.json |
| `engine/peerauth.go` | 握手状态机、attempt、会话与 `PeerEvent` 校验、`pruneSessions` |
| `engine/pairing.go` | 票据、配对码、`JoinByCode/JoinByTicket`、`dialPeer`、配对操作 |
| `engine/tailcat_host.go` | `recordPeer`（pending 入库）、`allowedPeerOn` |
| `engine/devices.go` | 设备/关联 API：Trust、Authorize、SetSync、Remove、Unpair |
| `engine/candidates.go` | 介绍：`Introduce`、候选列表、配对/忽略、重试 |
| `engine/peerproc.go` | 向后端实例下发 peers（仅已授权）与 `Introduce` 开关 |
| `internal/store/devices.go`, `candidates.go` | 设备/关联/候选表与 CAS、墓碑、上限 |
| `internal/rendezvous` | 配对码服务客户端、票据编码 |
| `internal/session/engine.go` | tailcat 会话层：握手搬运、介绍收发（`introduceLoop`） |
| `app/desktop/internal/host/node.go`, `internal/api/api.go`, `cmd/unic/cli.go`, `gui.go` | 桌面适配：批准、HTTP 路由、CLI |

## 验证

- 桌面三节点回环集成测试 `TestBuiltinSessionIntroducesTrustedDevices` 使用真实 Engine 与 builtin.tailcat 会话层，验证 A–B、B–C 配对后双向收到候选且不会自动授信；不代表真实 tailcat 隧道或多进程真机验收。
- Engine 覆盖设备/关联修订、上下文变化、缺少关联及已信任设备的指纹校验；存储触发器故障验证事务中途失败时回滚全部信任/授权。

## 规格与背景

- 契约：[device-contract](../contracts/device-contract.md)（设备/关联/撤销）、[identity-protocol](../contracts/identity-protocol.md)（密码学协议与向量）、[engine-contract](../contracts/engine-contract.md)、[sync-backend-contract](../contracts/sync-backend-contract.md)（后端侧握手/介绍 RPC）、[plugin-rpc-contract](../contracts/plugin-rpc-contract.md)。

## 缺口

- macOS 客户端的添加设备/待批准界面仅为原型（随机码、示例数据），未接 Engine；无设备介绍（候选）界面。
- 对象存储路径（仓库成员身份、`object_store` 票据兑现、成员排除）未实现；`object_store` 票据当前只被解析，不能使用。
- 密钥轮换、备份恢复、多设备共享身份未定义。
- 待批准设备数达上限时新握手直接被拒绝（`reject`），尚无面向用户的清理提示。
