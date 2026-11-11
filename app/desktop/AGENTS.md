# 桌面应用

先读 [README](README.md)。本目录是 `engine` 的桌面宿主，Go 代码里的约束：

- `internal/` 下的包只供桌面端使用；核心（根上的 `engine`、`internal/`）不能 import 本目录，`go test ./engine` 会检查。
- 命令：`go build -o unic ./app/service/cmd/unic && go build -o unicd ./app/service/cmd/unicd`；第二个本地节点 `UNIC_HOME=/tmp/unicA ./unicd`。
- HTTP `/v1` 的响应形状是对外约定（CLI、GUI、`curl --unix-socket`），改动须同步客户端与契约；按根目录约定，发布前不保留兼容分支。普通状态不含票据，审批强制携带指纹与修订，见[身份与信任](../../docs/architecture/identity-and-trust.md)。
- 事件和日志不得携带剪贴板内容；`Info` 只写 `text` / `image` / `N files` 这类种类。
- 剪贴板适配（`../../internal/clipboardintegration/clipboard.go`）遵循契约第 8 节的观察/应用协议，改动前先读它。

- Build the graphical program with `go build -o dist/unic-desktop ./app/desktop/cmd/unic-desktop`. The service and CLI belong to `app/service/`.

- Quick-panel system interaction follows the [Desktop contract](../../docs/contracts/desktop-quick-panel-contract.md). Keep OS target identity and synthetic input in the Desktop host; service and Engine must not own them.
