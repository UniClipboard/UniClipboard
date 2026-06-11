# Mobile Sync 二维码多候选地址（桌面端生成）

> 需求规格 —— 让桌面端注册移动设备时生成的 `uniclipboard://connect` 二维码，
> 一次性携带 **全部可达候选地址**（内网网卡 + 公网入口），使同一个码在内网与
> 公网下都能被扫码端探活连通。
>
> **状态**：已实现（2026-06-11，分支 `feat/mobile-sync-qr-multi-url`）。
> 实现期补充决策见 §3.1。
> **本规格范围**：桌面端二维码生成逻辑 + 前端切 host 重算路径（实现期扩入，
> 见 §3.1 决策 2）。解析端、移动客户端、部署层不在本规格内（见 §7 非目标）。
> **相邻真相**：协议总规范 `docs/architecture/mobile-sync-connect-uri.md`
> （v1）；本规格是其向后兼容的增量，落地时需同步回写该规范。

---

## 1. 背景与动机

v1 的 payload 只携带 **单个** `url`，其取值由 `register_device.rs:335` 的一段
**互斥分支** 决定：

```rust
let base_url = match settings.mobile_sync.lan_advertise_base_url.clone() {
    Some(url) => url,            // 有公网/反代地址 → 直接用它，最高优先
    None => {                    // 否则才回退 LAN：
        let ip = lan_advertise_ip.unwrap_or_else(auto_pick_advertise_ip);
        format!("http://{ip}:{port}")   // auto_pick 枚举所有网卡但只取第一个
    }
};
```

### 1.1 两种部署形态走的是不同分支

`lan_advertise_base_url` 是否存在，把现状切成两类场景——这是理解本需求的前提：

| 部署形态 | `lan_advertise_base_url` | 命中分支 | 二维码 `url` 内容 |
| --- | --- | --- | --- |
| **桌面端**（普通 GUI / `uniclip start`） | **不涉及，恒为 `None`** | 只走 `None` | 自动挑的 **单个网卡 IP** `http://<ip>:<port>` |
| **Server 端**（`uniclip start --server`，VPS 部署） | **专属，置备时写入**（`network set --url https://域名`，见 ADR-007 §2.5） | 走 `Some(url)` | **只剩那个公网域名**，无任何网卡 IP |

换言之：**`lan_advertise_base_url` 是 server 端专属的配置——桌面端的二维码里压根
不存在它，永远只有一个本机网卡 IP；只有 server 端的二维码才会带 `lan_advertise_base_url`，
且一旦带上就独占整个 `url`。** 两类场景各自踩中下面不同的坑。

### 1.2 由此带来三个问题

1. **（桌面端）多网卡赌错网段**：桌面端恒走 `None` 分支，`auto_pick_advertise_ip()`
   虽枚举了所有网卡，却 **只取排序后的第一个** 塞进 `url`；多网卡（有线 / WiFi /
   虚拟网卡）时挑中的 IP 不一定是手机能到达的网段，扫码后直接连不通。
2. **（server 端）有公网地址就丢光网卡 IP**：server 端 `Some(url)` 分支命中，
   二维码 `url` **就只剩这个域名**，所有 LAN 网卡 IP 被完全覆盖、根本不进码。手机
   回到与 server 同一局域网时反而连不上（除非内网也能解析该域名 / 反代）。
3. **内外网二选一（共同根因）**：上面两条同源——单值 `url` 是个非此即彼的选择，
   要么一个 LAN IP（只能内网用）、要么公网域名（只能外网用），**无法用一个码同时
   覆盖内网与公网两种网络位置**。本需求要把这个"二选一"改成"全候选合并"。

**本需求**：桌面端把全部候选地址（公网入口 + 所有合格网卡 IP）一并写入一个新
的 `urls` 数组字段，扫码端拿到后自行逐个探活。本规格只负责**把候选正确、稳定、
向后兼容地编码进二维码**。

---

## 2. 范围

### 2.1 本规格覆盖（桌面端生成）

- `ConnectPayload` 新增 `urls` 字段的 schema 与序列化规则。
- 桌面端候选地址的 **收集、过滤、排序、去重、截断** 逻辑。
- 主候选 `url` 与 `urls` 的关系、向后兼容约束。
- URI 长度上限调整。
- 生成侧的 golden vector 测试要求。

### 2.2 非目标（见 §7）

- 扫码端 / 移动客户端如何解析 `urls`、如何探活选路。
- 部署层 sslip.io 伪域名派生、Caddy、`UC_DOMAIN` 等编排细节。
- HTTP wire 协议（SyncClipboard 语义）—— 完全不变。

---

## 3. 决策汇总（已锁定）

