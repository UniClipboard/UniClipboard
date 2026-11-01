# desktop 本地主题偏好

Omarchy 主题开关由 Go/Wails 宿主的 `apps/gui-go/desktop_theme.go` 持有。它不属于 Engine，不进入 daemon 设置接口，也不参与设备间同步。

## 存储与所有权

Linux 宿主通过 `packages/desktop-host-go/apppaths` 解析当前资料的数据目录，偏好文件为 `desktop-theme.json`：

```json
{
  "followOmarchyTheme": false
}
```

文件缺失、字段缺失或 JSON 无法解析时，默认关闭。非 Linux 宿主不读取该文件。

宿主串行保存偏好。保存先写入同目录临时文件，再同步文件并原子替换。保存成功后才更新内存快照、增加 `revision` 并广播 `desktop-theme://changed`。保存失败时保留原偏好和版本，向控件返回错误。当前调色板不可用时，设置命令返回原快照，不改变偏好。

该布尔值是本机外观配置。按 [VISION.md](../../VISION.md) 的配置规则，它可以明文保存。文件不包含剪贴板内容、凭据或调色板。

## 调色板读取

`apps/gui-go/desktop_theme_palette.go` 依次查找：

1. `~/.local/state/omarchy/current/theme/colors.toml`。
2. 系统配置目录下的 `omarchy/current/theme/colors.toml`。

宿主不要求 `OMARCHY_PATH`。读取只在 Linux 运行，每次最多读取 64 KiB。调色板必须包含有效的 `background`、`foreground` 和 `accent` 颜色。

宿主每秒重新读取一次文件。读取失败或调色板无效时，快照报告不可用并清空调色板。文件恢复有效后，下一次轮询重新发布调色板。调色板只驻留内存，不写入业务设置。

## 应用规则

- 开启且调色板可用：使用 Omarchy 的深浅模式和配色，禁用手动主题控件。
- 关闭：使用原有应用主题设置，不保存平行备份。
- 已开启但调色板不可用：保留偏好，使用应用主题。调色板恢复后继续跟随。
- 调色板不可用：隐藏专用开关，保留已保存的偏好。

主窗口与快捷面板通过 `apps/gui-go/frontend/src/lib/window-theme.ts` 选择最终主题。初始快照在读取本地偏好后提供。查询响应与事件携带版本，前端忽略过期结果。GUI 退出时停止轮询。
