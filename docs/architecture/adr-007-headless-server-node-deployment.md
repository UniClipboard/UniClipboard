# ADR-007：无头 server 节点（VPS）+ mobile-sync 网关部署拓扑

- **状态**：Accepted（2026-05-29 设计评审定稿；与引擎抽取的衔接随 [ADR-005](./adr-005-uc-engine-extraction.md) 推进）
- **日期**：2026-05-29
- **相关文档**：[`adr-005-uc-engine-extraction.md`](./adr-005-uc-engine-extraction.md)、`docs/architecture/mobile-sync-connect-uri.md`、`docs/architecture/module-boundaries.md`

## 1. 背景

### 1.1 需求

用户希望把 UniClipboard 以 **无头方式部署在自己的 VPS / 容器** 里，作为一个 **常驻在线的成员节点**：

1. 在其他设备看来它是一个 **普通 iroh 成员**，通过 P2P 接收桌面端复制的内容并落库。
2. 它对外开放一个 **mobile-sync 接口**，手机端可以拉取最新内容（并可推送，推送会 fan-out 回桌面）。
3. 全程 **无需系统剪贴板支持**（VPS 无 X11/Wayland display）。

### 1.2 与 ADR-005 的关系（关键定位）

这个"server 节点"**不是新的引擎概念，也不是新 host 类别**。它就是 [ADR-005](./adr-005-uc-engine-extraction.md) §2.3 中 `uc-host-desktop` 的一个 **运行模式**——一个 **没有 GUI、不接系统剪贴板的桌面级宿主**。

因此本 ADR 只记录"部署拓扑 + 桌面宿主的一个运行模式 + 公网可达性/安全"这一层决策；引擎抽取（uc-engine / EngineHandle / platform 拆分）全部以 ADR-005 为准，本 ADR 不重复、不冲突。

引擎抽取落地后（ADR-005 Stage 2），本 ADR 的全部产物（`ServerHeadless` 运行模式、mobile_lan 网关、iroh 宿主配置）**随 `uc-host-desktop` 一起留在 host 层**，`uc-engine` 不受影响——server 节点对引擎而言只是"被注入了 Noop 剪贴板适配器 + 特定 iroh 配置的一次 start"。

### 1.3 现状（评审期已核实）

- CLI 业务命令（init/invite/join/send/recv/watch）已能无头运行（`UC_DISABLE_SYSTEM_CLIPBOARD=1` → Noop）。
- daemon 此前 **无条件** `LocalClipboard::new()`（`uc-desktop/src/daemon/runtime_assembly.rs`），无 display 即失败——这是唯一的功能性无头障碍。
- headless 解密 **已支持**：无 DISPLAY/DBUS 时 `SecureStoragePort` 回退到 `FileSecureStorage`，`Standalone` 运行模式强制 keyring auto-unlock；`init`/`join` 一次性落盘即可无人值守解锁。
- **mobile_lan 子系统已存在且为双向**：`uc-webserver/src/mobile_lan/`、`uc-infra/src/mobile_sync/`、`uc-cli .../mobile_sync/`；SyncClipboard 协议，绑 `0.0.0.0`，`GET /SyncClipboard.json` 拉、`PUT` 推（PUT 经 `ApplyInboundClipboardUseCase` 落库并 fan-out 给 iroh peers），可经 `uniclip mobile-sync` 非交互启用；iOS/Android 客户端已存在。
- iroh 0.98 每次启动绑 **随机 UDP 端口**（`node.rs` 的 `.bind()` 用 `0.0.0.0:0`），可通过 `bind_addr()` 固定，并通过 `add_external_addr()` 广播已知公网地址。
- x11rb/wayland 在 Linux 下无条件编译，但用 Noop 后仅被链接、从不实例化（运行期零开销）。

## 2. 决策

### 2.1 新增 `DaemonRunMode::ServerHeadless`（运行模式归 `uc-desktop`）

在 `uc-desktop` 的 `DaemonRunMode` 中新增 `ServerHeadless` 变体。它在 **所有现存维度上等价于 `Standalone`**（自监听 OS 信号、强制 keyring auto-unlock、自驱 deferred services、`DaemonProcessMode::Standalone`），**唯一区别** 是新增方法 `runs_system_clipboard() == false`：

- 装配时用 `NoopSystemClipboard` 替代真实剪贴板，**不构造 `LocalClipboard`**（无 display 会失败）；
- **不 spawn `ClipboardWatcherWorker`**（没有 OS 剪贴板可监听）；
- 入站落库 / mobile_lan 网关 / fan-out 全部照常（入站持久化与事件发布独立于 OS 剪贴板写入，OS 写是 best-effort）。

**边界**：`RunMode` 是 **桌面宿主进程模型** 概念，不属于 `uc-cli`，也不属于（未来的）`uc-engine`。引擎只接受被注入的 `SystemClipboardPort`（这里是 Noop），对"自己是不是 server"一无所知。

### 2.2 CLI daemon 入口：保留单二进制自启（Scope A），逻辑下沉 host

> 部分回答 [ADR-005](./adr-005-uc-engine-extraction.md) §7 **Open Question #3**。

