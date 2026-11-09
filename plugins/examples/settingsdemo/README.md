# 设置 Schema 示例

`Config` 是配置定义的来源。标签描述字段约束与 UI；静态 JSON 供各端读取，插件 `Init` 使用同一 JSON 校验和填默认值。生成工具只在开发期运行：

```sh
go generate ./plugins/examples/settingsdemo
go build -o /tmp/unic-settingsdemo ./plugins/examples/settingsdemo/cmd/plugin
```

命令同时更新本目录的 `settings.schema.json` 与 macOS 预览资源，Go 测试会检查二者是否与 struct 一致。查看生成结果：

```sh
go run ./plugins/examples/settingsdemo/cmd/schema
```

宿主可用现有 `engine.PluginInstance{Plugin: &settingsdemo.Plugin{}, Config: ...}` 或将构建产物作为外部处理插件装配。未修改发行版默认装配；安装器/schema 自动发现仍未接入。

处理器只给长度符合要求的文本添加标签，不改写正文。prefix/max_length/enabled/tags 参与处理；mode、notes、token、retry 用于展示其余控件，不访问外部服务。秘密字段没有默认值，不写日志或磁盘。

macOS 原型：Settings → Plugins → Preview plugin settings…。支持字段即时校验、Save preview、Cancel、Restore defaults；保存仅存在当前预览会话中。详情见[契约](../../../docs/contracts/plugin-settings-contract.md)。
