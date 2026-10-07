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
| E. daemon（Engine）：rendezvous HTTP 客户端 `RendezvousClient`（固定主机 `https://rendezvous.uniclipboard.app`） | `reqwest 0.12.28`（`Cargo.lock`），未调用 `.proxy()`/`.no_proxy()` | `reqwest-0.12.28/src/proxy.rs:515` 的 `Matcher::system()` 调 `hyper-util-0.1.20` 的 `Matcher::from_system()`，其 Linux 分支就是 `from_env`（`client/proxy/matcher.rs:228-234`）：依次取 `ALL_PROXY/all_proxy`、`HTTP_PROXY/http_proxy`、`HTTPS_PROXY/https_proxy`、`NO_PROXY/no_proxy`（大写优先），系统设置只在 macOS/Windows 且开 `client-proxy-system` 时读取（`:242-245`）。该文件中没有回环自动绕过，绕过只来自 `NO_PROXY` 规则；匹配器在客户端构建时创建（运行中是否重读是 P7 的观察项） | 无自动绕过 |
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

## 契约修订

原条款保留，不改写。修订针对「PAC、认证、动态变更」一段的目标偏移风险。

### R1：缺少解析器不是「不适用」，也不是完成

- 原条款写「若 AppImage 内没有解析器，这些不适用于 WebView，标为未支持」。**撤回这条推论作为完成标准**：AppImage 因 `GIO_MODULE_DIR` 限定而丢失发行版成熟的代理解析器（`glib-networking` 的 `libgiolibproxy`/`libgiognomeproxy`、libproxy、dconf GSettings 后端），本身就是需要补齐的打包缺口，不是排除理由。
- 事实核对（静态，不是行为结论）：旧 Tauri v1.1.1 AppImage（官方资产，SHA-256 见 `linux-17c10/tauri-v1.1.1/sha256.txt`，`unsquashfs -l`）的 `usr/lib/aarch64-linux-gnu/gio/modules` 只有 `libgiognutls.so`，没有 libproxy、gnome-proxy、dconf 模块，且 Tauri 入口同样设置 `GIO_MODULE_DIR`。因此旧包是否在用户机器上使用系统代理，没有被运行验证，**不能用「旧包也没有」当作迁移计划的完成理由**。产品文档（`docs-site`）对代理没有承诺，只有「手机 App 不依赖代理」与「本地代理会拦截中继」两类说明；迁移计划把「AppImage 的系统代理（libproxy/gnome-proxy）」列为明确 OPEN 项（`go-gui-migration-plan.md`），所以验收标准是「真实可用或写明框架/发行版边界」，不能缩小。
- 执行顺序（先文档，再实现）：
  1. **基线实测**：当前 AppImage（只带 `libgiognutls`）在 P1–P8 的实际结果，保留失败/不支持的原始证据。
  2. **调查并复用成熟维护库的打包集成**，不自写 resolver：`glib-networking` 的 `libgiolibproxy.so`/`libgiognomeproxy.so`（与捆绑 GLib 2.80 同源同 ABI，沿用 17c7 `deploy_gio_modules` 的 `dpkg -S` 来源校验与 NEEDED 校验）、libproxy 及其 PAC 运行时、`dconf` 的 `libdconfsettings.so` 与 `gsettings-desktop-schemas`（`org.gnome.system.proxy`）。17c4 已证明 **宿主** 副本会崩溃，所以只能带与捆绑 GLib 同构建的副本，不能放开 `GIO_MODULE_DIR`。
  3. 补齐后，同一运行器复测：
     - 回环直连不变（P1/P2：daemon HTTP/WebSocket 与前端可用，代理日志无回环目标）：若解析器从环境读取而没有回环绕过，按 Tauri 既有约定（`localhost,127.0.0.1,::1` 合并进 `NO_PROXY`）在进程入口最小修复，而不是改全局代理环境；
     - 宿主配置可达：用户的 dconf 数据库（`~/.config/dconf/user`，只读不需要 daemon）与环境变量，在非便携模式与便携模式（`HOME` 被重定向，F7）下各自的实际结果；
     - 模式：环境变量（大小写、`NO_PROXY`）、GNOME 手动代理、`ignore-hosts` 绕过、PAC（`autoconfig-url`；libproxy 的 PAC 运行时是否可打包与其许可证/体积）、认证（代理 URL 内的凭据：是否可测，不可测则写明）、系统设置动态变更（运行中的 GUI 是否随 `GSettings` 变化而改变路径，还是需要重启）。
  4. 打包后的体积、许可证、依赖闭包与内容检查（`appimage_content_check.py`、`runtime_pin`）一并更新，并复跑 17c7 TLS、17c5 便携、17c10/17c11 helper 回归。
- 若出现框架或发行版边界使某一模式无法完成（例如 PAC 运行时没有可再分发的许可，或 WebKitGTK 不随 GSettings 变化刷新），**写出实际证据并保持 OPEN，继续做其余可实现部分**；不因缺少模块、没有测试或没有产品文档而把整项排除。
- P6 不变：Engine 的 rendezvous 路径只是 Engine 出站的一种，iroh 单独结论。

