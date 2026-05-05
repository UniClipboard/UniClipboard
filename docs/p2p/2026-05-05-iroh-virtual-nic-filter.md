# iroh 虚拟网卡地址过滤设计与实现

> 落地时间：2026-05-05
> 涉及版本：iroh 0.98.2、uniclipboard `0.6.x` 起
> Issue 关联：UniClipboard#486
> 主要 commits：`50477a17` / `800a508a` / `ae39f432`

## 1. 背景与动机

UniClipboard 通过 iroh 建立设备之间的直连。iroh 在协商 NAT
穿透时会把本机所有可达的网络地址（"direct addresses"）发布到
pkarr/mDNS/DHT，让对端的 magicsock 拿到候选列表后逐个尝试连接。

问题在于：现代用户机器上常有**虚拟网卡**，它们的 IP 看起来像普通
LAN 地址，但跨主机不可达：

| 网段 | 来源 | 跨主机可达性 |
|---|---|---|
| `198.18.0.0/15` | Clash 默认 fake-ip 池 | 不可达（本机回环） |
| `100.64.0.0/10` | CGNAT / Tailscale 默认 IPv4 段 | 仅在同一 tailnet 内可达 |
| `fd7a:115c:a1e0::/48` | Tailscale IPv6 ULA | 仅在同一 tailnet 内可达 |
| `169.254.0.0/16` | IPv4 link-local autoconf | 仅本机有意义 |

如果这些地址被作为直连候选发布出去：

1. **死候选阻塞**：peer 的 magicsock 会逐个尝试 path validation。
   每条死路径都要消耗 PathId 预算（默认 13；项目里因为 multipath
   并发已经把上限提到 64，见 `uc-infra/src/network/iroh/node.rs`
   的 `build_transport_config`），并拉长 RTT 探测窗口。
2. **假赢风险**：本机 TUN 接口的 ACK 比真实 LAN 快，path-race 中
   虚拟路径可能临时胜出，但实际报文回不来；最终连接挂死或被踢回
   relay。Issue #486 在 Clash fake-ip 案例上观察到这一现象。

`uc-observability/src/profile.rs` 也已记录到部分用户出现 `EHOSTUNREACH`
错误，源头正是 VPN/Clash TUN 接口被 iroh 当作直连候选。

## 2. 设计反复讨论的关键决策

### 2.1 不是所有"虚拟 IP"都该一刀切过滤

设计上把这四段拆成两类：

| 类别 | 网段 | 处理 |
|---|---|---|
| **always-filtered** | `198.18.0.0/15`、`169.254.0.0/16` | 永远过滤，无 escape hatch |
| **overlay 类** | `100.64.0.0/10`、`fd7a:115c:a1e0::/48` | 默认过滤，专业用户可 opt-in 放行 |

**为什么不让用户也能关掉 Clash fake-ip 过滤？**
198.18.x 没有任何合法跨主机用例 —— 暴露给用户只会增加误用面，没有收益。

**为什么 Tailscale 段需要 opt-in？**
如果两台设备**都在同一个 tailnet** 中，且真实 LAN/公网直连不通，那
Tailscale 100.x / fd7a:: 是合法可达路径，过滤反而让用户损失一条
路径。这是少数派但真实场景。

### 2.2 反向命名 vs 正向命名

- `allow_relay_fallback`（已存在）业务正向语义，但 UI 文案是 "LAN-only Mode"，
  导致 UI checked === ON === LAN-only === `allow_relay_fallback = false`。
  存在唯一一处取反点，由 `uc-bootstrap/src/network_policy.rs` 集中收口。
- `allow_overlay_network_addrs`（本次新增）**正向同名传递，不取反**：
  UI checked === `allow_overlay_network_addrs`。不参与反向命名铁律。

这避免了两个语义反转字段叠加在一起带来的认知负担。

### 2.3 默认值 = `false`（保持现行行为）

