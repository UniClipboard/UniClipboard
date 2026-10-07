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

### P6：Engine rendezvous 出站（受控、仅拒绝，`stage3/p6-portable-v2`，退出码 0）

源码对齐：打包的 daemon SHA `ea0f0bcb…`，构建证据 Desktop `be709456e27c…`、`uc-engine 1.1.0-rc.22`、Engine rev `d4dd324a1a88…`；rendezvous 基址 `https://rendezvous.uniclipboard.app`（`uc-infra-p2p`，默认配置的 `reqwest::Client`，读取环境代理）。

探针：对隔离的真实 daemon 临时 profile 经真实协议（`/auth/connect` 的 Bearer 换会话）发一次 `POST /v2/setup/redeem`，邀请码与口令都是合成的；响应是 HTTP 200 封装，join 结果在 `data.status`（本探针看到 `pending`，没有等待最终结局）。网络是 `--internal`，代理只有拒绝型（记录 CONNECT 后 403，从不转发），rendezvous 名在 `/etc/hosts` 与证书 SAN 中指向受控内部目标作为附加控制。

| 场景 | 观测 |
| --- | --- |
| `rv-deny`（环境代理） | 代理日志 `CONNECT rendezvous.uniclipboard.app:443` 并 `Proxying refused on filtered domain`；目标未见请求或握手：daemon 的 rendezvous 请求走了环境代理且没有转发 |
| `rv-none`（无代理变量） | 代理日志无该主机；受控目标收到 daemon 的 TLS 握手（`tlsv1 alert unknown ca`，daemon 不信任测试 CA）：直连被观测到 |
| `rv-bypass`（`NO_PROXY` 含该主机） | 同上：代理未点名，目标收到握手：`NO_PROXY` 绕过有效 |

边界：握手以 `unknown ca` 结束，只证明到达 TLS 握手阶段，不证明请求完成或代理成功；只证明 rendezvous 这一条 Engine 出站，iroh 其他出站另有结论，不外推。首轮 `p6-portable`（rc 1，探针把 HTTP 200 封装当错误）保留并有 `ATTRIBUTION.txt`。

### P5：Go 更新器（`stage3/p5-portable-v2`，退出码 0）

入口是真实的 `check` 控制命令（手动检查的同一条代码路径，`update.NewHTTPClient()` = `ProxyFromEnvironment`），feed 是受控内部 HTTPS 目标 `update-feed.test`（Tauri 格式，宣告 `9999.0.0`，只检查不下载，下载与签名回归留在最终 P9 的隔离流程）。更新器使用自己的主机名和自己的代理日志窗口，不以 curl 或 WebView 替代。

| 场景 | 更新器路由 | 检查结果 | 目标看到 | 同场景 WebView |
| --- | --- | --- | --- | --- |
| `up-none` | direct | found | 1 | direct |
| `up-allow` | proxied（CONNECT 点名 feed 主机） | found | 1 | proxied |
| `up-deny` | refused | 失败 | 0 | refused |
| `up-dead` | failed | 失败 | 0 | failed |
| `up-bypass`（`NO_PROXY` 含 feed 主机） | direct | found | 1 | proxied |

45 项观测，25/25 要求。更新器只读环境变量（与 Tauri 更新器同为仅环境变量的对等行为），本轮没有测试它是否读取 GNOME 设置；改为通过 GIO 读取系统代理是 OPEN 的产品决策，没有实现。首轮 `p5-portable`（rc 3）是 fixture 缺陷：E2E 包构建时 `-X main.updaterPublicKey=` 为空，`updateClient()` 在任何 HTTP 之前返回“updates are disabled”，我只设了 `UC_UPDATE_ENDPOINT` 没设 `UC_UPDATE_PUBKEY`；原目录与 `ATTRIBUTION.txt` 保留，v2 提供了 17c5 feed 的公钥与真实签名串。只跑便携模式。

### GNOME `ignore-hosts` 为空时的回环缺陷（stage3 RED）与 loopback guard