| 项 | 结论 |
| --- | --- |
| 版本号 | payload `v` 与 URI envelope `v` **都保持 `1`**；`urls` 作为可忽略的附加字段 |
| 兼容性 | 老客户端 serde 自动忽略 `urls`，仅读 `url`；`url` 提取逻辑零改动 |
| `url` 主候选 | `url == urls[0]`（候选列表首项），保持 v1 语义 |
| 候选来源与顺序 | `[公网入口(若有), ...全部合格网卡 IP]` |
| 网卡口径 | 复用 `is_lan_candidate`（RFC1918 + Tailscale CGNAT `100.64.0.0/10`） |
| Tailscale | 纳入，纯 `http://<100.64.x.x>:<port>` 形式，无 MagicDNS 特殊处理 |
| Docker 网卡 | 按接口名前缀 `docker0` / `br-` / `veth` 剔除（⚠️ 见 §10 风险） |
| 端口 | 所有网卡候选共用 `settings.mobile_sync.lan_port`（缺省 `42720`） |
| 候选上限 | 去重后 **最多 20 个** |
| URI 上限 | `URI_MAX_LEN` 由 `800` 放宽到 `2000` |
| 公网入口来源 | `settings.mobile_sync.lan_advertise_base_url`（已含部署层派生值） |

### 3.1 实现期补充决策（2026-06-11 拍板）

| 项 | 结论 |
| --- | --- |
| 钉死 IP（`lan_advertise_ip=Some`） | 原规格 §5.1 漏列。排在 **公网入口之后、其余网卡之前** —— 无公网入口时 `urls[0]` = 钉死 IP，与 v1 `url` 取值完全一致 |
| TS 生成侧 | **同步纳入范围**。`src/lib/mobileSyncConnectUri.ts` 的 `buildConnectUri` 是生产路径（凭据弹窗切 host 实时重算 QR），不改则切 host 后码退化为单地址。前端重算时把所选 host 提升为 `urls[0]`，其余候选保序跟随 |
| 单候选时 `urls` | 编码器 **省略整个字段**（而非 `["唯一候选"]`）—— 最常见单网卡场景与 v1 字节零漂移，由 build 层结构性保证 |
| Docker `br-*` 收敛 | `docker0` / `veth*` 按名直接剔除；`br-*` 仅当地址落在 `172.16/12` 段内才剔除（§10 风险的收敛实现），PR review 确认名单 |
| 探测失败降级 | 已有公网入口/钉死 IP 候选时，网卡探测失败仅 `warn` 并继续（v1 在这两条路径不探测网卡，不能让探测失败弄死老路径）；无任何候选时照旧报错 |

---

## 4. Payload schema 变更

### 4.1 形态

base64url 解码后的 JSON（字段顺序固定为 `v, url, urls, user, pwd, o`）：

```json
{
  "v": 1,
  "url": "https://203-0-113-10.sslip.io",
  "urls": [
    "https://203-0-113-10.sslip.io",
    "http://192.168.1.5:42720",
    "http://100.64.0.5:42720"
  ],
  "user": "mobile_aabbccdd",
  "pwd": "AbCdEfGhIjKlMnOpQrSt",
  "o": { "did": "did_0123abcd", "label": "My iPhone", "proto": "syncclipboard" }
}
```

### 4.2 字段规则

| 字段 | 类型 | 规则 |
| --- | --- | --- |
| `v` | integer | 仍为 `1`，**不 bump** |
| `url` | string | **不变**。等于 `urls[0]`。老客户端只读它 |
| `urls` | string[] | **新增**。有序候选列表，每项是完整 base URL（无尾斜杠）。`#[serde(default, skip_serializing_if = "Vec::is_empty")]` |

### 4.3 向后兼容（硬约束）

- `ConnectPayload` **不得** 加 `#[serde(deny_unknown_fields)]` —— 老客户端正是
  靠"忽略未知字段"来无视 `urls`。
- `urls` 为空时 **不序列化**。意味着「没有候选增强」的旧式码与 v1 **字节完全一致**，
  现有 v1 golden vector（`mobile-sync-connect-uri.md` §7.1）必须保持不变。
- bump 版本号是 **被明确否决** 的方案：会让老客户端在读 `url` 之前就
  `UNSUPPORTED_VERSION` 拒掉整个码，破坏兼容。

### 4.4 编码规则（沿用 v1 §3.3）

- JSON 必须 UTF-8、minify、无尾换行。
- 顶层字段序固定 `v, url, urls, user, pwd, o`；`o` 内键字典序。
- 三者合起来保证 Rust / TS 跨语言 **字节级** 一致。

---

## 5. 候选地址生成逻辑

桌面端在 `register_device.rs` 内新增收集逻辑（建议 `collect_advertise_urls`），
产出有序、去重、截断后的 `Vec<String>` 作为 `urls`，并令 `url = urls[0]`。

### 5.1 收集顺序

```text
urls 候选（按优先级拼接）：
  1. 公网入口        ── settings.mobile_sync.lan_advertise_base_url（Some 时，原样，1 项）
  2. 全部合格网卡 IP ── 对每个合格 LanInterface：http://<ipv4>:<lan_port>
```

- 第 1 项的存在与否，沿用 v1 既有 base_url 决策：`lan_advertise_base_url=Some`
  即公网入口（可能是部署层派生的 `https://<ip>.sslip.io`，对本规格透明）。
- `lan_advertise_base_url=None` 时第 1 项缺省，`urls` 仅由网卡 IP 组成 —— 纯内网
  场景退化正确。

### 5.2 网卡过滤口径

