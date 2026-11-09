# 统一设备身份与配对协议 v1

- 状态：已批准（2026-11-08）；`internal/identity` 已按本文实现并通过向量。本文只定义密码学协议、编码与向量，不改变插件 RPC 帧格式。
- 日期：2026-11-08。
- 依据：[身份与信任](../architecture/identity-and-trust.md)、[设备契约](device-contract.md) §2–3。
- 向量：[`vectors/identity-v1.json`](vectors/identity-v1.json)，由独立脚本按本文规则生成；实现必须逐字节复现。

## 1. 目标与非目标

目标：(1) 设备身份独立于任何传输；(2) 远端必须证明持有登记的身份私钥，并把证明绑定到本次连接/仓库；(3) 重放、跨连接复用、降级、同 ID 异钥都被拒绝；(4) 用户可以靠一个与传输无关的指纹核对对端。

非目标：不提供密钥轮换、备份恢复、多设备共享身份（另定显式协议）；不提供内容机密性（P2P 由传输层承担，对象存储由对象协议承担）；不防御同用户权限下的恶意本机进程。

## 2. 原语与编码

- 签名：Ed25519（RFC 8032，纯 Ed25519，无上下文/预哈希）；哈希：SHA-256；MAC：HMAC-SHA256。仅使用标准库实现，不自造原语。
- `field(x)` = `u16_be(len(x)) || x`，`len(x) ≤ 65535`。所有整数大端。所有被签名/哈希的字节串都以带 NUL 结尾的 ASCII 域前缀开头，不同用途的域前缀互不相同，避免跨用途签名复用。
- 比较密钥、MAC、摘要一律常量时间。

## 3. 身份

- 本机身份是一个 Ed25519 密钥对，由 Engine 首次初始化时用 CSPRNG 生成，**与任何传输密钥（如 tailcat 节点密钥）无关**。
- `DeviceID(pk)` = `lower(base32_nopad(SHA256("unic/device-id/v1\0" || pk)[:16]))`，26 个字符。ID 由公钥导出，因此同一 ID 不可能对应两把不同公钥；若收到 ID 与公钥不匹配，或已登记 ID 的公钥不同，按“身份冲突”拒绝且不覆盖记录。
- 私钥存放：`DataDir/identity/device.key`，内容为 `UNIC-ID1\n` + base64(32 字节种子) + `\n`，权限 0600（目录 0700）。写入走临时文件 + fsync + 原子 rename + 目录 fsync。读取时重新导出公钥，格式不符、权限过宽、符号链接越界或与 `unic.db` 登记的公钥不一致时失败并报稳定错误，**不重新生成**。
- 私钥不离开 Engine，不通过 RPC 暴露；插件只能请求 Engine 对“本次握手转录”签名（§4.3），没有任意载荷签名接口。

## 4. P2P 身份证明（peer_sessions）

### 4.1 前提：传输上下文绑定

后端必须提供 `context_binding`：一个 ≤ 256 字节、**双方独立算出同一值**、且对本条传输连接唯一的字节串。例如 tailcat 使用“字典序排序的两端节点公钥 + 连接标识”。它的作用是把身份证明钉在这条连接上，防止中继/跨连接复用。实现注记（tailcat）：当前 tailcat 的 context_binding 由两端各自新鲜的 Hello nonce 保证每条连接唯一，而不是节点公钥加连接标识；传输层的对端密钥认证仍是前提。这与上例的构成不同，但满足“双方算出同一值且每条连接唯一”的要求，其他后端按同一要求自行取值。

传输本身必须已完成对端密钥认证（tailcat/WireGuard 满足）；后端若提供不了绑定值，不得声明 `peer_sessions`。

### 4.2 Hello

`Hello` = `u8(n) || versions[n] || pk(32) || nonce(32)`，`versions` 为支持的协议版本（本文为 `1`），`nonce` 为 32 字节 CSPRNG 随机数。发起方（dialer，记 I）与响应方（R）各发一个。选定版本为双方列表的最大公共值，无公共值则拒绝（`unsupported_version`）。Hello 整体进入转录，因此对版本列表的任何篡改（降级）都会使后续签名失效。