保留现有单二进制模型：`uniclip start` detached-spawn `uniclip daemon`（同一二进制 + 隐藏子命令）。**不** 在本阶段拆独立 daemon 二进制。

职责归位：

- `uc-cli` 只负责 **翻译用户意图 + 拉起宿主**：`start --server` 设 `UC_DAEMON_RUN_MODE=server`（子进程环境继承，与 `UC_PROFILE` 同模式），不解析 run-mode、不知道 `UC_DISABLE_SYSTEM_CLIPBOARD`、不引用 `ServerHeadless` 细节。
- `uc-desktop` 暴露 `daemon::run_standalone_from_env()`：自己读 `UC_DAEMON_RUN_MODE` 解析运行模式，server 模式下自己设置 Noop 剪贴板开关（平台层细节归宿主），再 `run`。

被否决的 **Scope B**（拆出独立 `uniclipd` 二进制、`uc-cli` 彻底不依赖 `uc-desktop`）：正交且更大，动 spawn/start/stop/probe/打包/CI 双二进制，并推翻"单二进制自启"的既有约定。**不阻塞本部署**，如要做须单开 ADR。

### 2.3 复用既有 mobile_lan 作为手机网关（不新增 API）

server 节点的"mobile-sync 接口"**直接复用现有 mobile_lan**，不新建协议/路由：

- 通过 `uniclip mobile-sync network set` / `add` 非交互启用并铸设备凭据。
- `GET /SyncClipboard.json` 拉最新；`PUT` 推（fan-out 回桌面）。
- mobile_lan 生命周期由 daemon settings 驱动（非 GUI），在 `ServerHeadless` 下照常启动。

### 2.4 iroh 直连：固定 UDP 端口 + 广播公网地址（relay 默认 Disabled）

为让桌面在公网直连 VPS（bridge 容器下 magicsock 只看得到私网 IP，发不了固定端口）：

- 新增 iroh 宿主配置 `bind_port` + `public_addr`（来源 env：`UC_IROH_BIND_PORT` / `UC_IROH_PUBLIC_ADDR`）。
- `Endpoint::builder(...).bind_addr("0.0.0.0:{port}")` 固定端口；**bind 后、首次 `EndpointAddr` 快照/配对/派发之前** 调 `endpoint.add_external_addr(public_addr)`，否则交换出去的地址 blob 不含公网地址。
- `RelayMode::Disabled` 走纯直连（贴合自托管初衷，零第三方）。地址不靠运行时 pkarr/DNS 发现，而靠配对时存进 `PeerAddressRepository` 的 `EndpointAddr`（公网 IP 固定，存下来长期有效）。
- **可回头项**：若某桌面网络封了出站 UDP 到该端口，无 relay 兜底。relay-as-fallback（n0 默认或自托管 iroh-relay）作为后续选项，不在本期默认开启。

与 ADR-005 §2.6 自洽：iroh endpoint 短生命、identity 长生命；`bind_port`/`public_addr` 是 **注入的宿主配置**，引擎不硬编码。

### 2.5 公网链路安全：Caddy TLS 反代 sidecar + 安装 URL 完整 base URL

mobile_lan 是 **明文 HTTP + Basic Auth**，仅为可信 LAN 设计。公网部署：

- **Caddy（或 nginx）反代 sidecar** 在 `443` 终结 TLS（自动证书），反代到 app 容器内部的 mobile_lan 端口；mobile_lan 端口 **只在 Docker 内网暴露，不向宿主/公网发布**。
- 手机安装 URL/QR 需指向 Caddy 的 `https://域名`，而现有构造是 `http://<advertise-ip>:<lan_port>`。因此 **`mobile-sync network set` 扩展 `--url <完整 base URL>`**，URL/QR 构造支持 scheme/host/port 覆盖；保留 `--ip <ip>` 的 LAN 形态。

### 2.6 headless 解密：复用既有机制（不新增解锁路径）

`ServerHeadless` 复用 `Standalone` 的强制 auto-unlock + `FileSecureStorage` 文件式 KEK。容器内 `HOME`/data dir 一致即可无人值守解锁；不引入任何新的 env 口令注入。

### 2.7 置备：加入已有空间，一次性交互，先于 daemon 启动

因为 `join` / `mobile-sync` 写命令都 `refuse_if_daemon_running()`，置备全部在 server 启动 **之前** 用一次性容器完成：

```
docker compose run --rm app uniclip join <code> <passphrase>
docker compose run --rm app uniclip mobile-sync network set --url https://<域名> --accept-network-risk
docker compose run --rm app uniclip mobile-sync add --label "<设备>"
docker compose up -d            # 此时才真正 uniclip start --server，读已持久化设置
```

### 2.8 Docker 拓扑与状态卷

- app 容器 `HOME=/data`，volume 挂 `/data`，**必须覆盖**：iroh identity、vault/keyslot、`uniclipboard.db`、iroh-blobs cache、文件式 KEK（丢失等于要重新 join）。
- 发布 iroh UDP 端口（`-p 42999:42999/udp`）；mobile_lan 端口仅 `expose` 到 Docker 内网；仅 Caddy 的 `443` 对公网发布。