证据（RED，保留）：`stage3/gnome-ignore-portable-v2`（退出码 1，`dconf update` 退出码 0，curl 控制与回环探测控制有效）：`gs-sys-empty` 中外部 WebView 请求 proxied，但 WebView 到 daemon 的回环连接为 0，代理日志出现 `GET http://127.0.0.1:<daemon>/auth/connect`、`/settings` 与 `ws://127.0.0.1:<daemon>/ws?auth=Session …`（会话令牌随 URL 发给了代理），页面 HTTP 取数经代理得到 200，但 WebSocket 失败。这是产品缺陷：GNOME 解析器只绕过用户 `ignore-hosts` 列出的主机，空列表时本地 daemon 的连接被送给用户的代理。注意：Tauri 的 AppImage 从未携带 GNOME 解析器（17c11 基线只有 `libgiognutls`），所以不曾走到这一边界；这是 R1 打包解析器的后果，不是相对 Tauri 的回归，也不声称与 Tauri 行为对等。

来源（官方，逐条核对）：
- WebKitGTK 2.52.6（构建镜像实际版本）`Source/WebCore/platform/network/soup/SoupNetworkSession.cpp`：默认模式使用 `g_proxy_resolver_get_default()`；自定义模式用 `g_simple_proxy_resolver_new` 重新构造（只有默认代理、`ignore_hosts`、按 URI 的映射）；无代理模式把解析器置空；没有任何回环特判。
- glib-networking 2.80.0：`gnome` 解析器优先级 80，`ignore-hosts` 交给 `GSimpleProxyResolver`，无回环特判，PAC 经 D-Bus 的 PACRunner；`libproxy` 解析器优先级 10。
- GLib 2.80.0 `gio/giomodule.c`：扩展点按优先级从高到低选第一个可用实现（`GIO_USE_PROXY_RESOLVER` 可按名字指定）。
- Wails v3.0.0-beta.28：没有代理 API（见前文调查）。

为什么不用现成的 WebKit/Wails 会话 API：`webkit_network_session_set_proxy_settings` 的 `CUSTOM` 模式用静态 `GSimpleProxyResolver` 替换系统解析器，会丢掉 GNOME 设置、PAC、认证与动态变更；`DEFAULT` 是全有或全无。解析发生在 `WebKitNetworkProcess`，不是 Go 进程，所以进程内注册解析器无效；该进程唯一可被应用控制的入口是它加载的 GIO 模块目录（AppRun 已把 `GIO_MODULE_DIR` 指向随包目录）。

选择：随包一个很薄的 GIO 扩展模块 `libgiouniclipboardloopback.so`（源码 `apps/gui-go/packaging/linux/gio-loopback-guard/uc_loopback_guard.c`，随包时在构建镜像内用 `gcc` 对 `gio-2.0` 编译，清单记录源码 SHA-256、编译器与命令），优先级 100，高于 `gnome`(80) 与 `libproxy`(10)。它只做一件事：URI 主机属于 `localhost`、`127.0.0.0/8`、`::1`（用 GLib 自己的 `GSimpleProxyResolver` 匹配器判断，不自写匹配）时返回 `direct://`；其余 URI 全部委托给「没有它时 GLib 本会选择的那个解析器」（按优先级列举扩展点实现，跳过优先级 ≥100 的自身，取第一个 `is_supported` 的实例并缓存），所以 GNOME 手动代理、`ignore-hosts`、PAC、认证与动态变更仍由原解析器处理，不复制任何配置解析。产品里保留的 `NO_PROXY` 合并是另一道保护：它保护 Rust daemon（reqwest 没有自动回环绕过）和环境变量路径；本模块保护 WebKit 的 GNOME 路径，二者不重复。

最小故障契约（**写于代码与两次临时烟测之后、真实 AppImage E2E 之前**：先写了 C 源码并用构建镜像做过两次未入库的临时命令行烟测——仅 libproxy 环境变量后端，同步查询：回环 URI 返回 `direct://`、外部 URI 返回环境代理；GNOME 后端未被烟测选中（keyfile 后端没有选到 gnome 解析器），异步、取消、错误路径未做烟测；这些不当作验证，也不补写单元测试，验证只来自下面的 E2E）：