### R2：基线实测（未改动的 17c11 包）与对表 F 的更正

基线运行 `baseline-a2a001ac-v2`（AppImage SHA-256 `a2a001ac…`，即 17c11 的包，产品代码未变）。早先的运行全部保留：`dev1`（tinyproxy 以 `nobody` 身份进不了沙箱目录，代理日志为空；`ss` 进程名被截断为 15 个字符，`WebKitNetworkPro` 永远匹配不上）、`dev2`（误把 curl 的 `CONNECT` 记到 WebView 上，判成 proxied；WebView 与 curl 共用主机名与日志窗口）、`dev3`（修正后的版本，没有 gsettings 场景）、`baseline-a2a001ac`（宿主 libproxy 未设 `XDG_CURRENT_DESKTOP`，gsettings 为 `manual` 而 `proxy` 输出 `direct://`）。修正方式：WebView 与 curl 使用不同主机名并各自保存代理日志窗口，回环判断只看 `Request` 行的目标（`Connect (file descriptor N): 127.0.0.1` 是客户端地址）。

观测结果（`passed` 表示夹具与对照成立，**不是** 系统代理功能完成）：

| 配置 | WebView 外部 HTTPS | curl 对照 | 回环 |
| --- | --- | --- | --- |
| 无代理 | direct | direct | 无泄漏 |
| 环境变量，代理放行 / 拒绝 / 不可达 | direct / direct / direct | proxied / refused / failed | 代理日志没有 daemon 端口与页面上报通道 |
| 宿主 GNOME 手动代理（用户 dconf），放行 / 拒绝 | direct / direct | 经宿主 `proxy` 命令行取得的 `http://127.0.0.1:<port>`：proxied / refused | 同上 |

- **缺口已证实**：AppImage 内的 WebView 对环境变量和 GNOME 设置都不使用代理（direct，目标看到了请求），而宿主的 libproxy 命令行能读到同一份 dconf 设置。代理被拒绝或不可达时请求直接逃逸到目标，这正是 R1 要求避免的静默直连。
- WebKitNetworkProcess（WebView 自身）持有 4 个到 daemon 回环端口的 TCP 连接；Go 宿主（`uniclipboard` 进程，`daemonclient` 的 `Proxy=nil`）另有 3 个。两者按真实进程名分别记录，不混写。
- **对表 F 的更正**：表 F 写「Engine 没有调用 `proxy_from_env`/`proxy_url`，所以没有代理」是源码检索结论，**被运行证据推翻**。设置了环境代理后，daemon 经代理发出 `CONNECT dns.iroh.link:443`、`CONNECT <region>.relay.n0.iroh.link.:443`、`CONNECT 1.1.1.1:443`、`CONNECT 8.8.8.8:443` 与 `GET http://use1-1.relay.n0.iroh.link./generate_204`；没有代理变量，或只配置 gsettings 时没有这些请求。已核实的源码事实：`iroh 1.3.0` 的网络探测与地址解析使用 `reqwest`（`net_report/reportgen.rs`、`address_lookup/pkarr.rs`），reqwest 0.12 默认读取环境代理；iroh 端点自己的 `proxy_url` 只在显式设置时才有。中继 `CONNECT` 具体由哪个组件发出 **没有做源码归因**，保持 OPEN。Engine 的出站只认环境变量，不读取 GNOME 设置（与 reqwest 的 Linux 分支一致）。这些结论只是观测到的行为，Engine 在本仓只读，不外推到其他版本。

## 阶段结果（固定的 stage3 包，进行中）

产品修复链（缺口 → 修复 → 复测）：

1. **stage1**（包 `970a9830…`）：AppImage 随附 GNOME 代理解析器（`libgiognomeproxy`）、libproxy 解析器（`libgiolibproxy`，环境变量、PAC）、dconf GSettings 后端及其依赖闭包（约 8 MB：libcurl-gnutls、libssh、libldap/liblber、libsasl2、librtmp、libduktape、libcrypto，均经 `dpkg -S` 溯源并检查 NEEDED 闭包）。GNOME 手动代理的 WebView 路径变为 proxied / refused / failed，没有静默直连。
2. **stage2**（包 `202485c5…`）：环境变量路径开通，但 libproxy 没有回环绕过，daemon 的回环连接被送到代理（红色，证据保留）。
3. **stage3**（包 `8c9881b0…`，产品提交 `e649e7057`）：`apps/gui-go/proxy_env_linux.go` 在进程初始化时把 `localhost,127.0.0.1,::1` 合并进 `NO_PROXY`/`no_proxy`（与 Tauri 的 `process_environment.rs` 同一约定，保留用户条目，`*` 优先），早于 WebKitGTK 创建和 daemon 启动。