Issue #486 已经在生产用户的 telemetry 里观察到死候选问题，过滤是
**已被验证有效的修复**。新字段的默认值与既有 v0.6.x 行为完全一致，
属于纯粹的"专业用户调优"开关，不影响多数派。

### 2.4 配置粒度 = 单一布尔

候选过的方案：

- ❌ **整体开关**（`filter_virtual_nics`）：误把 Clash 与 Tailscale
  绑在一起，开了之后用户不知不觉踩到 Clash 假赢。
- ❌ **三档枚举**（`Strict | Permissive | Off`）：心智负担过高，
  且 Permissive 与 Strict 之间的语义边界没有真实需求驱动。
- ❌ **CIDR 黑名单**（用户填段）：要求懂 CIDR；过早抽象。
- ✅ **单一布尔，仅控制 overlay 类**：简单、语义直接、未来扩展段
  时不需要改字段名。

### 2.5 修改后必须重启

iroh 的 `Endpoint::builder().addr_filter(...)` 在 `bind` 时被冻结
为 endpoint 常量，没有运行时可改的接口。叠加项目里 `BIND_LOCK`
（`uc-infra/src/network/iroh/node.rs:409` 附近）的"进程级单次 bind"
约束，运行时热切换被显式排除（Pitfall 3 防御）。

UI 上沿用既有 `RestartBanner` 组件——任何 `NetworkSettings` 字段
变更都共享同一个重启提示，因为它们的根因相同。

## 3. 整体架构与改动范围

```
                 ┌─────────────────────────────────────────────┐
                 │ Settings JSON (settings.json)               │
                 │  network: {                                 │
                 │    allow_relay_fallback: bool               │
                 │    allow_overlay_network_addrs: bool  ← 新  │
                 │  }                                          │
                 └────────────────┬────────────────────────────┘
                                  │  serde
                 ┌────────────────▼────────────────────────────┐
                 │ uc-core::settings::model::NetworkSettings   │
                 └────────────────┬────────────────────────────┘
                                  │  From<core>
            ┌─────────────────────┼─────────────────────┐
            ▼                     ▼                     ▼
    ┌───────────────┐    ┌─────────────────┐   ┌──────────────────┐
    │ application   │    │ daemon-contract │   │ bootstrap        │
    │ View / Patch  │    │ DTO (camelCase) │   │ network_policy   │
    └───────┬───────┘    └────────┬────────┘   └────────┬─────────┘
            │                     │                     │
            ▼                     ▼                     ▼
    ┌───────────────┐    ┌─────────────────┐   ┌──────────────────┐
    │ webserver     │    │ Frontend        │   │ uc-infra         │
    │ PUT /settings │    │ NetworkSection  │   │ IrohNodeConfig   │
    │ + smoke test  │    │ + Disclosure    │   │ + AddrFilter     │
    └───────────────┘    └─────────────────┘   └──────────────────┘
```

涉及的 crate：`uc-core`、`uc-application`、`uc-daemon-contract`、
`uc-bootstrap`、`uc-infra`、`uc-webserver`、前端。

## 4. 各层实现

### 4.1 `uc-core`：领域字段

`uc-core/src/settings/model.rs`：

```/Users/mark/conductor/workspaces/uniclipboard/edinburgh/src-tauri/crates/uc-core/src/settings/model.rs#L189-219
pub struct NetworkSettings {
    #[serde(default = "default_allow_relay_fallback")]
    pub allow_relay_fallback: bool,

    /// 是否允许把 VPN / overlay 类虚拟网卡地址（CGNAT 100.64.0.0/10、
    /// Tailscale ULA fd7a:115c:a1e0::/48）作为 iroh 直连候选。
    /// 默认 false（过滤）。
    #[serde(default = "default_allow_overlay_network_addrs")]
    pub allow_overlay_network_addrs: bool,
}

fn default_allow_overlay_network_addrs() -> bool {
    false
}
```

要点：