| 失败方式 | 契约 | 验证 |
| --- | --- | --- |
| 初始化时选到自身造成递归 | 不调用 `g_proxy_resolver_get_default()`；列举扩展点并跳过优先级 ≥100 | 模块加载进真实网络进程后外部请求仍按系统解析器（proxied/refused/failed）而不是挂起 |
| 没有可用的下游解析器 | 非回环 URI 返回 `direct://`（与 GLib 无解析器时一致） | 仅记录；本包总有 libproxy，未做 E2E，边界如实写明 |
| 同步 / 异步 / 取消 / 错误 | 回环：同步与异步都立即得到 `direct://`；非回环：取消令牌与错误原样交给下游；`lookup_finish` 用 `g_task_is_valid` 区分自己的与下游的结果 | libsoup 3 走异步路径：WebView 的外部 HTTPS、本地 HTTP 与 WebSocket 都经过它；取消与错误路径没有专门注入，边界如实写明 |
| URI 解析：`localhost`、IPv4 回环段、`::1`；伪装成回环的非回环主机 | 回环名与地址直连；`localhost.<域>` 等伪装主机不得直连 | `gs-sys-empty` 中增加伪装主机 `localhost.webview-probe.test` 的 WebView 请求，必须 proxied；IPv6 `::1` 没有专门 E2E，边界如实写明 |
| 进程与模块路径、helper 隔离 | 模块只在随包的 `GIO_MODULE_DIR` 里；宿主的 GIO 模块不被加载；helper 子进程的环境清理沿用 17c10/17c11 的规则 | 内容检查（模块集合与字节）、G7 映射检查、非便携矩阵 |

验收（stage4）：同一组 `gs-sys-allow`、`gs-sys-ignore`、`gs-sys-empty`，`gs-sys-empty` 外部 proxied，WebKit 到 daemon 的回环连接 ≥1，代理日志没有回环行，页面 HTTP 与 WebSocket 帧成功；再在同一个 stage4 包上完整复跑全部环境变量、P5–P8、便携与非便携矩阵及 TLS/便携/helper/内容/更新回归（stage3 的绿色不转移）。

### stage4：loopback guard 的 RED → GREEN（同一场景，不同包）

| | stage3（RED，保留） | stage4（GREEN） |
| --- | --- | --- |
| 包 | `8c9881b025457aa8…ea75`，产品 `e649e7057` | `45932fc7b6f6463c9ec1cfef7793b3aed1e50693404ef74f0a65c3db815373cf`，来源 HEAD `679bf0082a82d2eda0d1270d74a396dff7abffb1`，`dirty.diff` 为空，`build.rc`/`package.rc`/`content.rc` 均为 0，daemon `ea0f0bcb…` 未变 |
| 运行 | `stage3/gnome-ignore-portable-v2`（退出码 1） | `stage4/gnome-ignore-portable`（退出码 0，28 项观测，7/7 要求） |
| `gs-sys-empty` | 外部 proxied；WebKit→daemon 回环连接 0；代理日志含 daemon 的 `/auth/connect`、`/settings`、`ws://…`；页面 WebSocket 失败 | 外部 proxied；WebKit→daemon 回环连接 4；代理日志里只有回环探测对照 `/loopctl`；页面 HTTP 与 WebSocket 帧成功；伪装主机 `localhost.webview-probe.test` proxied |
| `gs-sys-allow` / `gs-sys-ignore` | 通过 | 通过（proxied / direct，daemon 可用） |

包内 GIO 模块现为 5 个（新增 `libgiouniclipboardloopback.so`，源码 SHA-256 与 `gcc -Wall -Wextra -Werror` 命令在清单里）；内容检查 26 项通过。

证据边界（精确未验，`boundary-portable` 结果见下表之后）：同步路径之外，异步、取消令牌与错误传播没有专门注入；「无下游解析器」的回退没有运行；IPv6 `::1`、`127.0.0.0/8` 非 `127.0.0.1` 成员与 `localhost` 的 E2E 由随后的 `loopback boundary` 增量与 live maps（网络进程已映射该模块）补充，结果见下一节；宿主 helper 进程没有带出该模块由 17c10/17c11 helper 回归检查（环境清理规则）。最小 GREEN 不等于完整代理支持。

### PAC / 认证 / 动态设置：源码调查与下一步最小场景（尚未实测）

