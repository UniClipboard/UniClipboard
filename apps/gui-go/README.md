# Go GUI 原型

Wails v3 外壳的最小端到端原型，保留 Rust daemon/Engine，复用现有 React 与 HTTP/WS 客户端。
固定版本为 `v3.0.0-beta.28`，仅本地评估，不加入生产打包或替换 Tauri。

## 编码前验证设计

| 失败方式 | 端到端检查 | 工件 |
| --- | --- | --- |
| 错连真实 profile、读写真实剪贴板或钥匙串 | 启动必须显式设置独立 profile、临时 HOME、禁用系统剪贴板；仅测试文件密钥设施 | 环境安全断言、进程与路径摘要 |
| daemon 短暂未就绪被误当不存在、重复启动 | 复用 CLI 已有发现/PID 验证；等待真实 daemon 健康；启动第二 GUI 复用同一 PID | PID/版本/健康断言 |
| 认证无法跨 Wails binding 或 bearer 泄露 | native 交换 JWT，React 直接 HTTP/WS；前端只获取短期 session；不输出凭据 | 真实认证请求成功、前端状态、脱敏日志 |
| React 仅在浏览器能运行、native binding/WS 失效 | 在真实 Wails WebView 中读取 daemon HTTP 与 WS 快照 | 原生窗口截图、前端就绪/HTTP/WS断言 |
| 多窗口不同步、关窗误停后台 | 从主窗口打开第二窗口、读取相同服务状态；退出 GUI 后检查 daemon 仍在 | 多窗口与 daemon 存活断言 |
| Go 公共代码提取导致 CLI 回归 | 编译原 CLI，运行隔离环境下已有帮助/daemon E2E；不新增单元测试 | CLI E2E结果与构建来源 |
| 原型无法复跑 | 一条命令构建与执行，记录源码/依赖/产物哈希 | manifest、assertions、SHA256SUMS |

原型只绑定最小宿主服务，不把业务逻辑搬进 Go，不复制 Engine 持久化/加密规则。
共享代码从 Go CLI 移至 `packages/desktop-host-go`，CLI/GUI 使用同一实现。
PoC 结束后只能选择继续达到完整功能验收或删除该入口；不建立长期 Tauri/Wails 双实现切换层。

## 当前范围

验证真实 daemon 的启动、认证、HTTP/WS、React 前端、多窗口与退出保留后台。
原生粘贴、GPUI 宿主、更新/签名、托盘/autostart、生产 profile 和 Windows/Linux 原生验收不在这个最小闭环中。

## 运行与复跑

在仓库根目录运行（本轮仅验证 macOS）：

```sh
bun install --frozen-lockfile
apps/gui-go/run.sh
```

启动脚本会构建原生应用包、创建 `uc-gui-go-*` 临时 HOME 与 `gui-go-*` profile。
可手工刷新会话、打开第二窗口。GUI 本身退出不会停止 daemon；启动脚本在演示结束后
通过同一隔离环境下的 CLI 停止测试 daemon，并保留临时 HOME 供排查。
禁止直接打开应用包连接生产 profile；启动守卫会拒绝非隔离配置。

```sh
apps/gui-go/e2e/run.sh target/gui-go/evidence
```

这条命令构建带 `e2e` 标签的独立应用包，在真实 WebView 中执行按钮点击，记录四份
原生窗口断言；两次启动必须复用同一 daemon PID。第一次退出后用 CLI 初始化合成
Space，再验证 CLI 状态；每次退出后检查 daemon 健康，最后停止并验证进程退出。
`assertions.json`、`native.jsonl`、`build-hashes.json` 与日志形成可复跑工件。
截图使用原生窗口捕获工具另外保存，不把浏览器开发服务器验收当成原生验收。
`e2e` 服务和自动操作不包含在普通构建中。

## 前端源码共享

`vite.config.ts` 的 `@` 直接指向 `apps/gui/src`，HTTP 客户端、生成的 SDK、WebSocket
客户端与脱敏日志代码均来自原目录，没有复制第二份。精确的 `@/lib/ipc` 别名指向
本入口的 Wails 适配，只向 WebView 返回短期 session，不返回 daemon bearer。
路径别名实现源码共享，不需要 Git 软链接；普通 Windows 检出也不会受软链接权限影响。
当前新增页面是诊断入口，尚未接入完整业务路由、所有 Tauri 插件与 native 命令。

## 验收边界

- Wails 与 runtime 同时固定为 `3.0.0-beta.28`；这是 beta 原型，不是生产迁移完成。
- daemon 的 CORS 只新增精确的 `wails://localhost` 来源，未扩大为任意来源。
- 首次启动可创建独立 daemon；已有兼容持久 daemon 会被复用；不兼容或 oneshot daemon
  会明确拒绝，不执行替换或强制结束。
- macOS SDK 的链接版本警告仍存在；本轮验证当前系统实际运行，不证明最低系统版本兼容。
- Windows/Linux、安装签名、更新、托盘、全局快捷键、原生粘贴与 GPUI 尚未验收。
