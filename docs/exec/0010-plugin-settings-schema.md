# 0010 执行计划：插件设置 Schema 与表单生成

- 日期：2026-11-08。
- 工作分支：`feat/plugin-settings-schema`，基于 `main` 的 `8a1edff`。
- 决策：[0010](../decisions/proposed/0010-plugin-settings-schema.md)；[详细契约](../contracts/plugin-settings-contract.md)。
- 范围：完成可与 0004 P5/P6/P7 独立推进的工作；运行时发布/保存接口另接。

## 独立交付

- [x] 定义 schema profile、版本、支持字段/控件、默认值、错误、限额和秘密字段约定。
- [x] 独立 `plugin/settings` 标准库工具：Go struct 生成、静态加载、严格校验、默认值规范化。
- [x] 可运行 Go 插件示例和静态 schema 生成命令；示例 Init 使用同一产物校验。
- [x] macOS Foundation 模型与通用 SwiftUI 表单，固定 schema 演示八种控件；会话内保存/取消/恢复默认值。
- [x] Go/Swift 共用 schema 与校验向量，并检查生成产物是否过期。
- [x] 完成最终构建、竞态测试、原生表单截图检查并登记结果。

## 后续集成（不属于本次独立交付）

- [ ] 与 P5 对齐插件实现 ID / 配置实例 ID。
- [ ] 与 P7 对齐 schema 清单/安装/版本摘要与禁用实例读取；不执行插件发现配置。
- [ ] 配置读取/保存 API、修订冲突、秘密字段操作、TOML 原子持久化及生效结果。
- [ ] 真实实例与 Lua 热替换接入；不通过 GUI 自行修改运行期策略。
- [ ] mygo + 自定义 UI Kit 表单渲染器与相同向量验收；界面原型待定。SwiftUI 客户端路线已取消，现有 `app/mac` 表单和已完成验证仅保留为实验样例。

## 验证

```sh
go generate ./plugins/examples/settingsdemo
go test -race ./...
go vet ./...
go build -o /tmp/unic-settings ./app/desktop/cmd/unic
go build -o /tmp/unicd-settings ./app/desktop/cmd/unicd
go build -o /tmp/unic-settingsdemo ./plugins/examples/settingsdemo/cmd/plugin
swift test --package-path app/mac
app/mac/scripts/build-app.sh
UNIC_SNAPSHOT=/tmp/unic-settings-light.png UNIC_TARGET=plugin-settings app/mac/.build/debug/Unic
UNIC_SNAPSHOT=/tmp/unic-settings-dark.png UNIC_TARGET=plugin-settings UNIC_DARK=1 app/mac/.build/debug/Unic
```

2026-11-08 验证结果：

- `go generate ./plugins/examples/settingsdemo` 成功；测试确认嵌入产物与 Swift 资源均匹配配置定义。
- `go test -race ./...`：27 个包、282 项测试通过。
- `go vet ./...` 通过；桌面两个入口和示例插件构建通过。
- `swift test --package-path app/mac`：6 项测试通过，其中 Go/Swift 共用 19 组配置向量；覆盖打包资源读取与资源丢失时安全报错。
- `app/mac/scripts/build-app.sh`：release 构建和 ad-hoc 签名成功。schema 安装到 `Contents/Resources/SettingsSchema`，不依赖开发机 SwiftPM 路径。
- 检查浅色/深色表单渲染截图：`/private/tmp/unic-settings-light.png`、`/private/tmp/unic-settings-dark.png`。模型测试与截图检查通过；应用未获得 UI 自动化授权，未完成真实点击的保存/取消交互实测。
- `git diff --check` 与新增文档相对链接检查通过。

宿主：macOS 15.6.1，Xcode 26.3（17C529），macOS SDK 26.2。截图只证明当前宿主的控件布局，不代替 macOS 26/27 外观或辅助功能实测。当前独立交付已完成，上述后续集成项保持未完成。