调查（glib-networking 2.80.0 `proxy/gnome/gproxyresolvergnome.c`，官方源码）：
- **PAC（`mode='auto'`，`autoconfig-url`）**：GNOME 解析器不在进程内求值，而是经 **用户会话总线** 调用 `org.gtk.GLib.PACRunner`（`/org/gtk/GLib/PACRunner`）；服务不可用时只发 `g_warning ("Could not start proxy autoconfiguration helper … Proxy autoconfiguration will not work")`，解析器继续工作。含义：PAC 依赖宿主的 PACRunner D-Bus 服务（总线激活，随 glib-networking 的服务组件安装）和一个会话总线，二者都不在 AppImage 里；便携模式的验证容器没有会话总线。这是已知的潜在缺口，不是已证实的缺陷，需要实测。
- **认证**：`use-authentication` 为真时，从 `org.gnome.system.proxy.http` 读 `authentication-user` / `authentication-password`，URI 转义后拼进 `http://user:password@host:port`；认证由 libsoup/WebKit 以该 URI 的凭据发出。
- **动态变更**：解析器连接 `GSettings::changed`，置 `need_update`，下一次查询前懒更新；因此运行中的进程理论上随设置变化，而不需要重启（dconf 的变更传播另受后端影响，需实测）。
- libproxy（`libgiolibproxy`，优先级 10）经 `libpxbackend` 评估 PAC（duktape）并读取环境变量与 GNOME 配置，不需要 PACRunner；选择哪个解析器由优先级与 `GIO_USE_PROXY_RESOLVER` 决定。

下一步最小场景（各自是一个独立增量，沿用现有 runner，不新建框架；失败原因先归因再决定产品改动）：
1. `gs-sys-pac`：系统 dconf `mode='auto'`、`autoconfig-url` 指向受控 PAC（对 WebView 主机返回 `PROXY 127.0.0.1:<port>`，其余 `DIRECT`），便携与非便携各一次；观测外部请求路径。预期风险：便携模式无会话总线、PACRunner 不存在 → PAC 不生效。若证实，候选成熟方案：随包带 `glib-pacrunner` 并在 AppRun 中以应用自己的会话总线启动，或在检测到 `auto` 时让 libproxy 解析器接管（`GIO_USE_PROXY_RESOLVER`/优先级），二者都不自写 PAC 引擎，选择依据实测与依赖成本。
2. `gs-sys-auth` 与 `env-auth-*`：tinyproxy `BasicAuth`；正确凭据 → proxied，错误凭据或缺失 → 不得直连（refused/failed）；GNOME 的 `authentication-*` 键与环境变量 URL 内凭据各一组；Go 更新器同样检查（Go 的 `ProxyFromEnvironment` 读取 URL 内凭据）。
3. `gs-user-dynamic`（非便携，真实会话总线与 `dconf-service`）：同一 GUI 进程内，请求 1 proxied → `gsettings set … mode 'none'` → 请求 2 direct → 设回 `manual` → 请求 3 proxied；记录是否需要重启；系统数据库（`dconf update`）的变更传播另做一次观测。
4. Fedora：基底镜像 `uc-gui-go-linux-runtime-fedora:17c7` + tinyproxy、dconf、`libproxy-bin`、iproute（`Dockerfile.17c12-fedora` / `-fedora-session`，Fedora 44 的 libproxy 0.5.12 与 glib-networking 2.80.1，与 Ubuntu 的 0.5.4 不同；AppImage 自带模块，宿主版本只是对照）；runner 里 tinyproxy 的 `Group nogroup` 在 Fedora 上要改为 `nobody`。

### P5 在 stage4 包上的结果（带尝试证明）

`stage4/p5c-portable`（退出码 0，53 项观测，38/38 要求）与 `stage4/p5c-nonportable`（退出码 0，59 项观测，38/38 要求）：`up-none` direct、`up-allow` proxied、`up-deny` refused（代理点名并拒绝，目标 0 次）、`up-bypass` direct；`up-reset`（代理接受连接后立即关闭）更新器失败、目标 0 次、代理收到更新器的 1 个连接，证明尝试而不是被禁用；每个场景都证明更新器已启用（端点与非空公钥在环境里，`up-none`/`up-allow` 成功证明密钥有效）。此前的 `stage4/full-*`（7 项 up-* 失败，缺 `feed-inputs` 的夹具错误）、`p5-*`（缺尝试证明）与 `p5b-*`（runner 配置生成缺陷）保留并带 ATTRIBUTION.txt；这些汇总早于完整性判定，最终整体验收用新判定重跑。

### PAC 的自包含：随包的 `glib-pacrunner`（RED 与设计契约，写于实现之前）