### 4.3 转录与证明

```
T = SHA256("unic/transcript/v1\0" || u8(kind) || field(context_binding) || field(Hello_I) || field(Hello_R))
Proof_I = Ed25519(sk_I, "unic/proof/v1\0" || 0x49 || T)      // 'I'
Proof_R = Ed25519(sk_R, "unic/proof/v1\0" || 0x52 || T)      // 'R'
```

`kind` = `1` 表示 peer_sessions。角色字节使 `Proof_I` 不能被当作 `Proof_R` 反射回去。双方在收到对方 Hello 后各自计算 T，收到对方 Proof 后用对方 Hello 中的 `pk` 验证。验证由 Engine 完成；后端只搬运 Hello/Proof 字节，其“已验证”之类的断言一律不被接受。

接受条件（全部满足）：版本选定成功；`pk` 长度正确，`DeviceID(pk)` 与已登记设备（若有）一致；签名验证通过；`nonce` 在本次尝试中未用过；从发出 Hello 起不超过 60 秒。任一失败：关闭连接，不留下可复用状态。

重放防护来自：双方新鲜 nonce + 连接绑定。Engine 对每次握手尝试保存 nonce，完成/失败/超时后丢弃；旧 Proof 在新连接上因 T 不同而无效。

### 4.4 配对令牌证明（仅配对码路径）

票据携带 128 位一次性 `tok`。入码方（I）不在线路上发送 `tok`，而发送：

```
PairProof = HMAC-SHA256(tok, "unic/pair-token/v1\0" || T)
```

出码方用本地保存的 `tok` 验证后立即作废该码（并通知 rendezvous consume）。这样窃听或恶意传输中继拿不到可重用的令牌，且证明绑定了本次转录。`PairProof` 只证明对方拿到过这张票，**不授信**：通过配对码进入的设备始终等待出码方用户批准（配对码与统一身份的原则不变）。

## 5. 指纹（人工核对）

```
FP = SHA256("unic/fingerprint/v1\0" || min(pk_A,pk_B) || max(pk_A,pk_B))[:16]
```

显示为 32 个大写十六进制字符，每 4 个一组共 8 组（128 位）。两端独立计算，与传输、配对路径无关，所以同一对设备经不同路径看到相同指纹，可记住“已核对”。

长度取 128 位是有意的：不使用提交-揭示的短 SAS 时，中间人可以对双方各自选择公钥来碰撞指纹，64 位只需约 2^32 次尝试，不够安全。界面可以分段展示、允许扫码/复制比对，但不得截短后宣称已核对。

## 6. 配对票据与入口

任何声明了配对能力的同步插件（不限 tailcat，P2P 或对象存储均可）都可以用同一套入口添加设备，由 Engine 统一处理，插件只提供自己的接入描述 `acc`：

1. **6 位配对码**：Engine 把票据登记到 rendezvous（`/v1/web-pairings`，沿用既有约束：`NNN-NNN`、5 分钟、一次性、限速），入码方输入后 resolve 得到票据。
2. **完整票据**：票据编码成一段文本（`unic-pair:` + base64url 的 JSON），可复制/粘贴、命令行传递或二维码；不经过 rendezvous，用于配对码服务不可用或不想经过第三方时。

两种入口得到的票据完全相同，之后的流程也相同；短码只负责“找到票据”，不授信。是否使用 rendezvous 是 Engine 的全局设置，插件不直接访问 rendezvous，也不自带配对码机制，所以新插件无需实现码服务即可获得短码配对。

票据 kind 仍为 `unic-pair`，版本升到 2（不与 v1 互通）：

```json
{"v":2,"kind":"unic-pair","tp":"peer_sessions","be":"<backend instance id>","dev":"<base64url pk>","acc":"<base64url opaque>","tok":"<base64url 16B>"}
```

