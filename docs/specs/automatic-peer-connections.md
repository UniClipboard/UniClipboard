# 自动连接的桌面宿主接入

状态：实现完成，Mac 与 iOS 模拟器验收通过；依赖本次 Engine 修改，尚未发布。

## 分工

Engine 是自动连接和重试的唯一负责人。桌面不定时调用刷新，不读取 Engine 内部成员或恢复阶段来编排流程。

- GUI 任意窗口获得焦点时，经现有认证客户端发送一次前台机会。请求独立执行并限制等待时间，不阻塞窗口事件。
- `POST /presence/opportunity` 只接收固定 `foreground`、`system_wake`、`network_changed`；未知输入使用固定错误，不回显原文。204 表示机会已接受，不表示已连接。
- daemon 自己订阅系统唤醒，不依赖 GUI 窗口是否存在；停止转发并回收监视器后再关闭 Engine。
- `uc-platform::system_wake` 只提供有界通知与关闭。macOS 使用 IOKit，Windows 使用系统恢复回调，Linux 使用 logind 的 `PrepareForSleep(false)`。后端注册失败可见，Engine 自己的周期重试仍可工作。
- 监视器的等待仅用于接收系统事件和有界关闭，不承担任何设备列表、退避或连接规则。

## 已执行验证

- 认证客户端测试通过：三种机会正确发送，未调用手动刷新。
- Mac 上实际注册系统唤醒通知并回收线程/通知源的测试通过。
- 真实 daemon sidecar、GUI 和前端资源构建通过，workspace all-targets check 通过。
- OpenAPI 引用与路径数量检查通过；实际认证接口三种机会返回 204，未知原因和多余字段返回 400 且不回显。
- Mac 与 iOS 模拟器完成同时冷启动、两个先后启动顺序、双方分别重启、iOS 后台再前台六种场景，每种双向测试正文通过，未调用刷新。
- 所有启动/重启场景自动连通约 2–6 秒；前台恢复另一次独立复验为 0.756 秒。iOS 强制停止后旧连接的离线判定约 65 秒，不宣称即时发现进程退出。
- 初轮验收使用本地路径覆盖，来源检查因旧短提交号失败；后续来源修复的结果见下节，不混用两轮证据。

## 本地验证前提

当前 `Cargo.toml` 的两个 Engine 依赖和 `Cargo.lock` 中的全部 Engine 包统一引用
`ac9a50a787a3280e1657c9d56edd26b207bc67de`，包含自动连接及严格审查后的修复。
本轮使用该提交的 Git 源进行编译，不再使用工作区路径覆盖。尚未推送的提交由本机 Git 缓存提供；
远端可获取性仍需推送后验证，不能将本机缓存当作远端发布证据。

这是来源配置的局部缺陷：旧来源缺少宿主已调用的新能力。修复只统一不可变来源与锁文件，不复制核心实现、
不增加备用来源，也不放宽既有来源检查。移动包仍须由同一 Engine 发布提供。

本轮来源检查、`cargo check --workspace --all-targets --locked --offline`、
`cargo build -p uc-daemon --locked --offline`、格式与差异检查通过。元数据确认只有一个 Engine Git 来源。
本轮没有重跑产品配对场景，前节模拟器结果仍为初轮验收证据。

真实系统睡眠、Wi-Fi/VPN 切换、Windows/Linux 和旧版混用未运行，记为跳过。模拟器首次粘贴权限由测试准备完成；连接计时不包含系统等待授权。

系统来源：[Apple IOKit 唤醒通知](https://developer.apple.com/documentation/iokit/kiomessagesystemhaspoweredon)。

Windows 回调能力已按[Microsoft 注册说明](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registersuspendresumenotification)及已安装 windows crate 接口核对，未声称 Windows 运行验证通过。