- `#[serde(default = ...)]` 让旧 `settings.json`（没这个字段）反序列化
  时自动回填 `false`，向后兼容。
- `Default` impl 在 `uc-core/src/settings/defaults.rs`，与 serde 默认
  函数保持一致（同一份业务规则不允许两处不一致）。

测试覆盖：默认值、旧 JSON 反序列化、显式 true/false JSON 反序列化
（见 `defaults.rs` 的 `network_settings_default_filters_overlay_addrs`
等用例）。

### 4.2 `uc-application`：View / Patch

`uc-application/src/facade/settings/models.rs` 加镜像字段：

- `NetworkSettingsView`：读路径用，包含完整字段
- `NetworkSettingsPatch`：写路径用，每字段是 `Option<bool>`，
  `None` = 不修改

`apply_settings_patch` 中显式判断 `Some(v)` 才写入，保证 patch 层面
的"缺字段不抹掉"语义。

测试覆盖：

- patch 缺字段保留已存在值
- patch 显式 Some(true) / Some(false) 双向覆盖
- View 透明搬运业务正向语义（不取反）

### 4.3 `uc-daemon-contract`：wire DTO

`uc-daemon-contract/src/api/dto/settings.rs` 中 `NetworkSettingsDto`
和 `NetworkSettingsPatchDto` 加 `allow_overlay_network_addrs` 字段。
wire 字段名经 `#[serde(rename_all = "camelCase")]` 自动转为
`allowOverlayNetworkAddrs`。

新增字段的 wire 设计有一个细节：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettingsDto {
    pub allow_relay_fallback: bool,
    #[serde(default)]
    pub allow_overlay_network_addrs: bool,  // 旧 wire 兼容
}
```

`#[serde(default)]` 保证旧前端发来的 wire（不含 `allowOverlayNetworkAddrs`）
仍然可以反序列化，回填 `false`。这一兼容性由
`dto_deserializes_legacy_wire_without_overlay_field` 测试钉死。

### 4.4 `uc-bootstrap`：翻译层

`uc-bootstrap/src/network_policy.rs` 是项目里**唯一允许进行业务语义
↔ infra 语义反转**的地方（Pitfall 1 铁律）。

```/Users/mark/conductor/workspaces/uniclipboard/edinburgh/src-tauri/crates/uc-bootstrap/src/network_policy.rs#L37-50
pub(crate) fn relay_policy_to_iroh_config(
    allow_relay_fallback: bool,
    allow_overlay_network_addrs: bool,
    rendezvous_base_url: Option<String>,
) -> IrohNodeConfig {
    IrohNodeConfig {
        // ↓ 全工程**唯一**取反点 — Pitfall 1 防御铁律。
        disable_relays: !allow_relay_fallback,
        // ↓ 正向同名字段，直接搬运不取反。
        allow_overlay_network_addrs,
        rendezvous_base_url,
    }
}
```

调用点（`builders.rs` 与 `non_gui_runtime.rs`）从 `SettingsPort` 加载
后透传到 `IrohNodeConfig`，并在 `tracing::info!(target: "settings.network", ...)`
中暴露字段值供排障。

### 4.5 `uc-infra`：iroh 接入与过滤实现

`uc-infra/src/network/iroh/node.rs` 是核心实现所在。

#### 4.5.1 `IrohNodeConfig` 加字段

```rust
pub struct IrohNodeConfig {
    pub rendezvous_base_url: Option<String>,
    pub disable_relays: bool,
    pub allow_overlay_network_addrs: bool,
}
```

#### 4.5.2 `is_virtual_nic_ip` 双类拆分

