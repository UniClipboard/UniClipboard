# 0010 · 插件设置 Schema 与各端表单生成

- 日期：2026-11-08。
- 状态：已采纳；独立 schema 工具和 macOS 预览已实现，运行时配置 API、插件清单/RPC 与安装器集成待主线接入。
- 关联：[0004 插件](0004-clipboard-pipeline-lua.md)、[0005 布局](../archive/0005-monorepo-layout.md)、[0006 macOS](../archive/0006-macos-swiftui-client.md)、[0008 实例与身份](../archive/0008-device-identity-and-backend-bindings.md)。

## 决策

插件提供静态的配置 schema，GUI shell 读取相同 schema 并生成平台原生表单。schema 描述配置定义，不从当前配置值推断类型、枚举或约束。一个插件实现共用一份 schema，每个配置实例保存各自的值。

采用 JSON Schema 2020-12 的明确子集，并以 `x-unic-settings-version: 1` 标识本项目支持的 profile，以 `x-unic-ui` 补充控件、顺序、分组、选项标题。不支持的关键字/版本必须拒绝，不能把未实现的校验规则静默忽略。精确定义见[设置契约](../../contracts/plugin-settings-contract.md)。

Go 插件可从配置 struct 与标签生成静态 JSON；Lua 和其他实现可直接提供同样的 JSON。生成器是插件作者的开发工具，发布时附带生成产物；安装器和 GUI 不执行第三方程序来发现设置。builtin 也使用相同格式。

`plugin/settings` 是独立公开工具包，只依赖标准库，不依赖 Engine 或 GUI。GUI 的校验只提供即时反馈；配置入口最终按相同 schema 校验，插件 `Init` 再做业务校验。默认值通过显式 Normalize 填入，只填缺失字段，不覆盖 false、0、空字符串或 null。

> 2026-11-09 更新：SwiftUI 客户端路线已取消，`app/mac` 的表单仅保留为实验样例；桌面表单集成改由 mygo + 自定义 UI Kit 承载，界面原型待定。Schema、校验与秘密字段契约继续适用。

首个实验 shell 实现在 `app/mac`：Foundation 模型与 SwiftUI 控件分离，使用 Go 生成的固定 schema 和 Go/Swift 共用测试向量。预览保存仅在会话内生效。运行时接入前，不修改 `plugin.Spec`、RPC 握手、后端实例装配或 TOML 持久化。

启用/停用、事件订阅、顺序和 Engine 标准同步策略继续由宿主统一管理，不放进插件专属 Config schema。秘密字段通过 `writeOnly` 声明，使用密码控件，不允许默认值或枚举；真实配置接口须另行实现已设置状态以及保留/替换/清空操作。

## 后果与验收

各端实现一次通用渲染器即可展示新增插件字段。第一版限制为基础字段、嵌套对象和基础类型列表；条件显隐、引用、动态选项、任意代码 UI 和网络校验不在本次范围。

schema 生成、加载、配置校验和 GUI 预览可以与 P5/P6/P7 并行。最终接入需要对齐实例 ID、配置修订/持久化、生效状态与安装清单；这些入口不能借本次预览宣称已完成。执行与验证见[执行计划](../../exec/0010-plugin-settings-schema.md)。