RED（保留）：`stage4/pac-nohelper-portable` 与 `stage4/pac-nohelper-nonportable`（退出码都是 3，`passed=true`、`functionalPassed=false`）：宿主没有 `glib-pacrunner`（仅在该场景的容器内把它改名）时 `gs-sys-pac` 的外部 WebView 请求是 `failed`（目标 0 次，没有直连逃逸，回环仍直连，页面 HTTP/WS 正常），GUI 日志：`Could not start proxy autoconfiguration helper: Error calling StartServiceByName for org.gtk.GLib.PACRunner … Proxy autoconfiguration will not work`。同一包在宿主有该服务时（`stage4/pac-*`，退出码 0）PAC 通过，但这只是依赖宿主组件的绿，**不能外推为自包含支持**；不能要求用户安装宿主依赖，也不能记为不适用。这是 R1 的打包/集成缺口。

选择：随包 glib-networking 自己的 `glib-pacrunner`（构建镜像里 `glib-networking-services` 2.80.0，依赖 `libproxy.so.1` 已随包）；由 Go 宿主在 Linux 启动早期管理它。成熟性：这是 GNOME 解析器官方使用的同一个 PAC 助手，PAC 求值仍由 libproxy（duktape）完成，不自写 PAC 引擎，不改解析器。

运行环境与行为契约（官方源码 `glibpacrunner.c`：`g_bus_own_name(G_BUS_TYPE_SESSION, "org.gtk.GLib.PACRunner", NONE)`，丢失名字则退出，没有空闲超时）：
- 使用现有会话总线（`DBUS_SESSION_BUS_ADDRESS`，或 godbus 的标准回退/自动启动），**不创建、不抢占真实用户的总线，也不改变宿主服务**：先问总线 `ListActivatableNames` 与 `NameHasOwner`：宿主已提供可激活服务时，什么都不启动（由总线激活宿主的那个）；只有没有宿主服务时才启动随包的助手。
- 没有会话总线（`SessionBus` 失败）：不启动，记录原因；PAC 照旧不可用并明确失败，不会静默直连。
- 多实例：每个 GUI 实例各自启动一个助手；总线名字用默认标志排队，后到者排队等待，先到者退出后接任；助手随各自的 GUI 退出。
- 退出与清理：子进程设 `Pdeathsig=SIGTERM`（GUI 崩溃也会结束它），GUI 正常退出时显式终止；不留孤儿，不写任何持久化数据。
- 竞态：在创建 WebView 之前同步启动，并等到总线上有人拥有该名字（上限 2 秒）；超时只记录，不阻塞启动。
- 异步/取消/错误边界：助手的 D-Bus 调用由 GNOME 解析器（glib-networking）发出并处理取消与错误；本次不改它们，也不在 Go 里复制。解析器在助手不可用时已经表现为请求失败而不是直连（RED 里已观察到）；助手中途退出的恢复（解析器是否重连）**未验证**，列入后续验收，不在本片之后留成无期限的 OPEN：本片最终矩阵内增加「助手被杀后新请求」观测并按结果处理。
- 打包：`usr/libexec/glib-pacrunner` 由 `glib-networking-services` 复制，`dpkg -S` 校验归属，NEEDED 闭包检查，清单记录 SHA-256；内容检查加入。

失败方式与验收：(1) 宿主有服务时我们误启动 → 抢占；验收：宿主有助手的运行里随包助手进程数为 0；(2) 无服务无总线 → 不启动且不崩溃；(3) 无宿主助手、有会话总线（便携自动启动与非便携）→ `gs-sys-pac-nohelper` 外部 proxied；(4) 回环仍直连（PAC 把回环也指向代理）；(5) GUI 退出后无 `glib-pacrunner` 残留；(6) 宿主 helper 子进程（17c10/11）不带出助手的环境。

#### 审阅后修订：PAC 助手的启动预算、所有权与生命周期（修订契约，写于改代码之前）

对 `838e56cd5` 的 `pacrunner_linux.go` 的审阅发现四个边界问题，修订如下（首版的 stage5 运行继续，不改动；修订后清洁重建同一个包再实证）：

