# 0029 · 插件展示文本多语言

- 状态：已实现（清单、Schema、本地服务 DTO 与桌面渲染）；待用户视觉确认与原生窗口验收。
- 日期：2026-11-22。
- 关联：[0010 插件设置 Schema](0010-plugin-settings-schema.md)、[0028 桌面插件市场](0028-desktop-plugin-marketplace.md)。
- 落地目标：[插件契约](../../contracts/plugin-contract.md)、[插件设置 Schema 契约](../../contracts/plugin-settings-contract.md)、[本地服务契约](../../contracts/local-service-contract.md)、[桌面运行时架构](../../architecture/desktop-runtime.md)。

## 背景

桌面端用中文原文做目录键翻译自身文案，不能翻译插件提供的内容。已安装插件在市场里查不到时，标题直接显示插件 ID，副标题又重复一遍 ID 与版本。Schema 的 `title`、`description`、`group`、`option_labels` 只能写一种语言。

## 决策

1. 清单增加可选展示字段：`name`、`summary` 与 `[i18n.<语言>]` 表（仅 `name`、`summary`）。它们是纯文本，不进入插件 ID、握手 Spec 或摘要比较，修改它们不需要重启。
2. `x-unic-ui` 增加 `i18n`：按语言覆盖 `title`、`description`、`group`、`option_labels`。无标签的原字段是默认文本。Go 生成器支持 `i18n:"{...}"` 标签。
3. 语言键是 `en`、`ja`、`pt-BR` 这类标签，最多 16 种。查找顺序：完整标签、主语言子标签（忽略大小写）、默认文本；每个字段独立回退。
4. 本地服务 `PluginInfo` 返回 `name`、`summary`、`i18n`，由桌面按当前界面语言解析。服务不感知界面语言。
5. 桌面插件名优先级：清单本地化文本、市场条目、插件 ID。没有简介时副标题只显示版本，不再重复 ID。
6. 本期不覆盖 Action 标题和市场目录条目；它们仍是单语言文本，另行决定。

## 边界

- 文本长度：`name` ≤64、`summary` ≤200（Unicode 字符数）；Schema `title`/`group`/标签 ≤256、`description` ≤1024；拒绝控制字符与非法 UTF-8。
- 非法语言键、未知键、`option_labels` 数量与 enum 不一致都会拒绝整个清单或 Schema。
- 不改变 Schema 校验和配置值；`option_labels` 只影响展示。
