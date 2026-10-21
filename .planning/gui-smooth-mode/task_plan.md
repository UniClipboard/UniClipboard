# Go GUI 流畅模式完整实装计划

## Goal

先完成现状与缺口梳理，再将自动／效果优先／流畅优先的完整行为落实到唯一 Go/Wails GUI 宿主。以 VISION.md 和现有流畅模式规格的产品语义为依据，替换已退役 Tauri 的实现路径。

## Next Step

已完成实现、本机验收、代码审查与当前分支提交。后续在 macOS/Windows 主机补做已记录的真机验证。

## Phases

### Phase 1：梳理用户行为、现状与缺口
**Status:** complete

- 核对旧规格与当前 Go GUI。
- 核对三套皮肤、窗口同步、持久化、系统偏好与自动采样。
- 给出模块所有权、实施顺序与验收标准。

### Phase 2：实现可保存、跨窗口一致的手动选择
**Status:** complete

### Phase 3：实现平台自动初判与运行表现降级
**Status:** complete

### Phase 4：补齐三套皮肤与公共效果控制
**Status:** complete

### Phase 5：真实宿主验收与文档更新
**Status:** complete

## Decisions

- 用户已调用 implement，授权实现、验证、审查并提交当前分支。
- 不修改 Engine，不将本机视觉偏好加入同步业务设置。
- 不直接用 profile=a 做自动化或覆盖原配置。

## Errors Encountered

- 大批文档合并读取导致输出截断；改用分段定向读取。

- 用户明确：仅适用于 Go/Wails GUI，排除 macOS/Windows 独立原生快捷面板；保留 WebView 快捷面板范围。

- 实施时保留同一 Go package 内的 visual_effects*.go，复用现有 DTO，避免额外的跨包投影。
- 当前可验证环境为 Linux；macOS/Windows 原生硬件和文件替换的真机证据须明确列为未验证，不以注入分类或交叉编译替代。