## 3. 明确不做的事

- ❌ **不引入 store-and-forward / 中央 relay blob 暂存**。server 节点是普通成员，只持久化"它在线时收到的"内容；与 [ADR-005](./adr-005-uc-engine-extraction.md) §2.5/§2.7"对端 offline 时数据落发送方本地、不引入中央暂存"一致。它 **不是离线信箱**。
- ❌ 不为手机网关新建协议/路由（复用 mobile_lan）。
- ❌ 不为 MVP 把 x11rb/wayland feature-gate 掉（链接但不实例化，运行期零开销）。
- ❌ 不在本期拆独立 daemon 二进制（Scope B）。
- ❌ 不在本期给 mobile_lan 加原生 TLS（用反代代替；原生 rustls 是 mobile_lan v2 的后续选项）。

## 4. 后果

### 4.1 正向

- 唯一功能性无头障碍（`runtime_assembly` 的无条件 `LocalClipboard::new()`）被一个干净的运行模式开关消除；普通 daemon 行为零变化。
- 手机网关、入站落库、headless 解密 **复用既有能力**，新代码集中在：`ServerHeadless` 变体、Noop/跳过 watcher 装配、`run_standalone_from_env`、iroh `bind_port`/`public_addr`、`--url`、Docker/Caddy 编排。
- RunMode 归位到 `uc-desktop`，为 ADR-005 的引擎抽取扫清了一处 host/CLI 职责混淆。

### 4.2 反向 / 成本

- 多一套部署文档与 Docker/Caddy 编排要维护。
- 公网明文经反代终结 TLS——运维必须保证 mobile_lan 端口不直接暴露公网（容器内网隔离 + 仅发布 443）。
- `RelayMode::Disabled` 下无 relay 兜底（见 §2.4 可回头项）。

### 4.3 边界铁律（落地后违反即偏离本 ADR）

1. 在 `uc-cli` 里解析 run-mode 或引用 `UC_DISABLE_SYSTEM_CLIPBOARD` / `ServerHeadless` 细节（应只设 spawn 契约 env）。
2. 为 server 节点新建"未投递内容"的持久化暂存表 / 中央 relay（违反 §3 与 ADR-005 §2.5）。
3. 为手机网关另起一套协议而非复用 mobile_lan。
4. 把 mobile_lan 明文端口直接发布到公网（必须经反代）。

## 5. 实施路径（对应 issue）

| Issue | 切片 | 类型 | 对应本 ADR |
|---|---|---|---|
| [#898](https://github.com/UniClipboard/UniClipboard/issues/898) | `ServerHeadless` 运行模式 + 无头启动 + 收 P2P 入站（**Scope A**：逻辑下沉 `run_standalone_from_env`） | AFK | §2.1 §2.2 §2.6 |
| [#899](https://github.com/UniClipboard/UniClipboard/issues/899) | 无头 daemon 跑通 mobile_lan 网关（拉/推/fan-out） | AFK | §2.3 |
| [#900](https://github.com/UniClipboard/UniClipboard/issues/900) | iroh 直连：pin UDP 端口 + 广播公网地址 | AFK | §2.4 |
| [#901](https://github.com/UniClipboard/UniClipboard/issues/901) | 手机安装 URL/QR 指向反代（完整 base URL） | AFK | §2.5 |
| [#902](https://github.com/UniClipboard/UniClipboard/issues/902) | Docker + Caddy + 状态卷 + 置备 runbook | HITL | §2.5 §2.7 §2.8 |

关键路径：#898 是唯一阻塞起点；#899/#900 在其后可并行；#901 接 #899；#902 收口。

## 6. 待决问题（Open Questions）

1. **relay 兜底**：是否在 `RelayMode::Disabled` 之外提供"direct 优先 + relay 兜底"开关（n0 默认 vs 自托管 iroh-relay）？影响封了出站 UDP 的桌面网络。
2. **mobile_lan 原生 TLS（v2）**：是否最终给 mobile_lan 加 rustls，免去反代依赖？（本期用反代规避。）
3. **Scope B**：是否后续拆独立 `uniclipd` 二进制以彻底解耦 `uc-cli ✗→ uc-desktop`？（须单开 ADR；与 ADR-005 OQ#3 合并考虑。）
4. **多 server 节点 / 多空间**：单 VPS 单空间已覆盖；多空间/多 profile 容器编排未定。

## 7. 决策记录

本 ADR 由 2026-05-29 的设计评审（grilling）推导，核心产品取舍：

- server 节点 = 普通 iroh 成员 + 手机 HTTP 网关，**不是路由中转、不是离线信箱**。
- 复用既有 mobile_lan，不造新接口。
- 公网安全用 Caddy 反代终结 TLS。
- iroh 直连用固定端口 + 广播公网地址，relay 默认关闭。
- CLI 保留单二进制自启，RunMode 归 `uc-desktop`（Scope A）。

任何对上述取舍的修订需更新本节并视情况新建后续 ADR。
