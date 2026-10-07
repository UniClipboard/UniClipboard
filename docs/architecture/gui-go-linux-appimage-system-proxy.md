# Go GUI 的 Linux AppImage 系统代理（切片 17c12）

本文是 17c12 的契约，**先于** 运行器写成。运行中发现的偏差记录在「契约修订」一节，不改写原条款。源码调查结论（下表）也是在写运行器之前做的，每条给出可复查的来源。没有源码或运行证据的内容标为 OPEN，不当作事实。

## 联网路径与各自的代理机制（源码调查，互不外推）

「系统代理」在 Linux 上没有统一入口。本产品的联网路径各自走不同的库，机制不同，**一条路径成功不能推出另一条**。

| 路径 | 实际使用的库 | 代理来源（源码依据） | 回环处理 |
| --- | --- | --- | --- |
| A. WebView 加载前端与访问 daemon（`baseUrl`/`wsUrl`，`127.0.0.1`） | WebKitGTK 4.1 + libsoup3 | Wails beta.28 在 Linux 没有任何代理 API（对 `pkg/`、`internal/` 全文检索 `proxy`，只有 WebView2/模板/文档命中，`linux_cgo.go` 只调用 `webkit_network_session_get_default()`）。代理由 GLib 的默认 `GProxyResolver` 决定，而它来自 GIO 模块 | 取决于解析器 |
| B. WebView 访问外部 HTTP(S)（页面里的 `fetch`/资源） | 同 A | 同 A | 同 A |
| C. Go 宿主 HTTP 客户端：更新器 `update.NewHTTPClient()` = `&http.Client{}`（`apps/gui-go/internal/update/update.go:291`，`Transport` 为 nil） | `net/http` 的 `DefaultTransport` | `http.ProxyFromEnvironment`：只读 `HTTP_PROXY/HTTPS_PROXY/NO_PROXY`（及小写），不读 GNOME 设置；Go 对 `localhost` 与回环地址自带绕过（`vendor/golang.org/x/net/http/httpproxy/proxy.go` 的 `useProxy`） | 自动绕过 |
| D. Go 宿主到 daemon：`packages/desktop-host-go/daemonclient/client.go:46-48` | `net/http` | `transport.Proxy = nil`，显式禁用 | 不经代理 |
| E. daemon（Engine）：rendezvous HTTP 客户端 `RendezvousClient`（固定主机 `https://rendezvous.uniclipboard.app`） | `reqwest 0.12.28`（`Cargo.lock`），未调用 `.proxy()`/`.no_proxy()` | reqwest 0.12 默认读取环境代理（`HTTP_PROXY/HTTPS_PROXY/ALL_PROXY/NO_PROXY`）；Linux 上不读系统设置。**reqwest 不自动绕过回环**（`NoProxy` 只来自 `NO_PROXY` 变量） | 无自动绕过 |
| F. daemon（Engine）：iroh 中继/端点 | `iroh 1.3.0`、`iroh-relay 1.3.0` | 代理只在端点构建器显式调用 `proxy_url(..)` 或 `proxy_from_env()` 时才有（`iroh-1.3.0/src/endpoint.rs:700-715`，仅 `HTTP_PROXY/HTTPS_PROXY` 两个变量）。对 Engine `crates/` 与 `compatibility/` 全文检索，**没有任何调用** | n/a |
| G. daemon 的遥测（OTLP） | `reqwest 0.13.3`，`default-features=false`，没有 `system-proxy` 特性 | 本片不触发，不下结论 | n/a |

旧 Tauri 外壳的做法（`crates/uc-tauri/src/process_environment.rs`）：进程入口把 `localhost,127.0.0.1,::1` 合并进 `NO_PROXY`/`no_proxy`，使得 libproxy/GIO、reqwest 等读取环境的代码都不经代理访问回环；AppImage 里同时用 `GIO_MODULE_DIR` 限定 GIO 模块目录。Go/Wails 外壳 **没有** 对应的 `NO_PROXY` 合并（对 `apps/gui-go` 检索 `NO_PROXY`/`HTTP_PROXY` 无命中）。这是否构成回归，由下面的实测决定，不靠推断。

AppImage 的 GIO 模块：只带 `libgiognutls.so`（17c7），不带 libproxy、gnome-proxy、dconf（`gui-go-linux-appimage-runtime-deps.md`）。因此路径 A/B 的解析器在 AppImage 内是否存在、是否读取宿主设置，需要 **运行时证据**；假设：AppImage 内没有代理解析器，WebView 不使用任何代理（OPEN，待实测，不当事实）。

