# GPUI 上游依赖迁移

日期：2026-10-22
状态：已完成

## 结论

用 Zed 仓库固定提交 `ddf762e368352ae372b7631f7eebda4f5366e8bd` 替换 `vendor/gpui`。该提交包含上游针对 Windows 字体选择失败的正式修复与文本定位 API 回退，早于平台拆包，优先保留 `gpui-component 0.5.1` 的接口兼容性。通过根 `[patch.crates-io]` 统一 GPUI 来源；GPUI 内部 crate 随同一提交解析。组件库直接使用的注册表过程宏仅负责代码展开，其兼容性由编译检查确认，不引入第二份 GPUI 类型。

## 已确认事实

- 上游修复：`https://github.com/zed-industries/zed/commit/6cafe4a9c5cac0ce86f957d8cb035ee3c925f183`（2025-11-07，PR #42212）。字体选择返回 `Option`，平台字体查询返回 `Result`，绘制回调返回 `DWRITE_E_NOFONT`。
- Windows 后端于 2026-02-19 拆包；直接采用当前主分支会扩大迁移范围，不是修复本次崩溃所必需。
- 本仓原有 `gpui-component 0.5.1` 依赖注册表的 GPUI 与过程宏；必须验证最终依赖图只有一份 GPUI。
- 生产构建必须可通过固定提交与锁文件复现；不得修改 Cargo 缓存源码或保留本地补丁副本。

## 实施清单

- [x] 固定上游依赖，更新锁文件并核查唯一来源。
- [x] 编译并处理必要的兼容调整。
- [x] 移除本地副本、仅为它增加的配置及测试入口。
- [x] 验证 Linux 和 Windows 目标，保留可执行的回归验证方法。

## 验证边界

当前主机为 Linux ARM64；Windows 交叉编译可借助临时 Zig 工具链完成。Windows 与 macOS 原生运行结果须明确区分，不宣称已经实机验收。

## 兼容性验证发现

直接固定字体修复提交时，`gpui-component 0.5.1` 在 `closest_index_for_x` 与 `index_for_x` 上产生三个编译错误；该提交当天早先合入了 API 合并（#42162）。上游在 2025-11-12 的 `ddf762e368352ae372b7631f7eebda4f5366e8bd`（#42505）已回退此改动。改用该提交，保持官方字体修复，并重新验证组件兼容性。

## 最终验证

- Linux ARM64：`cargo check -p quick-panel --locked --offline` 通过。
- Windows x64 GNU：快捷面板生产目标及 `windows_font_fallback` 集成测试均通过交叉编译检查。
- `quick-panel-core` 的 189 项测试通过。
- 依赖图确认仅有一份 GPUI，来源为固定 Zed 提交；未升级现有组件库。
- Windows 原生字体回归已加入 x64 与 ARM64 CI，当前 Linux 主机未执行 Windows 程序；macOS 原生窗口及焦点行为尚未验收。
- 本地 GPUI 副本与专用忽略项已删除；根锁文件保留无关依赖的既有解析结果。