```/Users/mark/conductor/workspaces/uniclipboard/edinburgh/src-tauri/crates/uc-infra/src/network/iroh/node.rs#L302-336
fn is_virtual_nic_ip(ip: IpAddr, allow_overlay: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            // Always-filtered classes:
            if (o[0] == 198 && (o[1] & 0xfe) == 18)        // 198.18.0.0/15
                || (o[0] == 169 && o[1] == 254)             // 169.254.0.0/16
            {
                return true;
            }
            // Overlay class (CGNAT / Tailscale 100.64.0.0/10):
            if !allow_overlay && o[0] == 100 && (o[1] & 0xc0) == 64 {
                return true;
            }
            false
        }
        IpAddr::V6(v6) => {
            // Tailscale IPv6 ULA fd7a:115c:a1e0::/48
            let segs = v6.segments();
            if !allow_overlay && segs[0] == 0xfd7a
                && segs[1] == 0x115c && segs[2] == 0xa1e0
            {
                return true;
            }
            false
        }
    }
}
```

要点：

- IPv4 段用位运算判 CIDR：`(o[1] & 0xfe) == 18` 等价于
  `o[1] in [18, 19]`（覆盖 `198.18.x.x` + `198.19.x.x` 即 /15）。
  `(o[1] & 0xc0) == 64` 等价于 `o[1] in [64, 65, ..., 127]`，
  覆盖 100.64.0.0/10。
- IPv6 ULA 用前 48 位（前 3 个 16-bit segment）判定。
- **本次同步补上历史漏过的 Tailscale IPv6 ULA `fd7a:115c:a1e0::/48`**。
  原版只过滤 IPv4，导致 IPv6-preferred 网络下行为不对称。

#### 4.5.3 `apply_addr_filter` 抽出可测的纯函数

`AddrFilter` 是 iroh 暴露的不透明 wrapper，外部不能直接 invoke
其内部闭包做单测。所以把过滤逻辑抽成：

```rust
fn apply_addr_filter<'a>(
    addrs: &'a Vec<TransportAddr>,
    allow_overlay: bool,
) -> Cow<'a, Vec<TransportAddr>> {
    // 任一虚拟 IP 都不存在 → 直接返回 Borrowed（零拷贝快路径）
    let any_virtual = addrs.iter().any(|a| match a {
        TransportAddr::Ip(s) => is_virtual_nic_ip(s.ip(), allow_overlay),
        _ => false,
    });
    if !any_virtual {
        return Cow::Borrowed(addrs);
    }
    // 否则构造过滤后的 Owned 集合 + 打 debug 日志
    let kept: Vec<TransportAddr> = ...
    let dropped: Vec<String> = ...
    debug!(
        target: "iroh.addr_filter",
        allow_overlay,
        dropped_count = dropped.len(),
        dropped = ?dropped,
        "filtered virtual-NIC addresses from candidate set",
    );
    Cow::Owned(kept)
}

fn build_addr_filter(allow_overlay: bool) -> AddrFilter {
    AddrFilter::new(move |addrs: &Vec<TransportAddr>|
        apply_addr_filter(addrs, allow_overlay)
    )
}
```

`Cow` 的设计避免了"无虚拟 IP"快路径的不必要拷贝（最常见情况）。

#### 4.5.4 `bind` 时的可观测日志

```rust
let allow_overlay = config.allow_overlay_network_addrs;
info!(
    target: "iroh.addr_filter",
    allow_overlay,
    "addr filter configured: overlay-network addresses {} ...",
    if allow_overlay { "ALLOWED" } else { "BLOCKED" },
);
let endpoint = Endpoint::builder(presets::N0)
    ...
    .addr_filter(build_addr_filter(allow_overlay))
    ...;
```

每次 daemon 启动留一行 INFO，便于 support 反查"用户当前是不是把
开关打开了"。

#### 4.5.5 测试覆盖

`apply_addr_filter` 与 `is_virtual_nic_ip` 各 4 段地址 × 2 状态
共 7 个单测用例，覆盖：

- 始终过滤类（Clash / link-local）忽略 overlay flag
- CGNAT v4 / Tailscale v6 ULA 跟随 overlay flag
- 真实 LAN / 公网 / 边界值（如 `100.63.255.255` 与 `100.128.0.0`
  分别在 `100.64.0.0/10` 之外）从不被过滤
