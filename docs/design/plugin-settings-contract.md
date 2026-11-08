# 插件设置 Schema 契约 v1

- 状态：独立工具与固定 schema 的 macOS 表单已实现；Engine/daemon 配置接口、清单与 RPC 发布、安装器读取、Lua 热替换均未接入。
- 依据：[0010](decisions/0010-plugin-settings-schema.md)、[插件契约](plugin-contract.md)、[SDK 配置所有权](sdk-extension-contract.md#4-插件装配与配置所有权)。
- 实现：`plugin/settings`；原生 shell 模型 `app/mac/Sources/SettingsSchema`；控件 `SchemaSettingsForm.swift`。

## 1. 格式与支持范围

根必须有：

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "x-unic-settings-version": 1,
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "mode": {
      "type": "string",
      "title": "处理模式",
      "enum": ["plain", "markdown"],
      "default": "plain",
      "x-unic-ui": {
        "widget": "select",
        "order": 1,
        "group": "文本",
        "option_labels": ["纯文本", "Markdown"]
      }
    }
  },
  "required": ["mode"]
}
```

只接受下列关键字，未知关键字、非法组合和不支持版本拒绝整个表单：

| 关键字 | 语义 |
|---|---|
| `type` | 单个 string/integer/number/boolean/object/array，不支持 null 或联合类型 |
| `title`、`description` | 纯文本；不执行 HTML、脚本或链接 |
| `properties`、`required`、`additionalProperties` | 对象的字段与必填集合；每层对象必须显式 `additionalProperties: false` |
| `items`、`minItems`、`maxItems` | 列表；items 只支持基础类型，不支持嵌套数组/对象列表 |
| `enum` | 基础类型的非空唯一值列表，最多 128 项，保留值类型 |
| `default` | 有效 JSON 默认值，必须符合此节点约束；对象默认允许由其子字段默认补全 |
| `minimum`、`maximum` | number/integer 的闭区间，可只定义一端 |
| `minLength`、`maxLength` | string 的 Unicode scalar 数量，非 UTF-8 字节数、非可见字形数 |
| `writeOnly` | true 仅用于 string 秘密字段；禁止 default/enum，祖先对象默认也不得含秘密值 |
| `x-unic-ui` | 可选展示提示，见第 2 节 |

不支持 `$ref`、外部 URI 解析、pattern、format、oneOf/anyOf/allOf、条件字段、动态选项或插件自带可执行 UI。`$schema` 是静态标识，不触发网络访问。支持此 profile 不等于完整支持 JSON Schema 2020-12。

schema ≤256 KiB，节点深度≤16（根为 0）、总数≤512；配置输入/规范化结果≤64 KiB、JSON 嵌套深度≤32。对象字段名≤128 UTF-8 字节，不能为空。重复 JSON 键、无效 UTF-8、尾随 JSON、非有限数字均拒绝。

为跨端一致，数值采用 binary64，绝对值≤9007199254740991。输入数值转为 binary64 的最短十进制表示后，十进制数学值必须与输入一致，否则拒绝，不能静默舍入。例如 `1e3`、`1.00`、`0.1` 可用，`9007199254740993`、`1.00000000000000000001` 不可用。需要更大整数/精确十进制的插件应定义 string。integer 规范化为普通整数写法，便于 Go JSON 解码到 int。

## 2. UI 映射

| 描述/提示 | shell 控件 |
|---|---|
| string / `input` | 单行文本框 |
| integer、number / `inputnumber` | 数值输入，支持键入和步进；integer 拒绝小数 |
| boolean / `switch` | 开关 |
| enum / `select` | 下拉选项；数字/布尔选项不能转成字符串 |
| string + `textarea` | 多行文本 |
| writeOnly string / `password` | 密码输入 |
| object / `group` | 嵌套分组 |
| array / `list` | 可添加/删除的基础字段列表 |

`x-unic-ui` 只有 `widget`、`order`、`group`、`option_labels`：

- `widget` 可省略，按上表推导；与 type/enum/writeOnly 冲突则拒绝。
- `order` 是整数，缺省 0；同级字段按 order 升序，再按字段名升序排列。
- `group` 是同级字段的展示组名，组按有序字段中首次出现的顺序排列。对象本身仍保留层级，不扁平化配置键。
- `option_labels` 是与 enum 一一对应的字符串数组，省略或空数组时展示值本身。label 不改变实际提交值。

必填字段始终需要有效值；可选字段必须区分“未设置”与已设置的 false/0/空串。macOS 预览以“Set…”和“Remove value”表达可选值，使用原生 Form、TextField、Stepper、Picker、Toggle、TextEditor、SecureField，保持系统键盘导航和语义颜色。

GUI 不把非法数值输入悄悄改回上一有效值；编辑错误时显示字段提示并禁用保存。未知 schema 不能进入可保存表单。

## 3. Go 生成与校验

```go
type Config struct {
    Mode string `json:"mode" title:"处理模式" required:"true" default:"\"plain\"" enum:"[\"plain\",\"markdown\"]" widget:"select"`
    Size int    `json:"size" title:"最大长度" required:"true" default:"500" minimum:"1" maximum:"10000"`
}

