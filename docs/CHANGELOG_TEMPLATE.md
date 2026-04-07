# Changelog Template

本模板用于每次发布时生成终端用户更新日志。

默认写法是：只总结“上一个已发布版本 -> 当前版本”之间，用户能直接感受到的变化。

它不是开发记录，也不是整个版本线的汇总说明。只有在提示词里明确指定更大范围时，才允许汇总多个版本。

## 模板

```markdown
## {VERSION} - {YYYY-MM-DD}

### Breaking Changes

- {仅在确有用户可感知的不兼容变化时保留}

### Features

- {描述用户能直接感受到的新增或改进，每条一行}

### Fixes

- {描述用户遇到的问题被如何修复，每条一行}
```

## 规则

1. **默认统计范围**是“上一个已发布版本 -> 当前版本”
2. **预发布版本**默认只写本次增量，不合并更早的 alpha / beta / rc 日志
3. **正式版本**默认也只写给定范围，不自动汇总整段预发布历史；若需要整段汇总，必须在提示词中明确说明
4. **仅包含有内容的分类**，空分类整段省略；`### Breaking Changes` 只在确有用户可感知的不兼容变化时保留
5. **只写用户可感知的变化**，不写重构、日志、CI、测试、依赖升级、架构调整等内部内容
6. **写结果，不写实现**；避免内部模块名、协议名、算法名、状态管理细节等术语
7. **拿不准就不写**；不能确认用户是否能感知到，就省略
8. **同类内容合并**为一条，避免把同一件事拆成多条重复描述
9. **每条一句话**，短、清楚、自然；英文版与中文版都遵循同一结构
10. **版本号**取自 `package.json` / `Cargo.toml` / `tauri.conf.json`
11. **日期**使用发布当天，格式 `YYYY-MM-DD`

## 示例

```markdown
## 0.4.0-alpha.5 - 2026-04-07

### Features

- Improve setup and pairing progress messages so it is clearer what the app is waiting for
- Make image and file previews more reliable across supported surfaces

### Fixes

- Prevent lag when previewing very large text content
```
