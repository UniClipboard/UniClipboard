# 当前事实

- `/` 为物理类型，`@` 为来源设备，`#` 为标签；`on:` 为时间预设，`ext:` 为文件扩展名。
- 依据共享模型 SYNTAX_KEYS、TYPE_FILTERS、现有候选构造和 GPUI filters.rs；不假设新语法。
- 现有无候选 sigil 回到全文搜索，必须保留路径及未知字面正文的兼容性。
- 两皮肤共同使用 useCompositeSearchBar；完整浏览器夹具能连真实隔离 daemon。
- 已有 Base UI、Fluent UI、浏览器原生 input 选区能力；不需要新增解析包或搜索引擎。
- 官方 main、工作树 HEAD 同 SHA；PR #1966 merged；t0283/t0284 无公开待审 PR。

## 工具问题

- ego-browser 文档中的 taskSpace 在本机 CLI 不存在；首次实际调用得到 ReferenceError。
- 使用用户要求的项目已有 webdriverio 端到端机制，不修改浏览器安装或用户配置。

- 用户新增 t0288 边界：macOS parts.tsx/controls.tsx/menus.tsx 的 icon-only 按压反馈及 macos.css 对应选择器；不是整文件独占。本任务避开这些 hunk。
- Seed 初次查询误用 q 参数（真实契约为 query）；创建标签字段为 tagId。两处夹具已纠正。

- t0288 已确认本地提交 751d6f76d5645b435cbac7beb13c6ce61fa66798，基线同为 174f85004；本任务未触及其 controls/parts/menus/root/macOS CSS 文件。
- 用户禁止继续自动视觉验收后，仅只读分析已有 JSON：类型、标签及真实移动来源筛选已有历史通过记录；middle-caret 用例用原生 select 事件模拟 React onSelect，证据不足；快速输入夹具错误地在 macOS 使用 Control+A 导致正文串接；预期 pix=6 也错误，真实图片没有可搜的 pix 文件名，实际 pix=5。
- 真实 Engine 400 invalid_query 已观察到（混用 AND/OR）；现有 UI 只提示 History Unavailable。补充明确的语法反馈，并让 Linux 错误状态不再走空结果渲染。
- React Doctor 经 npx 首次启动因 oxc-parser 可选 native 包缺失失败；bunx 成功运行，发现 macOS token-search 渲染函数复杂度警告，需消除本次新增分支造成的回归。
