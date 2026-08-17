# 来源：Windows `Win+V` 快捷键接管

## Issue

- UniClipboard #1569：用户希望增加一个设置，使快捷面板可使用 `Win+V`。
  https://github.com/UniClipboard/UniClipboard/issues/1569

## Microsoft 文档

- [`RegisterHotKey`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerhotkey)：
  - `MOD_WIN` 代表 Windows 徽标键，带 Windows 键的快捷键保留给操作系统。
  - 同一组合已被其他快捷键占用时，注册通常失败。
  - 登记成功才会把按键通知交给应用。

- [`LowLevelKeyboardProc`](https://learn.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc)：
  - 低层键盘监听可在系统继续分发前处理按键，并以非零返回值阻止后续处理。
  - 监听回调必须极快返回；超时后 Windows 会静默移除监听。
  - 监听需要独立消息循环，耗时工作应转交其他线程。

## 现有依赖

- `global-hotkey 0.8.0` 的 Windows 实现把 `super` / `meta` 转为 `MOD_WIN`，调用
  Windows `RegisterHotKey`；失败时将系统错误返回，不会使用键盘钩子绕过系统。
- 项目使用的 `tauri-plugin-global-shortcut 2.3.2` 正是通过该依赖完成 Windows
  全局快捷键登记。

## 项目现状

- `crates/uc-desktop/src/shortcuts.rs` 已把 `meta` 归一化为 Windows 的 `super`，
  因而当前录制器已经允许输入 `Win+V`。
- `src-tauri/crates/uc-tauri/src/commands/settings.rs` 在登记失败时保留旧的已生效
  快捷键，且不会写入新的设置。
- 文档目前错误地暗示只要关闭 Windows 剪贴板历史便可使用 `Win+V`；Windows 文档
  只承诺该键仍是系统保留键，不能给出“必定可接管”的产品承诺。

## 成熟模式

1. 用 Windows 正式登记接口申请快捷键。
2. 申请失败时，保留已生效配置并显示明确原因和可选操作。
3. 要真正接管系统组合键，就采用成熟键盘工具使用的低层键盘监听，但把它限制为 Windows 专属、显式开启、只拦截准确的 `Win+V`，并把可靠性约束当成一等需求。