- `apply_addr_filter` 对完整候选集的端到端行为

### 4.6 前端：UI、i18n、Disclosure

#### 4.6.1 UI 组件

`src/components/setting/NetworkSection.tsx` 是 Phase 95 已有的网络
设置组件，本次新增第二个 SettingRow：

- 复用既有 `RestartBanner` 组件（任一开关切换后共享 banner，因为
  根因都是"需要重启 daemon"）
- 复用 `useDebounce(value, 500)` 模式：本地乐观切换 → 500ms
  debounce 后 PUT；多次连击合并为一次请求
- 失败时回滚到 persisted 值 + 显示 inline `saveError`
- 给两个 Switch 都加 `aria-label`，便于 testing-library 在 DOM
  里区分 / 屏幕阅读器友好

新增独立的 `AllowOverlayAddrsDisclosure.tsx`，与 `LanOnlyDisclosure`
同模式（click-only Popover），但内容针对 overlay 语义，4 段说明：

- 影响范围（哪些段、哪些不在范围）
- 默认关闭原因（死候选阻塞）
- 何时开启（同 tailnet + 真实 LAN 不通）
- 权衡（开错了只是连接慢，不会断 — relay 仍然兜底）

#### 4.6.2 类型同步

前端有两处 `NetworkSettings` 类型：

- `src/api/daemon/settings.ts` — wire 边界类型（与 daemon HTTP API 对齐）
- `src/types/setting.ts` — 应用内类型（与 `uc-core::Settings` 对齐）

两处都加 `allowOverlayNetworkAddrs: boolean` 字段，由人工 cross-review
保持同步（项目无 ts-rs / bindgen 自动生成）。

#### 4.6.3 i18n 文案

`src/i18n/locales/en-US.json` / `zh-CN.json` 在
`settings.sections.network.allowOverlayAddrs` 下新增完整 key 树：

```
allowOverlayAddrs:
  label
  description
  infoIconAriaLabel
  saveError
  disclosure:
    title
    intro
    covered: { title, description }
    whyOff:  { title, description }
    whenOn:  { title, description }
    tradeoff:{ title, description }
```

#### 4.6.4 toSettingsPatchRequest 的 spread 改造

`src/api/daemon/settings.ts` 中 patch 构造改为 spread：

```typescript
if (settings.network) {
  patch.network = { ...settings.network }
}
```

理由：测试和 `SettingContext.updateNetworkSetting` 经常传
`Partial<NetworkSettings>`，如果在 patch 里显式列字段会输出
`undefined` 值，违背 wire patch "缺字段=不修改" 的语义。spread
天然只镜像 input 中实际存在的字段。

## 5. iroh AddrFilter 工作机制（基于源码）

这是本次实现的关键技术点，查清后才敢宣称"过滤生效"。

### 5.1 调用位置

iroh `0.98.2` 的 `Endpoint::builder().addr_filter(filter)` 把
`AddrFilter` 存到 endpoint 内部，bind 时传给
`AddressLookupServices`：

```/Users/mark/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/iroh-0.98.2/src/endpoint.rs#L277-278
if let Some(filter) = self.addr_filter {
    address_lookup.set_addr_filter(filter);
}
```

### 5.2 publish 路径中的过滤

`AddressLookupServices::publish()` 是过滤真正生效的地方：

```/Users/mark/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/iroh-0.98.2/src/address_lookup.rs#L549-563
pub(crate) fn publish(&self, data: &EndpointData) {
    let data = match &*self.addr_filter.read().expect("poisoned") {
        Some(filter) => data.apply_filter(filter),  // ← 过滤
        None => Cow::Borrowed(data),
    };
    let services = self.services.read().expect("poisoned");
    for service in &*services {
        service.publish(&data);  // ← 过滤后的 data 才送给 pkarr / mDNS / DHT
    }
    ...
}
```