## 失败方式与验收

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| P1 | 配置了代理（环境变量或宿主系统设置）后，WebView 访问 daemon 的回环 HTTP/WebSocket 被送进代理或失败，前端不可用 | 真实 AppImage + 真实 daemon：代理在「拒绝一切」模式（任何到达代理的请求都得到 403 并记录）；断言前端面板就绪、daemon HTTP 取数成功、WebSocket 事件到达、**代理日志中没有 `127.0.0.1`/`localhost` 目标** |
| P2 | 代理不可达（端口无监听）时前端因代理而卡死 | 同 P1，代理端口关闭：面板仍就绪，daemon 连接仍可用 |
| P3 | WebView 访问外部 HTTP(S) 的实际路径被误认：只看到成功就以为用了代理 | 受控目标（非回环地址，容器内别名）与代理各自记录；三种结果分开：目标日志有请求而代理无 = 直连；代理日志有 CONNECT/请求 = 经代理；两者皆无 = 失败。因果正对照：同一容器里一个已知遵守环境代理的客户端（curl）必须出现在代理日志里，证明代理与日志链有效；`no_proxy` 绕过对照 |
| P4 | 宿主系统代理设置（GNOME `org.gnome.system.proxy`）被读取与否 | 容器里安装宿主的 dconf/gsettings 与 `glib-networking`，用 `gsettings` 写入手动代理，断言 WebView 外部请求的实际路径（直连/经代理），并记录 AppImage 的 GIO 模块目录。不预设必须生效 |
| P5 | Go 更新器是否按环境代理访问非回环的 feed | 更新器（e2e 构建的 `UC_UPDATE_ENDPOINT` 覆盖）指向非回环主机名；代理日志与 feed 日志区分；回环 feed 在设置了代理时仍直连（Go 自带绕过）。仅覆盖环境变量机制 |
| P6 | daemon 的 rendezvous HTTPS 请求是否按环境代理出站（只是 Engine 出站的一种，**不外推 iroh**） | 容器网络 `--internal`，`rendezvous.uniclipboard.app` 只解析到受控夹具或不可达；代理对该主机只记录 `CONNECT` 并返回受控错误，**不转发、不到生产服务**；不创建真实配对。触发方式见「触发与安全边界」 |
| P7 | 代理不可达与恢复 | 代理停止 → 请求失败的形态（错误码、耗时、是否永久卡住）；代理恢复 → 新请求经代理。注意 reqwest 在构建客户端时读取环境，daemon 内是否重新读取是观察项 |
| P8 | `NO_PROXY` 与大小写优先级的实际行为 | 记录各路径（Go、reqwest、WebView）对 `NO_PROXY`、`no_proxy`、`HTTP_PROXY`/`http_proxy` 同时存在时的实测结果，不假设一致 |
| P9 | 回归：代理夹具不改变既有链路 | 17c7 WebView TLS、17c5 便携 E2E、内容检查在同一 AppImage 上仍通过 |

PAC（`autoconfig_url`）、代理认证、系统设置动态变更、SOCKS：先看路径 A/B 的解析器是否存在；若 AppImage 内没有解析器，这些不适用于 WebView，**标为「未支持」并写实测证据**；对 Go（路径 C）与 reqwest（路径 E）只调查环境机制（不支持 PAC；认证只可通过 URL 内的凭据，本片不验）。未验证项保持 OPEN。

## 触发与安全边界

- 所有测试在 `--internal` 网络（无出口）内，主机名由容器自己的 `/etc/hosts` 解析到夹具。代理夹具不转发到任何外部地址；对 `rendezvous.uniclipboard.app` 只记录并返回受控错误。
- 不创建真实 rendezvous 配对、不注册；不修改 endpoint 业务代码。若没有无副作用的触发方式（受控主机上的请求才算），P6 保持 OPEN 并写明原因。
- 不修改真实用户 profile、密钥或全局配置；所有数据在运行器创建的沙箱中。

## 修复策略（只有在实测证明缺陷之后）

1. 先用真实 AppImage + 代理负对照证明是否有回归，保留失败工件；
2. 再选成熟库/框架的最小正确集成（例如 Go 侧复用 `net/http` 的 `ProxyFromEnvironment`、在进程入口复用 Tauri 已有的回环 `NO_PROXY` 约定），不手写代理层；
3. 不给所有子进程盲目改全局代理环境；
4. 修复后在同一真实包上复跑同一运行器。

## 明确不证明

真实桌面会话、Wayland、GPU、原生 amd64、其他发行版、deb/rpm（不带 AppImage 的 GIO 模块限定，路径 A/B 的行为会不同，OPEN）、macOS/Windows 的系统代理（各自的机制不同）、Engine 内 iroh 的代理能力（Engine 在本仓只读，上表 F 只是源码检索结论）。
