# 发现记录：Only LAN + 允许虚拟网卡配对失败

## 已确认事实

- 双方生产 profile。
- 双方都开启 Only LAN：`allow_relay_fallback=false`。
- 双方都允许虚拟网卡：`allow_overlay_network_addrs=true`。
- Mac 发布地址包含 `100.79.191.42`，Fedora 发布地址包含 `100.117.177.15`。
- 双方 Tailscale ping 互通。
- Fedora 能解析配对码，能建立配对连接并发送 Request。
- Mac 只记录到底层配对连接接收超时，没有进入应用层配对处理日志。

## 当前根因判断

- `RendezvousPairingInvitationAdapter::serialize_ticket` 直接使用 `endpoint.addr()`。
- `endpoint.addr()` 是本端当前地址快照，可能包含本机代理、链路本地、虚拟机网段等不适合给远端拨号的地址。
- `node.rs` 的地址过滤逻辑没有被配对码生成路径复用，导致配对 sponsor ticket 绕过了已有过滤规则。
- `IrohPairingSessionAdapter::dial_by_invitation` 直接调用 `endpoint.connect`，没有复用已有的分批重试和实际路径日志。

## 修复方向

- 抽出可复用的 EndpointAddr 过滤函数，用同一套规则服务 endpoint 发布日志、配对码生成和测试。
- 配对码生成时按照当前设置过滤 sponsor ticket。
- 配对拨号时复用已有连接 helper，补上实际路径日志。

## 代码复核

- `node.rs` 已经有 `is_virtual_nic_ip` 和 `apply_addr_filter`，并已有测试证明 `allow_overlay=true` 时保留 Tailscale 地址、仍剔除 `198.18.0.0/15` 和 `169.254.0.0/16`。
- 这套规则目前是 `node.rs` 私有函数，`rendezvous/invitation_adapter.rs` 无法复用，所以 `serialize_ticket` 仍直接序列化 `endpoint.addr()`。
- `Settings.network.allow_overlay_network_addrs` 已经从 GUI / non-GUI bootstrap 传入 `IrohNodeConfig`，但 invitation adapter 也持有 `SettingsPort`，可以在生成配对码时读取同一设置。

## 最终修复

- 新增 `network/iroh/addr_filter.rs` 作为地址过滤唯一入口。
- `node.rs` 和配对码生成路径都复用同一套地址过滤规则。
- 配对码生成时读取当前 `allow_overlay_network_addrs` 设置：允许虚拟网卡时保留 Tailscale，仍剔除 `198.18.0.0/15` 和 `169.254.0.0/16`。
- 配对加入方改用已有分批重试连接流程，失败时仍返回 sponsor 不可达，同时可记录实际选中的连接路径。