**因此**：

- 我们注册的 `AddrFilter` 在每次 publish endpoint addressing
  数据给下游 lookup service 之前都会运行一次。
- pkarr 公网 DHT、mDNS LAN 广播、可能的 DNS publisher 都拿到的是
  **过滤后的子集**。
- peer 通过任何 lookup 渠道查询本机时，得到的候选地址列表里
  **不会包含**我们丢弃的虚拟网卡 IP。

### 5.3 publish snapshot ≠ publish 给 peer 的内容

代码里的 `log_publish_addrs()` 函数打 INFO 日志：

```rust
let addr = endpoint.addr();
let ip_addrs: Vec<String> = addr.addrs.iter().filter_map(...).collect();
info!(
    stage,
    ip_addrs = ?ip_addrs,
    "iroh endpoint publish snapshot (refs UniClipboard#486)"
);
```

`endpoint.addr()` 返回的是 **magicsock 内部知道的本机所有 socket 地址**
（由 OS 接口枚举得出），是 `AddressLookupServices::publish()` **之前**
的 raw 集合。它会包含所有虚拟网卡 IP，**这是预期的、不参与过滤**。

排障时不要把这个 snapshot 当成"对端能看到的列表"。**真正决定对端
看到什么的，是 `apply_addr_filter` 的 dropped 日志**（5.5 节）。

### 5.4 lookup 路径中的过滤

`AddrFilter` 同样作用于 lookup 返回结果：本机通过 pkarr/mDNS/DHT
查询某个对端 NodeId 时，返回的候选地址列表也过这个 filter。即使
对端发来的列表里包含 `100.x`（比如对端用的是旧版本没装这个
filter），本机的 magicsock **也不会去拨**这些地址。

这就是为什么"两边都升级"虽然是最干净的，但单边升级也仍然有
保护效果——本机至少不会被坏候选拖累。

### 5.5 dropped 日志解读

每次 filter 把虚拟 IP 从输入候选集中剔除，都打一条 DEBUG 日志：

```
target=iroh.addr_filter
message="filtered virtual-NIC addresses from candidate set"
allow_overlay=<bool>
dropped_count=<N>
dropped=["100.79.191.42:60781", "198.18.0.1:60781"]
```

注意：`dropped` 显示的是**那次 filter 调用的输入候选集中被丢掉的
IPs**。iroh 的 publish 是**事件驱动**的，magicsock 每发现新地址
就触发一次 `AddressLookupServices::publish()`，每次的输入
`EndpointData` 不一定包含全部本机地址。

所以排障时看到 `dropped=["198.18.0.1:..."]` 只丢了 1 个不要慌——
那次 publish 可能就只携带了 198.18，100.x 是另一次 publish 的
事件。把同一个 endpoint port（如 `60781`）的所有 dropped 日志
聚合起来看才是完整图景。

## 6. 配置项使用

### 6.1 用户视角（UI）

进入 `Settings → 网络`：

- **LAN-only Mode**（既有）— 默认 OFF。打开后禁用 iroh relay 回落，
  仅同局域网直连。
- **允许虚拟网络地址**（本次新增）— 默认 OFF。开启后允许 Tailscale 等
  overlay 网络地址作为直连候选。

每个开关右侧的 ⓘ 图标点开有详细说明。开关切换后会显示重启 banner，
点"立即重启"或下次启动时生效。

### 6.2 文件视角（settings.json）

dev profile 路径（macOS）：

```
~/Library/Application Support/app.uniclipboard.desktop-dev/settings.json
```

production 路径：

```
~/Library/Application Support/uniclipboard/settings.json
```

字段在 `network` 段下：

```json
{
  "network": {
    "allow_relay_fallback": true,
    "allow_overlay_network_addrs": false
  }
}
```

直接编辑文件后**必须重启 daemon** 才能生效。

