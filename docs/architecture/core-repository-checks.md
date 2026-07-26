# 核心仓库消费者检查

desktop 通过一个统一入口检查独立核心的消费边界：

```bash
bun run check:core-repository
```

该入口读取 Cargo 的完整锁定依赖信息，并检查以下规则：

- 已迁出的核心、绑定、兼容和验收目录不能留在 desktop workspace 或文件树中。
- desktop 对核心包的全部直接依赖只能来自 `UniClipboard/core` 的同一个不可变提交。
- desktop 正式运行代码只通过 `uc-engine` 使用核心业务，不能直接依赖内部实现包或 LAN 协议包。
- `uc-observability` 只使用同一提交中的可移植观测约定。
- `uc-webserver` 必须显式启用 LAN 兼容；daemon 和 bootstrap 不能直接启用；CLI 只允许在开发工具中显式启用。
- LAN 只能由用户设置驱动，P2P 失败不能触发自动切换。

检查程序自带三个隔离错误样例，分别模拟本地核心路径、浮动核心版本和自动 LAN 回退。每次执行都必须证明三个样例会被拒绝。

核心内部依赖、公开入口、绑定来源、密文持久化和发布完整性由 `UniClipboard/core` 仓自己的检查负责。desktop 不再读取核心源码来重复验证这些规则。