stage3 同一个包上的两个矩阵。页面自身的 HTTP 取数与 WebSocket 帧只存在于非便携矩阵（`proxy-nonportable-v2`，页面探测 runner `16155f180`）；便携矩阵 `proxy-portable` 是页面探测之前的 runner 跑的（只有 socket 与代理日志），**没有** 页面 HTTP/WS 覆盖，便携的页面证据来自 `smoke-pageprobe-v2`、`p8-portable-v2`、`p7-portable`，最终仍须在最终包上用当前 runner 跑完整便携矩阵：

| 矩阵 | 运行目录 | 退出码 | 观测 | 要求 |
| --- | --- | --- | --- | --- |
| 便携 | `stage3/proxy-portable` | 0 | 85 项，`passed=true` | 24/24，`functionalPassed=true` |
| 非便携（真实 HOME、会话总线、Secret Service） | `stage3/proxy-nonportable-v2` | 0 | 86 项，`passed=true` | 24/24，`functionalPassed=true`，G7 无违规且无 unverified |

路由：无配置 direct；放行 proxied；拒绝 refused；不可达 failed（目标从未看到请求）；守护进程回环不经代理。便携模式下真实用户 dconf 不可见是 F7，**仍是 OPEN 的产品决策**，运行器只把它记为观测，不静默更改语义。

### 页面探测协议修正

首次页面探测运行 `smoke-pageprobe` 失败（保留，见其 `ATTRIBUTION.txt`）：原版本等待 `clipboard` 主题的事件，而该主题在没有复制动作时本就静默，不是连接故障。修正（提交 `16155f180`）：订阅快照主题 `status`、`peers`、`paired-devices` 并带 nonce，以收到的真实快照帧（`status:status.snapshot`）为准；`smoke-pageprobe-v2` 通过。会话令牌不写入共享报告，运行后从 `*.control` 中脱敏。

### P8：环境变量大小写与 `NO_PROXY`（固定 stage3 包）

场景：`env-upper`（只设大写）、`env-conflict`（小写指向放行代理，大写指向不可达端口）、`env-bypass`（`NO_PROXY` 含 WebView 探测主机）、`env-bypass-other`（`NO_PROXY` 只含无关主机）。

| 运行 | 退出码 | 结果 |
| --- | --- | --- |
| `stage3/p8-portable` | 3 | `passed=true`、`functionalPassed=false`：路由全部符合，仅 2 项「用户 `NO_PROXY` 保留且加入回环」断言失败。归因：断言读了 GUI 的 `/proc/<pid>/environ`（exec 时的环境），Go `init()` 的 `os.Setenv` 只改运行时副本；探针位置错误，不是产品缺陷（同次运行中 daemon 子进程环境是合并后的值）。目录与 `ATTRIBUTION.txt` 保留 |
| `stage3/p8-portable-v2` | 0 | 断言改为读子进程（daemon）环境，要求未删除：50 项观测，8/8 要求 |
| `stage3/p8-nonportable` | 1 | 夹具失败：启动时漏设会话镜像，`secret-tool: not found`，T1 失败，没有场景运行；与产品无关。保留并写有 `ATTRIBUTION.txt` |
| `stage3/p8-nonportable-v2` | 0 | 会话镜像，56 项观测，8/8 要求，G7 无违规、无 unverified |

路由（便携与非便携一致）：仅大写变量与小写变量一样被 WebView 采用（proxied）；小写与大写冲突时跟随小写（proxied，没有走不可达端口，只作观测，优先级由解析器库决定）；`NO_PROXY` 命中探测主机时 WebView 直连（direct）而 curl 对照仍经代理，`NO_PROXY` 只含无关主机时仍经代理；用户 `NO_PROXY` 项被保留，回环名加在末尾，daemon 回环不经代理，页面 HTTP 与 WebSocket 帧都通过。

### P7：代理中断与恢复（`stage3/p7-portable`，退出码 0）

场景 `env-recover`，同一个 GUI 进程：代理在线时 WebView 经代理（proxied）；停止 tinyproxy 后请求失败（`TypeError: Load failed`），代理日志无该主机、目标没有收到请求，不直连；在同一端口重新启动 tinyproxy 后，同一进程再次经代理（proxied，目标收到 1 次）；中断后页面再次取 daemon 的 HTTP 与 WebSocket 帧（`status:status.snapshot`）仍然成功。13 项观测，6/6 要求。只跑了便携模式，且只用环境变量代理：**不证明** GNOME 设置在运行中变化（动态配置）、非便携模式或其他平台下的中断恢复；这些保留在本切片待完成，非便携在最终同包矩阵复跑，GNOME 动态场景单独做。运行证据：`stage3/p7-portable/appimage-assertions.json`、`run.log`、`done-p7-portable.rc`（0）。

### 仍未完成（OPEN，逐项增量补做）

P5 真实 Go 更新器、P6 受控 Engine rendezvous（仅拒绝型 CONNECT，不转发）、GNOME `ignore-hosts` 遗漏回环时本地 daemon 的行为、PAC / 认证 / 动态设置、Fedora、同一最终干净包上的 17c7/17c5/17c10/17c11/内容检查回归。