### 6.3 何时建议开启 `allow_overlay_network_addrs`

仅在以下条件**同时**满足时考虑开启：

1. 两台设备**都**安装了 Tailscale（或其他相同的 overlay 网络）
   并加入同一个 tailnet
2. 真实 LAN / 公网 NAT 穿透**不通**（连接持续走 relay 或失败）
3. 在 Tailscale 客户端里能 `ping` / `ssh` 通对端，证明 overlay 路径
   本身可用

不满足任一条件，开启后只会让连接变慢（多消耗 path-validation
预算去试不通的 100.x），不会带来任何收益。**默认保持关闭**就好。

## 7. 日志与可观测性

### 7.1 启动时

每次 daemon 启动留两行 INFO：

```
target=settings.network
message="applying network settings: allow_relay_fallback=... → disable_relays=..., allow_overlay_network_addrs=..."

target=iroh.addr_filter
message="addr filter configured: overlay-network addresses {ALLOWED|BLOCKED} (Tailscale 100.64/10 + fd7a:115c:a1e0::/48)"
```

第一行来自 `bootstrap`（settings 加载点），第二行来自 `uc-infra`
（`bind` 时）。两行都包含 `device_id`，便于跨实例排障。

### 7.2 运行时

publish 事件触发的 filter 执行：

```
DEBUG target=iroh.addr_filter
message="filtered virtual-NIC addresses from candidate set"
allow_overlay=<bool>
dropped_count=<N>
dropped=[<list of "ip:port">]
```

只有当输入候选集**确实**包含被过滤段的 IP 时才打印（`apply_addr_filter`
对干净候选走 `Cow::Borrowed` 快路径不打日志）。

### 7.3 排障 grep 命令

```bash
LOG="$HOME/Library/Application Support/app.uniclipboard.desktop-dev/logs/uniclipboard.json.<DATE>"

# 当前开关状态
grep '"target":"settings.network"' "$LOG" | tail -3
grep "addr filter configured" "$LOG" | tail -3

# 过滤行为
grep "filtered virtual-NIC" "$LOG" | jq -r '.dropped' | sort | uniq -c

# magicsock 内部 raw 全集（参考用，不等于发给 peer 的内容）
grep "iroh endpoint publish snapshot" "$LOG" | tail -3
```

## 8. 真机验证流程

按"成本 / 价值"分三级，建议至少跑 Level 1。

### 8.1 Level 1 — 基础回归（不需要 Tailscale）

1. **启动 daemon**：`pnpm tauri:dev` 或 `tauri:dev:peerA`
2. **看启动日志**：grep `target=settings.network` 与
   `addr filter configured`，确认 `allow_overlay_network_addrs=false`
   + `BLOCKED`。
3. **UI 验证**：
   - 进 Settings → 网络，看到两个开关
   - ⓘ 图标点开有 disclosure popover
   - 切换新开关 → RestartBanner 立即出现
   - 关闭设置面板再打开 → banner 消失（in-memory pending 不跨 session）
4. **持久化**：`cat .../settings.json | jq .network` 确认字段值。
5. **重启后**：再 grep `addr filter configured`，确认状态从 `BLOCKED`
   翻到 `ALLOWED`。
6. **dropped 日志**（如果本机装了 Clash 或 Tailscale）：
   `grep "filtered virtual-NIC" "$LOG"` 看是否有过滤事件。

### 8.2 Level 2 — Tailscale 真过滤（需要 tailnet）

1. **关闭开关 + 重启**，确认 BLOCKED 状态下：
   - `dropped` 日志中包含本机的 `100.x:port`（Tailscale IPv4）
   - 如果本机有 Tailscale IPv6 ULA，也应被丢弃
2. **打开开关 + 重启**，确认 ALLOWED 状态下：
   - `dropped` 日志中**不再**包含 `100.x`
   - 仍然丢弃 `198.18.x`、`169.254.x`（始终过滤类）

