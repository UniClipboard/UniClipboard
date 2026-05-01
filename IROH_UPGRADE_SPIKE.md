# iroh 0.95 → 0.98.x 升级 spike 报告

## 1. 版本选择
- iroh: 0.95.1 → **0.98.2**
- iroh-blobs: 0.97.0 → **0.100.0**
- iroh-tickets: 0.2 → **0.5.0**
- iroh-quinn-proto: 0.13 → **替换为 noq-proto 0.17.0**（iroh 0.97 把 quinn 分叉成了 noq；`iroh-quinn-proto` 这个传递依赖在 0.97+ 时代不再存在）
- 兼容性来源：
  - `cargo info iroh` (0.98.2, rust 1.89, 用 `noq` 作为 transport 后端)
  - `cargo info iroh-blobs` (0.100.0, 与 iroh 0.98 同代)
  - `cargo search iroh-tickets` → 0.5.0 唯一最新版
  - `cargo update` 一次解析成功，没有 SemVer 冲突（log 第 34-58 行）

## 2. cargo check 总览
- 总 error: **8**（仅 lib，未触及 tests/probes，因为 lib 没过就不会 check tests）
- 总 warning: **0**
- 默认 feature vs --all-features 差异：**完全相同**（uc-infra 没声明任何自定义 features，且 Cargo.toml 没引用 iroh 的 `discovery-*` / `address-lookup-*` 等 feature flag，所以 feature 改名对项目零影响）

## 3. 错误分类

| 类型 | 条数 | 代表 error code 或 typename | 修复难度 |
| --- | --- | --- | --- |
| **改名（机械替换）** | 0 | — | — |
| **API 签名变化** | 4 | E0061 `Endpoint::builder(preset)` / E0061 `SecretKey::generate()` / E0599 `Endpoint::conn_type` / E0282 type inference | 中 |
| **crate 改名 / 路径变化** | 2 | E0433 `iroh_quinn_proto` → `noq_proto`；E0603 `iroh::endpoint::TransportConfig` 变私有 + non_exhaustive | 中 |
| **feature flag 改动** | 0 | — | — |
| **真正的语义/行为变化** | 2 | `conn_type()` API 整体被移除（来自 0.97 Custom Transports 重构）；`TransportConfig` 不再公开导出（影响 BBR 配置路径） | 中-高 |

> **关键观察**：项目当前代码库**没有引用** `iroh::discovery` / `Discovery` / `MdnsDiscovery` / `DhtDiscovery`，所以 0.96 那个 `discovery → address_lookup` 大改名对我们**零影响**。这意味着升级远比表面看起来便宜。

## 4. 关键 API 可用性
- [x] **`Builder::addr_filter`** — 存在于 `iroh-0.98.2/src/endpoint.rs:593`，签名 `pub fn addr_filter(mut self, filter: AddrFilter) -> Self`
- [x] **`AddrFilter::new(closure)`** — 存在于 `iroh-relay-0.98.0/src/endpoint_info.rs:247`，签名 `pub fn new(f: impl Fn(&Vec<TransportAddr>) -> Cow<'_, Vec<TransportAddr>> + Send + Sync + 'static) -> Self`。另有现成工厂：`unfiltered()` / `relay_only()` / `ip_only()`。重新导出在 `iroh::endpoint::AddrFilter`
- [x] **`MdnsAddressLookup`** — 存在于 `iroh-0.98.2/src/address_lookup/mdns.rs:113`，需 feature `address-lookup-mdns`
- [x] **`TransportAddr::Ip(SocketAddr)`** — 存在于 `iroh-base-0.98.0/src/endpoint_addr.rs:54`，enum variant 还在，项目 `node.rs:163` 不会因此报错
- [x] **`Endpoint::addr() → EndpointAddr`** — 存在于 `iroh-0.98.2/src/endpoint.rs:1127`，签名未变；`persistable_addr.rs` 不会报错
- [x] **`EndpointAddr::from_parts(id, addrs)`** — 存在于 `iroh-base-0.98.0/src/endpoint_addr.rs:104`，签名 `pub fn from_parts(id: PublicKey, addrs: impl IntoIterator<Item = TransportAddr>) -> Self`，与项目用法兼容

**6/6 关键 API 全部可用**，且 spike 编译时这些类型/路径**没有任何一个出错**——所有 8 个错误都跟 `addr_filter` 这条主线无关。

## 5. 触及文件清单
所有报错都集中在 `uc-infra/src/network/iroh/` 下 4 个文件：

- `src-tauri/crates/uc-infra/src/network/iroh/node.rs` — **3 errors**
  - L22: `use iroh::endpoint::{TransportConfig, VarInt};` — `TransportConfig` 变私有
  - L25: `use iroh_quinn_proto::congestion::BbrConfig;` — crate 不存在，改 `noq_proto::congestion::BbrConfig`
  - L281: `Endpoint::builder()` — 0.98 要求传 `preset: impl Preset` 参数
- `src-tauri/crates/uc-infra/src/network/iroh/connect.rs` — **2 errors**
  - L50–51: `endpoint.conn_type(addr_id)` — `conn_type()` 方法不存在了；需要找替代（多半是 `address_lookup()` services 或新的 connection telemetry API）
