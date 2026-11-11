# unic

跨设备剪贴板同步。点对点(WireGuard 隧道,无账号、无控制服务器),新设备必须由用户批准并核对指纹。

这是一个 monorepo:一个平台无关的 Go 核心,加上各平台的应用。桌面界面（含 macOS）统一采用 mygo + 自定义 UI Kit，界面原型待定。

## 目录

| 路径 | 内容 |
|---|---|
| [`engine/`](engine/) | 公开 Go SDK:生命周期、配对与信任、内容与租约、发送与接收、剪贴板观察/应用协议。各端共用的入口,契约见 [docs/contracts/engine-contract.md](docs/contracts/engine-contract.md) |
| `internal/` | 核心实现:`session`(会话层)、`wire`(线协议)、`store`(SQLite)、`tcnet`(tailcat 传输)、`rendezvous`(配对码服务)、`clip`(剪贴板内容模型)等；另有独立迁移工具 `v1migration`，包含平台专属凭据读取 |
| [`app/service/`](app/service/) | UI-free CLI, Engine service and offline migration tool. |
| [`app/desktop/`](app/desktop/) | 桌面应用(Go,mygo):`unicd` daemon、`unic` cli 与 gui |
| [`app/android/`](app/android/) | Android 应用(尚未开始) |
| [`app/ios/`](app/ios/) | iOS 应用(尚未开始) |
| `bindings/` | 将来:把 `engine` 暴露给 Kotlin / Swift 的薄封装(gomobile 等) |
| [`docs/`](docs/) | 设计决策与接口契约 |
| `prototype/` | GUI 交互原型 |

依赖方向只有一个:`app/*` 和 `bindings/` 依赖核心,核心不依赖任何 `app/*`。公开的 `engine` 包不引入 GUI 工具包、文件监听或配置文件解析,`go test ./engine` 里有测试守住这一点。当前布局与依赖约束见[仓库总览](docs/architecture/overview.md)。

## 开发

```sh
go test -race ./...                                              # 核心与桌面端的全部 Go 测试
go build -o unic ./app/service/cmd/unic
go build -o unicd ./app/service/cmd/unicd
go build -o unic-migrate ./app/service/cmd/unic-migrate
go build -o unic-desktop ./app/desktop/cmd/unic-desktop
```

各端的构建、运行和约定见各自目录下的 README。国内网络下载依赖慢时:`GOPROXY=https://goproxy.cn,direct go mod download`。

### 代码检查

使用 [golangci-lint](https://golangci-lint.run/docs/welcome/install/local/)，版本固定在 `.golangci-lint-version`（本地与 CI 保持一致）：

```sh
curl -sSfL https://golangci-lint.run/install.sh | sh -s -- -b ./bin "$(cat .golangci-lint-version)"
./bin/golangci-lint run ./...
./bin/golangci-lint fmt --diff             # 检查格式，非零退出表示需要格式化
./bin/golangci-lint fmt                    # 自动整理 gofmt / imports
```

`.golangci.yml` 启用错误处理、vet、静态分析、无效赋值、未使用代码、拼写和 nolint 注释检查。PR checks 在 PR、主分支推送及合并队列中执行全量 lint、格式检查、`go test -race ./...` 和桌面入口的构建；Go 版本读取 `go.mod`，使用 `ubuntu-latest` Linux runner。需要强制合并门禁时，在 GitHub 分支保护中将 `Lint` 和 `Test and build` 设为必需检查。


本机 UniClipboard v1 的历史、图片、文件与标签可通过 `unic migrate` 导入。支持新数据目录和已有历史合并，操作见[桌面迁移说明](app/desktop/README.md#从本机-uniclipboard-v1-迁移)。

## Independent distributions

`make package-headless` produces an archive with `unic`, `unicd` and the offline `unic-migrate` helper.
`make package-desktop` produces both archives and adds `unic-desktop` to the desktop archive.
Both archives reuse the same CLI and service binaries. Output includes a SHA-256 manifest.
Archives are unsigned; platform installers and service registration remain separate.

Versioned CLI candidates use `go run ./tools/package -product cli`.
The CLI workflows assemble exact npm platform packages and expose `unic` and `uniclip` through the same launcher.
Alpha and stable use external npm dist-tags. Promotion reuses the accepted packages.
See [product builds and CLI releases](docs/architecture/releases.md) for commands, acceptance and publication prerequisites.
See [headless architecture](docs/architecture/headless.md).

## License

unic is licensed under the [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0-only).