### 8.3 Level 3 — 跨设备连通性（需要两台设备）

如果两端都装了 UniClipboard：

1. 默认配置下复制粘贴正常 → 默认行为没坏
2. 两端都打开新开关 → 复制粘贴仍然正常 → overlay 路径不会比 LAN/relay 差
3. （可选）造一个对称 NAT + Tailscale 通的环境，验 OFF/ON 的连接成功率
   差异

## 9. Pitfall 防御与铁律

### 9.1 反向命名铁律（Pitfall 1）

- `allow_relay_fallback` 业务正向 ↔ infra `disable_relays` 反向 → 唯一
  取反点位于 `uc-bootstrap/src/network_policy.rs`
- `allow_overlay_network_addrs` 全链路正向同名传递 → 不参与铁律
- 任何在 DTO / View / 前端 store 维护反向布尔镜像字段都视为回归
- 既有审计测试在 `src/api/daemon/__tests__/settings.test.ts` 的
  `反向命名审计 (Pitfall 1 fence)` describe 块里钉死

### 9.2 BIND_LOCK 进程级单次（Pitfall 3）

- `IrohNodeBuilder::bind` 在 production 路径下用
  `OnceLock` 守护，第二次调用 panic
- 含义：运行时热切换 `allow_overlay_network_addrs` 不可能，必须
  重启 daemon
- 这与 UI 的 RestartBanner 设计是同一根因

### 9.3 IPv6 ULA 一致性

- 历史欠账：原本只过滤 IPv4 100.64/10，IPv6 ULA `fd7a:115c:a1e0::/48`
  漏过
- 本次修补：开关同时控制两段；测试覆盖两段对称
- 任何后续加新 overlay 段的 PR 必须 v4/v6 同时实现，不允许只实现一边

### 9.4 occasional dropped_count=1 不是 bug

见 5.5。不要看到一条只丢 1 个就以为过滤漏了——iroh publish 是事件
驱动的增量更新。

## 10. 已知边界与未来扩展

### 10.1 不在本期范围

- **运行时热切换**：必须重启 daemon。`BIND_LOCK` 是结构性约束，
  解决需要独立 phase（"endpoint 重建" 工程量与风险都更大）
- **UI 中"过滤动作可见性"**：当前只有日志，没有给用户面板上
  显示"今天过滤了 N 个虚拟 IP"。如有 telemetry 信号显示用户需要，
  可以加
- **细粒度网段开关**：不拆 Clash / Tailscale / 其他独立开关。
  YAGNI，单一布尔够用
- **CIDR 用户自定义黑名单**：同上

### 10.2 未来可能扩展

- 加新的虚拟网卡段（如 ZeroTier、Hyper-V 默认段）：
  - 始终过滤类直接加进 `is_virtual_nic_ip` 的 always-filter 块
  - overlay 类（条件可达）加进 `if !allow_overlay { ... }` 块
- 给 `apply_addr_filter` 加 metric counter（dropped 总数、按段分类
  count），导到 OTLP，便于宏观观察用户群中的虚拟网卡分布
- 双协议栈对称增强：如果未来 IPv6 链路 prefer 度更高，把 IPv4
  link-local 的 IPv6 等价（fe80::/10 link-local）也加进 always-filter

## 11. 参考资料

- Issue：UniClipboard#486
- iroh `AddrFilter` 文档：`iroh-0.98.2/src/address_lookup.rs` 顶部
  module doc
- iroh PR：iroh#3960、iroh#4010（`AddrFilter` 引入与 publish 路径过滤）
- 相关 phase 工件：`.planning/phases/094-backend-network-allow-relay-fallback/`、
  `.planning/phases/095-networksection-ux/`
- 项目分层规范：`src-tauri/crates/uc-core/AGENTS.md`、
  `src-tauri/crates/uc-application/AGENTS.md`、
  `src-tauri/crates/uc-infra/AGENTS.md`