- `src-tauri/crates/uc-infra/src/network/iroh/blobs.rs` — **2 errors**
  - L107–108: 同上 `conn_type()` 调用
- `src-tauri/crates/uc-infra/src/network/iroh/identity_store.rs` — **1 error**
  - L90: `SecretKey::generate(&mut rand::rng())` — 0.98 改成无参 `SecretKey::generate()`，内部 RNG

其余 8 个 `network/iroh/*.rs` 文件 + 3 个 `tests/iroh_*_probe.rs` **没有报错**（实际上 lib 没过 tests 不会被 check，但根据当前用到的 API 表面看应也安全）。

## 6. 工作量估算
- **机械改名**：~10 min（`iroh_quinn_proto` → `noq_proto`、`SecretKey::generate(&mut rng)` → `SecretKey::generate()`、`Endpoint::builder()` 加 preset）
- **API 适配**：~2-3h
  - `Endpoint::builder(preset)`：需要决定用哪个 `Preset`（n0 默认 / 自定义）。**这正好是接 `addr_filter` 的入口**，顺手做。
  - `TransportConfig` 变私有：需要找新 BBR 配置路径——可能是通过 `noq` 的 transport config，或者 iroh 提供了别的封装。可能需要用 `unstable-custom-transports` feature。
  - `conn_type()` 移除：替换成 0.97 Custom Transports 后的连接元数据 API，估计是 `Endpoint::address_lookup()` 或 connection-level 方法。需要短期翻 changelog/示例。
- **重新设计**：**无**。所有架构假设（discovery/AddressLookup、TransportAddr、EndpointAddr、Endpoint::addr）都还在；这次升级**不需要重新设计任何东西**。
- **总体**：**半天**（含写 `addr_filter` 屏蔽 `198.18.0.1` 的实际逻辑 + 跑一遍 P2P 烟囱测试）

## 7. 风险点
1. **`TransportConfig` 变私有 + non_exhaustive**：当前项目用它来配 BBR congestion。如果新 API 不允许同等粒度的 BBR 控制，会被迫**降级到默认 congestion**。要验证 noq-proto 的 `BbrConfig` 是否真的能塞回 iroh 0.98 的 transport 配置，或者评估能否接受默认 congestion（CUBIC）。
2. **`Endpoint::builder(preset)` 的 Preset 选型**：n0 在 0.97 引入 Custom Transports 是为了支持自定义 transport 后端。我们大概率只需要默认 preset，但要查清楚默认 preset 是哪个、是否需要打开 `unstable-custom-transports` feature。
3. **`conn_type()` 移除的下游影响**：项目用它做 holepunch 状态日志/可观测。如果新 API 形态完全不同（比如改成 stream of events），可能要重写一小块 telemetry。可观测性降级是可接受的兜底。
4. **依赖图扰动巨大**：这次解析新增了 ~50 个 crate（noq、noq-udp、noq-proto、iroh-dns、wasm-bindgen 升 0.2.106→0.2.120、hickory-* 升 0.25→0.26-beta 等），编译时间和二进制大小可能涨 5–15%。增量编译不会受影响，CI 冷启会变慢。
5. **iroh-blobs 0.100 跨了 3 个 minor**（0.97 → 0.100）：本次 spike 只 check 了 lib 编译能力，没有验证 `iroh-blobs::Downloader` API、ALPN、protocol handler 这些细节。这部分等 lib 过了再针对性看。
6. **`hickory-proto/resolver` 从 0.25.2 跳到 0.26.0-beta.4**：beta 依赖入主分支不理想，但这是 iroh-relay 0.98 拉进来的传递依赖，我们无法绕开。

## 8. 推荐路线
- [x] **直接升 0.98.2，A → B 一气呵成**

理由：8 个错误全部局限在 4 个文件，0 个改名错误，所有关键 API 都在；分两步（先 0.95→0.96 吃改名，再 0.96→0.98 吃 addr_filter）反而要付两次"改 Cargo.toml + cargo update + 全量 rebuild"的成本，而第一步根本没改动需要做。直接一步到位最经济。

## 9. 报告产出位置
- worktree 路径：`/Users/mark/conductor/workspaces/uniclipboard/istanbul`
- 分支：`mkdir700/debug-slow-sync`（HEAD `2dd4ec83`）
- 注：原始指派的 `agent-af305ad9` worktree 在 v0.5.0 老 commit 上、根本没有 iroh 代码；编辑权限也只许在 `istanbul` 这个 worktree 里写。已 reset agent-af305ad9 到原状态 `8f3edb12`，未触碰其它 worktree。

未 commit。改动的文件：
- `src-tauri/crates/uc-infra/Cargo.toml`（iroh deps 升级）
- `src-tauri/Cargo.lock`（cargo update 自动解析）
- `IROH_UPGRADE_SPIKE.md`（本报告）

完整 cargo check 输出保留在 `/tmp/iroh-spike.log`（--all-features）和 `/tmp/iroh-spike-default.log`（默认）。