schemaJSON, err := settings.Generate[Config]()
definition, err := settings.Parse(schemaJSON)
normalizedJSON, err := definition.Normalize(inputJSON)
err = definition.Validate(normalizedJSON)
```

省略示例中的错误处理仅为简洁；实际调用必须逐项处理。生成器读取类型与标签，不读取实例当前值。支持显式 json 字段名、基础类型、struct、基础类型 slice；拒绝匿名嵌入、指针/map/interface、递归类型、[]byte、float32、自定义 JSON/Text marshaler 和 `json:",string"`。未导出字段与 `json:"-"` 忽略。Go 整数范围与可移植数值范围取交集；标签只能收窄类型范围。

支持的标签：

| 标签 | 值 |
|---|---|
| `json` | 配置字段名；允许 omitempty/omitzero，它们不决定必填性 |
| `required`、`secret` | true/false；secret 映射 writeOnly |
| `title`、`description`、`widget`、`group` | 文本 |
| `default` | JSON 字面量，string 必须含 JSON 引号 |
| `enum`、`optionLabels` | JSON 数组 |
| `minimum`、`maximum` | 十进制数值 |
| `minLength`、`maxLength`、`minItems`、`maxItems`、`order` | 整数 |

`Parse` 取得 schema 的独立副本，`Definition` 可并发使用，`JSON()` 返回新副本。`Validate` 不插入默认值；`Normalize` 在新解析对象上只填缺失键并校验，不修改调用者数据。缺失对象只有显式对象默认才会被创建；现有对象/列表递归补默认。缺失 required 且无默认时仍报 required。null 是显式非法值，不当作缺失。

错误结构为 `{code, path}`，path 为 JSON Pointer。只使用已声明的字段名和列表索引，不回显未知键、配置值或插件自由文本。基础错误包含 required/type/range/length/items/enum/unknown_property/invalid_json；schema 错误包含 invalid_schema/unsupported_schema/limit_exceeded。GUI 可以本地映射为人类提示，不按错误文本分支。

示例 `plugins/examples/settingsdemo` 同时提供 struct、开发期生成命令、静态 schema 和可编译的处理插件。`Init` 按嵌入的静态 schema 规范化后解码配置，再检查业务条件（例如标签 UTF-8 字节限制）。模式、备注、凭据与重试字段用于演示控件，不执行外部操作；处理器只按开关/长度条件添加前缀及额外标签，正文不变。

## 4. 配置所有权与后续接入

schema 绑定插件实现及版本，配置绑定实例。同一实现的多个实例共用 schema，互不共享值。Go/builtin/Lua 使用同样的数据格式。schema 的发布、摘要与插件版本一致性须随清单/安装器一起落实；本次不新增清单或 RPC 字段。

最终配置路径是：shell 编辑 → 宿主配置接口 → schema 校验 → 插件业务校验 → 持久化/生效。必须对齐修订冲突、坏配置保留、需重启和 Lua 候选替换失败保留旧实例等既有契约。GUI 通过 schema 校验不表示业务校验、持久化或运行期生效成功。

writeOnly 是字段分类和 UI 约定，不自动加密或脱敏。当前预览无已有凭据，密码仅存内存，日志、截图数据、UserDefaults 均不输出密码。真实接口须单独约定 `is_set` 状态、保留/替换/清空操作，禁止把掩码当作真实值发回。

Engine 启用、顺序、事件、全局同步授权和标准后端策略不属于此 schema。运行时生效方式以宿主返回的结果为准，插件不能用 UI 提示自行声明热更新。

## 5. 验收与边界

Go 和 Swift 读取同一份 Go 生成 schema 与 `validation-cases.json`：校验默认值、false/0、类型、枚举、边界、嵌套未知键、列表、Unicode 长度与秘密错误。另测重复键、深度/大小、生成器不支持的类型、schema 不支持关键字和数值精度。

macOS 设置的 Plugins 页提供“Preview plugin settings…”。保存只作用于该预览会话；Cancel 丢弃草稿，Restore defaults 重新填默认值并移除可选凭据。该预览不向 daemon、TOML 或真实插件实例写入。其他 GUI shell 的渲染器和真实配置接口仍待实现。