复用 `list_lan_interfaces.rs` 的 `is_lan_candidate` 判定（不是 `auto_pick` 那个
只认 RFC1918 的口径）：

- **接受**：RFC1918（`10/8`、`172.16/12`、`192.168/16`）+ Tailscale CGNAT
  `100.64.0.0/10`。
- **剔除**：loopback、链路本地 `169.254/16`、Clash fake-ip `198.18/15`。
- **额外剔除（本规格新增）**：接口名前缀命中 `docker0` / `br-` / `veth` 的虚拟
  网卡 —— Docker 默认网桥 `172.17.0.0/16` 落在 RFC1918 段内，仅按 IP 段过滤会被
  误纳入。`LanInterface.name` 字段可用于此判定。

### 5.3 排序

网卡部分沿用既有桶序：`10/8 → 172.16/12 → 192.168/16 → 100.64/10`，段内按
IPv4 数值序。公网入口恒定排在所有网卡之前（`urls[0]`）。

### 5.4 去重与截断

- 按最终字符串去重（公网入口若恰好等于某网卡 URL，只保留一份且保留靠前位置）。
- 去重后 **截断到 20 项**。被截断时记一条 `tracing` 说明丢弃数量（不得静默截断）。

### 5.5 端口

所有网卡候选统一使用 `settings.mobile_sync.lan_port`，缺省 `42720`。公网入口项
自带其完整 URL（含其自身端口或经反代的 443），不附加 `lan_port`。

---

## 6. URI 长度上限

- `URI_MAX_LEN`：`800` → `2000`。
- 理由：20 个 `http://<ipv4>:<port>` 候选 base64 编码后约 1100+ 字符，会超旧
  上限。`2000` 给足余量。
- 副作用：候选多时 QR version 升高（约 ~27）、密度变大，但常见家用网卡极少
  超过 4 个，`20` 仅为上限保护。`build` 超限仍返回 `UriTooLong`。

---

## 7. 非目标（后续独立推进）

| 范畴 | 说明 |
| --- | --- |
| 解析端 | `parse_mobile_sync_connect_uri` 读取 `urls`、扫码端探活选路逻辑 —— **不在本规格**。本规格只保证生成端写出合法、可被忽略的 `urls` |
| 移动客户端 | iOS / Android / Shortcut 如何消费 `urls`、逐个 `GET /SyncClipboard.json` 探活 —— 后续 |
| 部署层 | `UC_DOMAIN` 留空时派生 `<dashed-ip>.sslip.io`、Caddy 签证书、`network set --url` 置备 —— 后续；本规格只 **消费** `lan_advertise_base_url` 的最终值 |
| HTTP wire | SyncClipboard 协议、Basic Auth —— 完全不变 |

---

## 8. 涉及改动点（桌面端）

| 关注点 | 文件 | 改动 |
| --- | --- | --- |
| 编码 | `src-tauri/crates/uc-application/src/usecases/mobile_sync/connect_uri.rs` | `ConnectPayload` 加 `urls`；`build_mobile_sync_connect_uri` 接收候选列表；`URI_MAX_LEN → 2000`；新增 golden vector |
| 候选生成 | `src-tauri/crates/uc-application/src/usecases/mobile_sync/register_device.rs` | 新增 `collect_advertise_urls`（来源 / 过滤 / 排序 / 去重 / cap20 / Docker 前缀剔除）；`url = urls[0]` |
| 规范回写 | `docs/architecture/mobile-sync-connect-uri.md` | 新增 `urls` 字段说明、候选生成口径、新增 golden vector；声明向后兼容 |

---

## 9. 测试要求

- **旧 golden vector 不变**：不带 `urls` 的输入必须仍编码出 v1 §7.1 的完全相同
  字符串（验证 `skip_serializing_if` 生效、字节零漂移）。
- **新 golden vector**：含 `urls` 的输入产出一条新的、被 §7 记录的稳定字符串，
  Rust 端 round-trip 断言。
- **候选生成单测**（port mock 驱动 `LanInterfaceProbePort`）：
  - 多网卡 → `urls` 含全部、按桶序、`url == urls[0]`。
  - Docker 网卡（`docker0` / `br-xxxx` 名 + `172.17.x.x`）被剔除。
  - Tailscale `100.64.x.x` 被纳入、排在 RFC1918 之后。
  - 有 `lan_advertise_base_url` → 公网入口排首位。
  - 无任何合格网卡且无公网入口 → 沿用 v1 `NoLanInterfaceAvailable`。
  - 超过 20 个候选 → 截断到 20 且记 tracing。
- **超长**：构造 >2000 字符 → `UriTooLong { max: 2000 }`。

---

## 10. 待决 / 风险

- **Docker 前缀名单的误伤**：真实 LAN 网桥也可能命名为 `br-*`。实现时需收敛
  判定（例如仅剔除 `docker0` 与 `veth*`，对 `br-` 更谨慎，或结合 `172.17/16`
  段联合判定），并在 PR review 中确认名单。
- **公网入口可达性**：本规格不探测公网入口是否真的可达，只负责编码；探活由扫码端
  负责（非目标）。
