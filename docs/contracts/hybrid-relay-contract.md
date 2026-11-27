# 混合中转同步与分块接力契约（Direct-to-S3）

- 状态：Phase 0、Phase 1 与 Phase 2 已实现并经 `-race` 端到端验证（`internal/relaycrypto`、确定性向量、`internal/relayserver`、`cmd/unic-relay`、`internal/relayclient`、`wire.RelayProfile`/`RelayHandoffOffer`/`Ack`/`RelayChunkData`、`engine` 混合路由/共享 `TransferAssembly`/Level 3 P2P 中途位图结算与断线反向回落/接收放弃/吊销、配置与桌面/移动端适配）；中转数据面已切换为 **Direct-to-S3 预签名对象存储直传**（对齐 `UniClipboard/relay` Hyper Relay API v2）；Phase 3（官方托管上线与移动端后台真机实测）待实施
- 日期：2026-11-26
- 依据：[身份与信任](../architecture/identity-and-trust.md)、[流水线与本机历史](../architecture/pipeline.md)、[统一身份协议](identity-protocol.md)、[设备契约](device-contract.md)、[流水线提交契约](pipeline-contract.md)、[同步后端业务契约](sync-backend-contract.md)、[Engine 公共契约](engine-contract.md)、[0034 同步发送进度与接收回执](../decisions/proposed/0034-sync-transfer-progress-and-receipt-ack.md)

本文定义 `unic` 混合同步传输（P2P 在线直连优先 + E2EE 临时中转 Direct-to-S3 0 等待回退 + 大载荷统一分块 P2P 接力续传）的密码学格式、Hyper Relay Direct-to-S3 控制/数据面 API、P2P 接力控制帧、Engine 调度与资源边界。

---

## 1. 角色与职责边界

> **两类独立中继程序边界**：本文定义的**混合传输 HTTP 中转服务（`UniClipboard/relay` / `hyper-relay`）**是应用层 E2EE 短 TTL 零知识控制面（配合 S3/COS/R2/MinIO 预签名直传），与 **`builtin.tailcat` 所使用的底层 P2P/DERP 隧道中继节点（如 `derper`）**是两个职责正交、进程与部署完全独立的程序。用户可独立自部署任意其一或同时自部署两者，互不耦合。

| 组件 | 职责 | 明确禁止 |
|---|---|---|
| **Engine（含 `internal/relaycrypto`）** | 持有设备身份私钥与信任表；决定每目标路由（P2P 直连 vs 中转回退）；生成 `TransferID`、加密/解密/签验分块与信封；维护共享接收暂存组装器（`TransferAssembly`）、跨通道去重、配额、撤销屏障与单路径策略执行 | 不直接硬编码官方云厂商 SDK；不向日志/事件/错误泄露密钥、令牌、接入密钥、预签名 URL、自定义 URL 或正文 |
| **内置 P2P 传输（`builtin.tailcat`）** | 维持既有 `peer_sessions` 长连接（经直连或独立 DERP 中继）；在认证会话上交换 `relay_profile`；在 Phase 2 承载 `relay_handoff_*` 与 `relay_chunk_data` 控制/数据帧；响应非阻塞重连提示 `WakePeer` | 不持有设备私钥；不自行绕过 Engine 授信或决定中转回退 |
| **内置中转客户端（`internal/relayclient`）** | 按 Direct-to-S3 Relay API 执行信封公告、预签名直传 `PUT` + `commit`、预签名直下 `GET`、SSE 订阅、消费确认（ACK/DELETE）与投递令牌吊销；对预签名对象存储 URL 严格执行凭据剥离、SSRF/私网 IP 阻断与脱敏 | 不接触明文剪贴板内容；不依赖 `tailcat`/`tcnet`；不向对象存储发送中转鉴权头；不记录预签名 URL；不自行放宽 SSRF/大小/超时限制 |
| **混合 HTTP 中转服务端（官方托管 / 自部署 `UniClipboard/relay`）** | 零知识控制面：验证请求签名、投递令牌、可选自部署实例准入控制与配额；签发短时效 AWS SigV4 预签名 PUT/GET URL；在 `commit` 时经 S3 `HeadObject` 校验对象存在性与字节上限并经 SSE 推送事件；在消费确认、取消或 TTL 到期后经 `DeleteObject` 立即清理密文对象 | **不中转密文分块字节流**；不持有任何设备私钥或解密密钥；无法查看明文、文件名、MIME 或来源应用；不作为设备信任权威；不耦合 DERP 隧道转发 |
| **宿主（桌面 `unicd` / 移动端 `app/expo`）** | 显式装配内置中转客户端并提供中转开关、自部署 URL 与可选接入密钥配置；向用户区分展示直连与中转状态；管理移动端前后台网络与上传保活 | 不在 iOS Share Extension 中启动第二个 Engine 实例 |