1. **无上限的总线调用**：首版的 `ListActivatableNames` / `NameHasOwner` 用无上下文的 `Call`，2 秒只包住外层的睡眠循环；一次卡住的调用可以无限阻塞 `init`，即无限阻塞启动。修订：连接用 `dbus.ConnectSessionBus(dbus.WithContext(ctx))`，每次调用用 `CallWithContext`，整条启动路径共享一个总预算（3 秒）；`init` 只等「就绪或放弃」信号，超过预算就记录并继续启动，PAC 照旧明确失败。证据不是日志里的 2 秒，而是真实 E2E：总线地址指向不存在的路径（`gs-sys-pac-nobus`）时 GUI 启动延迟有界。
2. **已有所有者但不可激活**：首版只看可激活列表。修订：启动时若总线上已有该名字的所有者（宿主服务、或其他实例的助手、或手动起的助手），**不启动** 任何进程、不抢占（不带替换标志）；之后通过 `NameOwnerChanged` 监视该名字，所有者消失且宿主不可激活时再启动随包助手（接任）。验证：`gs-sys-pac-owned`（非便携，手动起一个所有者，宿主服务文件在该容器内改名使其不可激活）：随包助手数为 0 且 PAC 成功；杀掉所有者后随包助手接任且 PAC 恢复。
3. **助手崩溃或被杀后的恢复**：同一个监视在名字所有者变空时重启随包助手，限频（60 秒内最多 5 次，超出则记录并放弃），GNOME 解析器按名字（不是唯一连接名）调用，新所有者出现后无需重连。验证：`gs-sys-pac-kill`：运行中 `kill -9` 助手，随包助手被重新拉起，之后的新 WebView 请求 proxied。
4. **`Pdeathsig` 绑定的是创建子进程的线程，不是进程**：首版注释「init 跑在主线程，主线程与进程同寿」是假设。修订：所有子进程都由一个 `runtime.LockOSThread()` 且永不解锁、永不退出的监视 goroutine 创建，让创建线程的寿命等于进程寿命；注释据此改写。验证：GUI 正常退出（`exit`）与强制终止（`SIGKILL`）后，随包助手进程都在 8 秒内消失；这是实测，不是源码推断。
5. **没有会话总线**：连接失败 → 记录并放弃，PAC 明确失败（外部请求 failed，不直连，GUI 其余功能不受影响）。这不是「自包含的 PAC 支持」：GNOME 的 PAC 路径本身需要会话总线，真实桌面总有；无总线时不支持，并按此记录。

### 回环边界与 live maps（`stage4/boundary-portable`，退出码 0，仅便携，分项结果）

场景 `gs-sys-allow`、`gs-sys-ignore`、`gs-sys-empty`，28 项观测，15/15 要求。`gs-sys-allow`（GNOME 默认 ignore-hosts）与 `gs-sys-empty`（空 ignore-hosts）里，真实 WebView 访问三个各自独立的真实监听器：`127.0.0.2`（`127.0.0.0/8` 中不是 `127.0.0.1` 的成员）、`localhost`、`::1`，监听器都直接收到请求（各 1 次），代理日志没有点名；同一场景里外部请求仍 proxied，伪装主机 `localhost.webview-probe.test` 在 `gs-sys-empty` 里 proxied；`/proc` maps 证明 WebKitNetworkProcess 已映射 `libgiouniclipboardloopback.so`。边界：这是便携模式的分项，不是完整矩阵，也没有非便携和 Fedora；宿主 helper 没有带出该模块、异步/取消/错误传播、无下游解析器的回退仍未验证。

### 运行器的完整性判定（修改前写下的失败方式）

触发：`stage4/p5b-*` 因为配置生成错误在第二个场景中途抛出异常，结果却是 `passed=false`、`functionalPassed=true`——只有 `up-none` 的要求被评估，其余场景没有执行，被当成「没有失败」。`functionalPassed` 必须表示「所选场景全部执行完且没有错误，并且所有要求成立」。

这个判定自身可能的失败方式：(1) 把合法跳过的场景（非便携模式下 `gs-ph-*`）算成未完成，导致永远不通过——跳过必须有记录的原因并按「已处理」算；(2) 提前停止（`StopScenario`，如 T0 夹具失败）没有留下未完成标记，仍被判成功；(3) 场景在要求评估之后、清理之前抛错，要求全真但场景不完整；(4) 选了场景但一个要求都没有（`--require` 下为空集）被判成功。判定：每个场景末尾写 `completed=true`；`functionalPassed = 有要求 ∧ 要求全真 ∧ 无 error ∧ 所选场景都 completed 或带跳过原因`。真实负控制：缺少 `feed-inputs` 时 `up-*` 在 T0 直接停止，结果必须是 `functionalPassed=false`；正控制是完整重跑。不写事后单元测试。

### 仍未完成（OPEN，逐项增量补做）

GNOME `ignore-hosts` 遗漏回环时本地 daemon 的行为、PAC / 认证 / 动态设置、Fedora、同一最终干净包上的 17c7/17c5/17c10/17c11/内容检查回归。