- `tp`：传输种类，取 `peer_sessions` 或 `object_store`。入码方本地必须有声明相同种类且能处理该 `acc` 的兼容实例，否则返回“无兼容后端”，不回退到 tailcat。多个兼容实例时由用户选择。
- `be`：签发票据的后端实例 ID。入码方按它找实例，找不到即“无兼容后端”（不凭 `tp` 猜测，也不回退）；出码方只接受来自同一实例的配对证明。
- `dev`：出码方身份公钥。P2P 路径在 §4.3 中**必须**要求响应方 `pk` 与之相等；对象存储路径要求之后读到的该作者对象由此公钥签名。rendezvous 能替换整张票，所以仍须展示指纹。
- `acc`：插件定义的接入描述（tailcat 地址、仓库定位与加入材料等），对核心不透明，≤ 1 KiB，视为秘密，不进日志/事件/错误；其内容格式由对应插件的契约定义。
- `tok`：一次性令牌。P2P 经 §4.4 的 PairProof 证明；对象存储路径中，加入方在仓库里写入的“加入请求”对象须携带同样的 PairProof（转录换成该请求对象的 `repo_binding` 与 `seq`），出码方据此确认请求来自持票者。
- 完整票据 ≤ 2 KiB，因此二维码/短链场景可用；有效期、一次性、限速和“出码方必须批准”与配对码一致。完整票据没有第三方服务参与，但同样等同密码，界面默认折叠打码。

## 7. 对象存储来源证明（object_store）

对象存储没有在线握手，每个对象携带作者签名：

```
sig = Ed25519(sk_author, "unic/object/v1\0" || field(repo_binding) || field(author_device_id) || u64(seq) || u64(created_ms) || content_sha256)
```

- `repo_binding`：后端据仓库标识与仓库级根上下文导出的 ≤ 256 字节值（同一仓库恒定，不同仓库/账号不同）。签名因此不能搬到别的仓库。
- `seq`：作者在该 (repo_binding, 作者) 下严格递增的序号。Engine 保存每作者的最高序号，`seq ≤` 水位的对象视为重放并丢弃；序号空洞允许（离线/丢弃），水位不回退。
- 接收方先按 `author_device_id` 找到已登记公钥再验签；未登记作者的对象只产生“候选”，不产生内容。共享仓库凭据只证明能访问仓库，不代替本签名。
- 内容机密性、成员增删与密钥更新属于对象协议，不在本文；缺少它们的对象后端不得宣称已完成远端成员撤销（见设备契约 §5）。

## 8. 错误与限额

稳定错误码：`unsupported_version`、`bad_proof`、`identity_conflict`、`replay`、`expired`、`binding_mismatch`、`bad_pair_proof`。错误信息不含公钥以外的任何身份材料，不含 `acc`、`tok`、私钥或路径。

限额：Hello ≤ 128 字节；`context_binding`、`repo_binding` ≤ 256 字节；票据 ≤ 2 KiB；每个后端实例并发未完成握手 ≤ 8，同一来源单位时间超限则丢弃。

## 9. 向量与验收

`vectors/identity-v1.json` 提供：两把固定种子密钥及其 DeviceID、指纹、P2P 的 Hello/转录/双方 Proof/PairProof、一个对象签名。实现的验收测试：

- 逐字节复现向量中的全部派生值；
- 改动任一输入（`context_binding`、任一 Hello 字节、角色字节、`kind`）使验证失败；
- 把 `Proof_I` 当作 `Proof_R` 提交失败；把连接 1 的 Proof 用于连接 2 失败；
- 版本列表被篡改（降级）失败；同 ID 不同公钥返回 `identity_conflict`；
- 对象：改 `repo_binding`/`seq`/`content_sha256` 失败；`seq` 不大于水位判重放；
- 坏/越权/符号链接的身份文件拒绝启动且不重新生成。

## 10. 未决事项

- 密钥轮换/备份恢复：另行立项，本版本没有隐式路径。
- 如需更短的核对码（例如 6 位 SAS），须引入提交-揭示流程并另出版本，不在 v1 中降低指纹长度。
- RPC 侧方法已定义：`sync_host.identity_hello`/`identity_prove`/`check_peer`（[同步后端业务契约](sync-backend-contract.md) §3；Go DTO 在 `plugin/rpc/identity.go`，Schema 与向量见 `protocol/`）；Engine 一侧的实现尚未落地。对象存储路径的方法（作者签名、水位）另定。