---

## 2. 密码学协议与信封格式 v1

所有整数大端编码；所有二进制字段在 JSON 中采用无填充 base64url（RFC 4648 §5）；`field(x) = u16_be(len(x)) || x`（沿用 [identity-protocol.md](identity-protocol.md#2-原语与编码)）。

### 2.1 原语与依赖

- 身份签名：Ed25519（RFC 8032，沿用 `internal/identity`）
- 密钥协商：X25519（`crypto/ecdh`，RFC 7748）
- Ed25519 → X25519 映射：
  - 私钥：由 32 字节 Ed25519 seed 计算 `h = SHA512(seed)`，取 `h[:32]` 按 RFC 8032 clamp 得到 X25519 标量私钥；
  - 公钥：通过标准 Edwards→Montgomery 双有理映射（`filippo.io/edwards25519` `Point.SetBytes(pk).BytesMontgomery()`）得到 32 字节 X25519 公钥；拒绝非法编码、低阶点及产生全零共享密钥的输入。
- 密钥派生：HKDF-SHA256（RFC 5869）
- 对称认证加密：AES-256-GCM（12 字节 nonce，16 字节 tag）
- 摘要：SHA-256

### 2.2 统一逻辑载荷流（Plaintext Stream）

一次发送视图（经后端/单路径策略过滤后的最终 `sendSet`）按确定性字节流打包：

```text
PlaintextStream = u32_be(len(header_json)) || header_json || part[0].bytes || part[1].bytes || ...
```

- `header_json` 为 UTF-8 编码的 `wire.Clip` JSON（含 `t="clip"`、`id=transfer_id`、`origin=sender_device_id`、`ts`、`hash`、`source_application`、`parts[{kind,name,size}]`），最大 `wire.MaxControl`（1 MiB）。
- `part[k].bytes` 严格等于 `parts[k].size` 字节，按索引顺序紧接拼接。
- 将 `PlaintextStream` 按 `chunk_size` 切片为 `N` 个明文块 `P[0] .. P[N-1]`：
  - 总长度 ≤ `SINGLE_CHUNK_THRESHOLD`（默认 **2 MiB**）：`N = 1`，`chunk_size = len(PlaintextStream)`；
  - 总长度 > `SINGLE_CHUNK_THRESHOLD`：`chunk_size = 1048576`（1 MiB），`N = ceil(len(PlaintextStream) / chunk_size)`，上限 `MAX_CHUNKS = 4096`。

### 2.3 分块 AEAD 加密

每次传输由 CSPRNG 生成独立的 32 字节 `CEK`（Content Encryption Key）：

```text
chunk_key    = HKDF-SHA256(ikm=CEK, salt=SHA256("unic/relay-chunk-salt/v1\0" || field(transfer_id)), info="unic/relay-chunk-key/v1\0", len=32)
nonce_prefix = HKDF-SHA256(ikm=CEK, salt=SHA256("unic/relay-chunk-salt/v1\0" || field(transfer_id)), info="unic/relay-chunk-nonce/v1\0", len=8)
nonce(i)     = nonce_prefix || u32_be(i)
aad(i)       = "unic/relay-chunk-aad/v1\0" || field(transfer_id) || field(sender_id) || field(recipient_id) || u32_be(i) || u32_be(N) || u8(is_final)
C[i]         = AES-256-GCM.Seal(key=chunk_key, nonce=nonce(i), plaintext=P[i], aad=aad(i))
chunk_hash[i]= SHA256(C[i])
```

安全性质：
- 任意修改块序号、调换顺序、截断末块、替换到另一笔传输或转投其他设备均导致 AEAD 认证失败。
- 每个 `C[i]` 独立对应 `chunk_hash[i]`，接收端无论从 HTTP 还是 P2P 收到第 `i` 块，均可**立即单块校验并解密**，不依赖全量收齐。

### 2.4 目标密钥封装（Wrapped CEK）

针对单个接收设备 `R`（已认证 Ed25519 公钥 `pk_R`，映射为 `x_R`），发送方生成一次性临时 X25519 密钥对 `(eph_sk, eph_pk)`：

```text
shared     = X25519(eph_sk, x_R)
wrap_salt  = SHA256("unic/relay-wrap-salt/v1\0" || field(relay_origin) || field(sender_id) || field(recipient_id) || field(transfer_id))
wrap_key   = HKDF-SHA256(ikm=shared, salt=wrap_salt, info="unic/relay-wrap-key/v1\0" || eph_pk || x_R, len=32)
wrap_nonce = HKDF-SHA256(ikm=shared, salt=wrap_salt, info="unic/relay-wrap-nonce/v1\0" || eph_pk || x_R, len=12)
wrapped_cek= AES-256-GCM.Seal(key=wrap_key, nonce=wrap_nonce, plaintext=CEK, aad=envelope_aad)
```

`relay_origin` 为规范化后的中转源标识（如 `https://relay.uniclipboard.app`，小写 scheme/host、去除尾斜杠与默认端口），防止跨中转站点搬运信封。

### 2.5 外层签名信封（Outer Envelope）

```json
{
  "v": 1,
  "transfer_id": "<32-hex>",
  "sender_id": "<26-char-device-id>",
  "recipient_id": "<26-char-device-id>",
  "created_ms": 1764124800000,
  "expires_ms": 1764125400000,
  "chunk_size": 1048576,
  "total_chunks": 12,
  "total_ciphertext_bytes": 12103456,
  "eph_pub": "<base64url-32B>",
  "wrapped_cek": "<base64url-48B>",
  "chunk_hashes": ["<base64url-32B>", "..."],
  "sig": "<base64url-64B>"
}
```

- `envelope_aad` 与签名原文共用规范化二进制拼接（不依赖 JSON 键顺序或空白）：

```text
envelope_body = "unic/relay-envelope/v1\0"
  || field(relay_origin)
  || u8(v)
  || field(transfer_id)
  || field(sender_id)
  || field(recipient_id)
  || u64_be(created_ms)
  || u64_be(expires_ms)
  || u32_be(chunk_size)
  || u32_be(total_chunks)
  || u64_be(total_ciphertext_bytes)
  || eph_pub(32)
  || SHA256(chunk_hashes[0] || ... || chunk_hashes[N-1])

envelope_aad = SHA256("unic/relay-envelope-aad/v1\0" || envelope_body)
sig          = Ed25519.Sign(sk_sender, "unic/relay-envelope-sig/v1\0" || envelope_body || wrapped_cek)
```

接收端强制验证顺序（任一失败直接丢弃且不进入解密/写盘）：
1. `v == 1`，`recipient_id == self_device_id`，`sender_id != self_device_id`；
2. 本地时钟检查：`expires_ms > now` 且 `created_ms <= now + 5min` 且 `expires_ms - created_ms <= 30min`；
3. 查本机 `devices` / `device_bindings`：`sender_id` 必须为 `trusted`、关联已授权且 `sync_enabled=true`；
4. 用本机已登记的 `sender_pk` 验证 `sig`（绝不信任信封自报公钥）；
5. 检查 `total_chunks`、`chunk_size`、`total_ciphertext_bytes` 在全局与本机 `Limits` 内，且跨通道去重表未命中相同 `(sender_id, transfer_id)`；
6. 仅在上述全部通过后，解封 `wrapped_cek` 并进入分块拉取/接收组装。

---

## 3. 信箱路由、投递令牌与服务端 HTTP API v1

### 3.1 信箱标识与路由交换（`relay_profile`）

- 接收方在选定的 `relay_origin` 上派生不透明信箱 ID：
  ```text
  mailbox_id = lower(base32_nopad(SHA256("unic/relay-mailbox/v1\0" || field(relay_origin) || recipient_pk)[:20])) // 32 chars
  ```
- **投递令牌（Delivery Token）**：
  - 接收方为每个已授权对端设备生成独立的 256-bit 随机 `delivery_token`（可按对端独立吊销）；接收方将 `SHA256(delivery_token)`（而非明文令牌）注册到中转服务端的该信箱白名单（每信箱上限 64 条）。
  - 在 `builtin.tailcat` 认证会话建立（且双方在 `wire.Hello.Caps` 中协商了 `CapRelayProfile = "relay_profile"`）或配置更新时，双方通过有界异步控制写队列交换：
    ```json
    {
      "t": "relay_profile",
      "v": 1,
      "relay_origin": "https://relay.uniclipboard.app",
      "mailbox_id": "<32-char>",
      "delivery_token": "<base64url-32B>",
      "max_bytes": 26214400,
      "epoch": 1
    }
    ```
  - `delivery_token` 与 `mailbox_id` 属于秘密字段，存入本机受保护数据库，**绝不出现在状态 DTO、事件、日志或错误信息中**。
  - **SSRF 与端点校验**：发送端仅接受满足以下条件之一的 `relay_origin`：(a) 等于本机配置的官方/自托管 `relay.url`；或 (b) 位于用户显式允许的自托管白名单中且为合法 HTTPS（本地环回仅允许测试模式显式开启）。绝不自动连接对端自报的未知主机或私网 IP。
  - **撤销联动**：执行 `UnpairDevice` 或 `RemoveBinding` 时，本机立即删除该对端已保存的 `relay_profile`，并向自身中转服务提交 `DELETE /v1/relay/mailboxes/{mailbox_id}/senders/{token_hash}` 吊销其投递令牌（尽力而为；即便网络离线，本机接收验证第 3 步仍会彻底拒绝该设备信封）。

### 3.2 接收方请求鉴权（Owner Auth）

所有读/管信箱请求必须携带以下 HTTP Header：
- `X-Unic-PubKey`: 接收方 32 字节 Ed25519 公钥（base64url）
- `X-Unic-Timestamp`: Unix 秒级时间戳（允许与服务端偏差 ±60s）
- `X-Unic-Nonce`: 16 字节随机值（base64url，服务端在 120s 窗口内防重放）
- `X-Unic-Sig`: Ed25519 签名，签名为：
  ```text
  "unic/relay-auth/v1\0" || field(relay_origin) || field(METHOD) || field(PATH_AND_QUERY) || field(timestamp) || field(nonce) || body_sha256(32)
  ```

服务端校验 `DeriveMailboxID(relay_origin, PubKey) == path.mailbox_id` 且签名有效。

### 3.2a 实例准入控制（官方 Cloud API Key 与自部署防白嫖）

官方托管实例与自部署 `hyper-relay` 实例均支持以下准入控制（不影响零知识 E2EE 属性）：

1. **公钥白名单（`RELAY_ALLOWED_PUBKEYS`）**：服务端仅允许白名单内的 `X-Unic-PubKey` 执行 Owner Auth（注册/吊销投递令牌、订阅 SSE、拉取与确认消费）；
2. **接入密钥 / Cloud API Key（`X-Unic-Relay-Key`）**：
   - **官方托管（`https://relay.uniclipboard.app`）**：用户登录 **UniClipboard Cloud**（`https://cloud.uniclipboard.app`）后，经 OAuth 2.0 PKCE 一键授权自动签发（或在 `/account/api-keys` 手动创建）用户专属 API Key（前缀 `uc_rk_`）。客户端在请求中转控制面时通过 `X-Unic-Relay-Key` 携带；中转服务端仅将 `SHA-256(key)` 发往 Cloud 校验接口（带短 TTL 缓存），绝不透传或记录明文 Key。
   - **自部署实例**：服务端可配置静态 `RELAY_ACCESS_KEY`（常量时间比较）；未配置 Cloud 校验端点时回落静态密钥模式。
   - 客户端经 `sync.relay.access_key`（或环境变量 `UNIC_RELAY_ACCESS_KEY`）保存，视为秘密字段，绝不写入日志、事件、状态 DTO、备份或普通错误，且请求对象存储预签名 URL 时严格剥离。

### 3.3 HTTP API（Direct-to-S3 控制面 + 区域路由对象存储直传数据面）

所有控制面端点均基于配置的 `<base_url>`（官方全球统一入口 `https://relay.uniclipboard.app`，保证全球设备、出差或切换代理时 `relay_origin` 与 `mailbox_id` 一致），密文分块字节流**不经过中转服务**，由客户端凭短时效 AWS SigV4 预签名 URL 直连对象存储（官方实例按传输来源区域自动路由：**中国大陆直传腾讯云 COS，海外直传 Cloudflare R2**，每笔传输持久化其 `storage_profile` 以确保跨区接收与 ACK/TTL 清理命中同一存储桶；自部署支持任意 S3/COS/R2/OSS/MinIO）：

| 方法与路径 | 鉴权 | 作用 |
|---|---|---|
| `GET /healthz` | 无 | 健康检查，返回 `{"status":"ok","service":"hyper-relay","storage":"s3-direct"}` |
| `PUT /v1/relay/mailboxes/{mailbox_id}/senders/{token_hash}` | Owner Auth | 登记/更新一个允许向本信箱投递的令牌哈希（支持带 `max_bytes`） |
| `DELETE /v1/relay/mailboxes/{mailbox_id}/senders/{token_hash}` | Owner Auth | 吊销指定对端投递令牌 |
| `POST /v1/relay/mailboxes/{mailbox_id}/transfers` | `Authorization: Bearer <delivery_token>` | **统一创建传输（单块与多分块统一入口）**：提交外层签名 `envelope` JSON，立即推送 SSE `transfer_announced` 事件，返回 `{transfer_id, expires_at_ms, max_chunk_bytes, upload_mode: "presigned", upload_urls: [{index, method, url, headers?, expires_at_ms}, ...]}`（最多内联前 16 块的预签名 `PUT` 授权） |
| `POST /v1/relay/mailboxes/{mailbox_id}/transfers/{transfer_id}/chunks/{index}/upload-url` | `Authorization: Bearer <delivery_token>` | 为超出内联数量（`index >= 16`）或预签名已过期的分块签发/刷新短时效预签名 `PUT` URL，返回 `{method: "PUT", url, headers?, expires_at_ms}` |
| *直连对象存储 `PUT <presigned_url>`* | URL 预签名 | 发送端直传第 `index` 块密文 `C[index]` 至 S3 兼容对象存储（严禁携带任何中转鉴权头） |
| `POST /v1/relay/mailboxes/{mailbox_id}/transfers/{transfer_id}/chunks/{index}/commit` | `Authorization: Bearer <delivery_token>` | 确认第 `index` 块直传完成：服务端执行 S3 `HeadObject` 校验对象存在且大小合法（超限则立即删除对象并返回 `413`），置位可用位图并推送 SSE `chunk_ready` 事件，返回 `204` |
| `GET /v1/relay/mailboxes/{mailbox_id}/events` | Owner Auth | SSE 长连接（支持 `Last-Event-ID` 补拉与 `: keepalive` 心跳），推送 `transfer_announced` 与 `chunk_ready` 事件 |
| `GET /v1/relay/mailboxes/{mailbox_id}/transfers` | Owner Auth | 列举当前未过期的待领取传输摘要及可用分块位图（用于冷启动/前台恢复补拉） |
| `GET /v1/relay/mailboxes/{mailbox_id}/transfers/{transfer_id}/chunks/{index}` | Owner Auth | 为已提交分块 `index` 签发短时效预签名 `GET` URL，返回 `{method: "GET", url, expires_at_ms, size}` |
| *直连对象存储 `GET <presigned_url>`* | URL 预签名 | 接收端直连下载第 `index` 块密文 `C[index]`（严禁携带任何中转鉴权头；下载后强制校验 `SHA256(C[index]) == chunk_hashes[index]` 与 AEAD） |
| `POST /v1/relay/mailboxes/{mailbox_id}/transfers/{transfer_id}/ack` | Owner Auth | 接收方确认已完整接收（或已通过 P2P 接力完成），服务端立即 `DeleteObject` 删除对象存储中的全部分块与元数据 |
| `DELETE /v1/relay/mailboxes/{mailbox_id}/transfers/{transfer_id}` | Owner Auth 或对应 `delivery_token` | 发送方取消未完成上传或接收方主动拒绝/清理，服务端立即 `DeleteObject` 清理全部对象 |

#### Direct-to-S3 客户端强制安全约束

1. **严格剥离中转凭据**：请求预签名对象存储 URL 时，绝不发送 `Authorization`、`X-Unic-Relay-Key` 或任何 `X-Unic-*` Header；重定向目标同样剥离敏感 Header。
2. **预签名 URL 的 SSRF 防护**：默认强制 `https://`（仅在客户端配置为回环测试地址或显式测试模式时允许回环 `http://`），且在拨号阶段验证解析出的实际 IP，拒绝私网、环回、链路本地（含云元数据 `169.254.169.254`）、CGNAT、组播与未指定地址（防范 DNS Rebinding）。
3. **预签名 URL 脱敏**：预签名 URL 携带临时签名凭据，视为秘密，绝不写入日志、事件、状态 DTO 或错误文本。
4. **端到端完整性与大小硬边界**：下载前校验服务端申报的 `size <= maxBytes`，读取响应体时仍使用 `LimitReader(maxBytes+1)` 截断保护；解密前强制校验 `SHA256(C[i]) == envelope.chunk_hashes[i]` 与 AES-256-GCM 认证。

标准错误状态码：`400`（格式/大小越界）、`401`/`403`（签名或令牌无效）、`404`/`410`（不存在或已过期消费）、`409`（重复或冲突）、`413`（超单次/单块上限）、`429`（限流/信箱满）、`5xx`（服务端或对象存储暂不可用）。客户端一律映射为 Engine 稳定错误码，不透传服务端自由文本或预签名 URL。

---

## 4. 混合调度与回退语义（Engine 契约）

### 4.1 逐目标路由判定

当 `dispatchProcessed` 针对目标集合中的某台已授权设备 `D` 派发发送视图时：

1. 若 `builtin.tailcat` 存在与 `D` 的有效活跃会话（`connected` 且 `trustRevision` 匹配）→ 派发至 P2P，**不调用中转**。
2. 否则，若中转已启用、策略允许、存在有效 `relay_profile` 且视图总字节数 ≤ `effective_relay_max_bytes` → **0 等待启动中转发送**，同时异步调用 P2P 传输的 `WakePeer(D)` 重置退避并立即尝试建连。
3. 否则：
   - 若超过中转大小上限 → 目标记录失败/跳过原因 `relay_size_exceeded`（自动同步）或返回明确可展示错误（显式发送）；
   - 若无中转路由或中转已禁用 → 维持原有 `no_targets` 行为。

### 4.2 Operation、TargetStatus、进度与入站放弃（对齐 0034）

直接复用 [0034](../decisions/proposed/0034-sync-transfer-progress-and-receipt-ack.md) 落地到 `engine` 与 `/v1/operations` 的进度与回执数据结构，不重复引入平行的进度或阶段字段：

- **复用 `TargetStatus.Progress`（`TransferProgress`）**：上报中转分块上传及 Phase 2 接力续传的单调字节/分块进度，沿用 `EventOperations` 200 ms 合并节流、`Operation.HistoryID` 与 `ListOperations`；绝不写入 SQLite，不含文件名或路径。
- **扩展 `ReceiptStage` 枚举**：在现有 `"local_written"` / `"remote_received"` / `"remote_discarded"` 基础上新增 **`"relay_stored"`**（中转服务端已接受完整密文分块，等待目标设备领取）。
- **新增正交字段 `TargetStatus.TransportRoute`**：有限枚举 `""`（未确定）/ `"p2p"` / `"relay"` / `"hybrid"`（发生过 Phase 2 P2P 接力续传）。
- **发送端终态语义**：
  - 纯中转且所有分块上传成功获服务端确认后，目标进入 `TargetSent` 且 `ReceiptStage="relay_stored"`、`TransportRoute="relay"`；宿主 UI **不得**将 `relay_stored` 渲染为“对方设备已收到或已写入剪贴板”。
  - Phase 2 经 P2P 接力续传完成且双方已协商 `clip_ack` 时，接收端在 `TransferAssembly` 提交或丢弃后经 P2P 回传 `clip_ack`，将发送端目标推进为 `TargetSent` + `ReceiptStage="remote_received"`、`TargetDiscarded` + `ReceiptStage="remote_discarded"` 或 `TargetFailed`（`TransportRoute="hybrid"`）。
- **入站 `OperationReceive` 与中途 `DiscardReceive`**：
  - 经 HTTP 中转下载及 `TransferAssembly` 跨通道拼装的入站传输，统一登记为内存态 `OperationReceive`（`PhaseReceiving` → `PhaseCompleted`）并上报单调 `ReceiveProgress`，直接复用桌面与移动端的接收进度 Toast；
  - 当用户在接收中途调用 `Engine.DiscardReceive(ctx, opID)`（`POST /v1/operations/{id}/discard`）时，Engine 立即中止 HTTP 分块下载与写盘、删除 `TransferAssembly` 暂存、释放配额预留，尽力发送短超时 `DELETE .../transfers/{id}` 清理中转分块，且若存在活跃 P2P 接力会话则同步通过异步控制队列回报 `clip_ack{status:"discarded"}`（或拒绝待确认的 `relay_handoff_offer`），让发送端及时停止续传并转为 `TargetDiscarded`（`ReceiptStage="remote_discarded"`）。

### 4.3 取消、挂起与撤销屏障

- `sync.enabled=false`、`SetRuntimeState(suspended)`、暂停关联、移除关联或 `UnpairDevice` 均立即取消所有在途中转上传/下载 HTTP context，关闭活动 SSE 连接，等待活跃 I/O 退出后确认撤销屏障。
- 取消在途中转上传时，发送方尽力发起一次短超时（≤ 2s）`DELETE .../transfers/{id}` 清理未完成分块；不阻塞关闭预算。
- 重启进程后不自动重放未完成的中转上传；中转未消费对象随 TTL 自动过期清理。

---

## 5. Level 3 分块结算与 P2P 接力续传协议（Phase 2）

本节定义大载荷（`total_chunks > 1`）在 HTTP 上传中途、目标 P2P 会话变为 `connected` 时的无缝接力流程。

### 5.1 切换触发条件

发送端同时满足以下条件时发起接力结算：
1. 该目标当前处于 `relay_uploading` 状态且未取消；
2. 目标 `D` 刚刚建立或恢复了经认证且授权有效的 P2P 会话（`trustRevision` 匹配），且双方在 `wire.Hello.Caps` 中协商了 `CapRelayHandoff = "relay_handoff"`；
3. 尚未向中转上传完成的剩余字节数 ≥ `HANDOFF_MIN_REMAINING_BYTES`（默认 **2 MiB**）且剩余块数 ≥ 2；
4. 尚未对同一笔 `(transfer_id, peer_id)` 超过单次传输最大接力切换次数（默认上限 **3 次**，防止抖动网络下反复横跳）。

### 5.2 `wire v1` 能力协商与可选控制帧

保持 `wire.Version == 1`，沿用 [0034](../decisions/proposed/0034-sync-transfer-progress-and-receipt-ack.md) 的 `wire.Hello.Caps` 机制新增两项可选能力：
- `CapRelayProfile = "relay_profile"`（Phase 1）：启用会话上的 `relay_profile` 路由与投递令牌交换；
- `CapRelayHandoff = "relay_handoff"`（Phase 2）：启用 `relay_handoff_offer`、`relay_handoff_ack` 与 `relay_chunk_data` 分块接力帧。

对端未声明对应能力时，发送端不发出对应控制帧并保持纯 HTTP 或普通 P2P 平滑降级。`relay_profile`、`relay_handoff_offer` 与 `relay_handoff_ack` 均经 `internal/session` 的有界异步控制写队列发送，严禁在 `recvLoop` 内持锁阻塞写连接。

在已认证且已协商 `relay_handoff` 的 `builtin.tailcat` 会话上定义以下三类消息：

1. **发送端 → 接收端：`relay_handoff_offer`**
   ```json
   {
     "t": "relay_handoff_offer",
     "v": 1,
     "transfer_id": "<32-hex>",
     "envelope": { "...": "完整的外层签名信封（含 chunk_hashes 与 sig）" },
     "relay_uploaded_bitmap": "<base64url-bitset>"
   }
   ```
   位图按小端字节序、每字节 LSB 优先编码：`chunk i` 位于字节 `i/8` 的 bit `i%8`。

2. **接收端 → 发送端：`relay_handoff_ack`**
   ```json
   {
     "t": "relay_handoff_ack",
     "v": 1,
     "transfer_id": "<32-hex>",
     "accept": true,
     "reason": "",
     "have_bitmap": "<base64url-bitset>",
     "want_p2p_bitmap": "<base64url-bitset>"
   }
   ```
   - 接收端收到 `offer` 后，先按 §2.5 完整校验 `envelope` 签名、信任与策略，并查找/创建本机共享 `TransferAssembly(sender_id, transfer_id)`：
     - 若该 `(sender_id, transfer_id)` 已完成提交（命中去重）→ `accept=true`、`have_bitmap` 全 1、`want_p2p_bitmap` 全 0（发送端立即结束并释放中转）；
     - 若拒绝（配额不足、策略禁止、已撤销）→ `accept=false`、`reason` 为稳定码；
     - 若接受：
       - `have_bitmap` = 本机暂存中已通过 `chunk_hashes[i]` 校验并落盘的分块集合；
       - 对于 `¬have_bitmap` 的缺失块：
         - 未在 `relay_uploaded_bitmap` 中的块 **必须** 加入 `want_p2p_bitmap`；
         - 已在 `relay_uploaded_bitmap` 中且接收端尚未开始下载的块，接收端可选择一并加入 `want_p2p_bitmap`（避免公网下行中转）或保留由本机 HTTP 并发拉取。

3. **发送端 → 接收端：`relay_chunk_data`**
   - 先发长度前缀 JSON 控制帧：
     ```json
     {
       "t": "relay_chunk_data",
       "v": 1,
       "transfer_id": "<32-hex>",
       "index": 5,
       "size": 1048592
     }
     ```
   - 紧随其后在同一流上写出恰好 `size` 字节的加密分块 `C[5]`（与中转格式逐字节一致）。
   - 沿用现有 [30 秒单次正文 I/O 不活跃截止时间](sync-backend-contract.md#built-in-peer-session-body-lifetime)。

### 5.3 接收端共享组装器（`TransferAssembly`）与回落容错

- **单份资源记账**：同一个 `(sender_id, transfer_id)` 无论同时有多少块来自 HTTP、多少块来自 P2P，只占用**一份** `total_plaintext_bytes` 的接收预留与一个有界暂存目录（位于 `InboxDir/unic-recv` 下，受启动清理与 `MaxStoredBytes` 约束）。
- **逐块写入与校验**：每收到一块 `C[i]`（来自 HTTP 或 P2P），先校验 `SHA256(C[i]) == chunk_hashes[i]`，再用 `CEK` 解密，按块序落盘并置位 `have_bitmap[i]`；重复块幂等忽略。
- **统一提交与回执**：当 `have_bitmap` 全部 `0..N-1` 置位后，按序解析 `PlaintextStream` 中的 `wire.Clip` 元数据与各个 `part`，校验 `clip.Snapshot.Hash`，再走既有 `admitReceive` → `received` 处理器 → 历史与 `Received` 原子发布 + 持久化跨后端去重记录；随后异步向中转发送 `POST .../ack` 清理已上传分块。若本笔传输经由已协商 `clip_ack` 的 P2P 会话完成接力，接收端同时通过异步控制队列回传 `clip_ack{id: transfer_id, status: "received"|"discarded"|"failed"}`，对齐 0034 的端到端接收确认语义。
- **P2P 中途断线反向回落**：若 `relay_chunk_data` 推送中途 P2P 连接中断，发送端依据对端已知 `have_bitmap`（或未确认集合）恢复通过 HTTP `PUT` 上传剩余未传分块；接收端继续通过 SSE / `GET` 拉取缺失块，**已落盘的分块全部保留不丢失**。

---

## 6. 默认配置与资源硬边界

| 参数 | 默认值 | 硬上限 / 说明 |
|---|---|---|
| `sync.relay.enabled` | `false`（经 Cloud 授权或配置自部署后开启） | 关闭后零中转网络请求与零额外开销；官方中转需先登录 UniClipboard Cloud 获取 API Key 后一键开启 |
| `sync.relay.url` | `https://relay.uniclipboard.app` | 全球统一官方控制面入口（数据面自动按区域直连国内 COS / 海外 R2）；支持替换为自托管 HTTPS URL（或 `UNIC_RELAY_URL`）；与 Tailcat DERP 完全独立 |
| `sync.relay.access_key` | `""`（官方需 Cloud API Key；自部署可选） | 随控制面请求 `X-Unic-Relay-Key` 发送（或 `UNIC_RELAY_ACCESS_KEY`）；秘密字段，脱敏不入日志/状态 |
| `sync.relay.max_bytes` | `25 MiB`（官方默认） | 不得超过 `limits.max_total_bytes`；自托管可调大 |
| `SINGLE_CHUNK_THRESHOLD` | `2 MiB` | ≤ 2 MiB 走 1-RTT 单块上传 |
| `DEFAULT_CHUNK_SIZE` | `1 MiB` | 允许范围 `256 KiB .. 4 MiB` |
| `MAX_CHUNKS` | `4096` | 超过拒绝 |
| `RELAY_OBJECT_TTL` | `10 min` | 服务端硬上限 `30 min`；消费 ACK 即删 |
| `RELAY_UPLOAD_CONCURRENCY` | `3` | 单笔大传输并发上传分块数 |
| `HANDOFF_MIN_REMAINING_BYTES` | `2 MiB` | 低于此门槛不触发中途接力，直接 HTTP 收尾 |
| `MAX_ACTIVE_ASSEMBLIES` | `16` | 单实例并发未完成接收组装上限，超出按 `resource_exhausted` 拒绝新入站 |
| `ASSEMBLY_IDLE_TTL` | `10 min` | 超过过期时间或空闲超时立即回收暂存文件与配额 |

---

## 7. 验收测试矩阵（实现前置门控）

1. **密码学向量验收**：跨实现逐字节复现 `Ed25519→X25519`、`wrapped_cek`、分块 AEAD 加密/解密、外层信封签名、信箱 ID 与 Owner Auth 请求签名；篡改任意字段/块号/顺位/来源/目标/源域名均验证失败。
2. **零额外开销验收**：`sync.relay.enabled=false` 时运行既有 P2P 套件，验证零 HTTP 中转请求与零额外暂存占用。
3. **P2P 在线零中转验收**：双节点 P2P 已连接时发送文本/图片/文件，断言中转服务端请求数为 0。
4. **0 等待回退验收（Phase 1）**：断开/阻断 P2P 通道，发送端提交小文本（单块 1-RTT）与 8 MiB 文件（多分块），验证立即开始 HTTPS 上传、接收端实时收到并提交历史、中转服务端收不到明文与文件名、消费后分块即被删除。
5. **安全与撤销验收**：未授信设备、已 `UnpairDevice` 设备、暂停同步关联、过期信封、重放信封、错误 `delivery_token` 与 SSRF 内网 `relay_origin` 全部被拒绝。
6. **Level 3 中途结算与续传验收（Phase 2）**：限速 HTTP 上传 10 MiB 文件，上传至约 30% 时恢复 P2P 连接（已协商 `relay_handoff` 与 `clip_ack`），断言触发 `relay_handoff_offer`/`ack`、已传分块不重传、剩余分块经 P2P 补齐、接收端单次入库且文件 SHA-256 逐字节一致、中转临时分块被清理、发送端经 `clip_ack` 升级至 `ReceiptStage="remote_received"` 且 `TransportRoute="hybrid"`。
7. **P2P 接力中途断线反向回落与接收放弃验收（Phase 2）**：(a) 在 P2P 续传进行到 70% 时主动切断 P2P 连接，验证剩余 30% 自动回落 HTTP 补齐并完整提交，无重复记录、无暂存泄漏；(b) 在纯中转拉取及 P2P 接力中途分别调用 `DiscardReceive`，验证暂存与配额即时释放、中转分块被清理，且接力场景发送端收到 `discarded` 及时停传并落入 `TargetDiscarded`（`ReceiptStage="remote_discarded"`）。
