# Android 应用

先读 [Engine 公共契约](../../docs/contracts/engine-contract.md) 和 [0003](../../docs/decisions/proposed/0003-engine-sdk.md)。本目录是宿主：负责剪贴板、权限、主线程、通知；业务语义（配对、信任、同步策略）全部来自 Go 的 `engine`，不要在这里重新实现。

- 设备地址与配对票据是秘密，不要写进日志或界面之外的地方。
- 通过配对码进来的设备永远需要用户批准。
- 能力如实声明（`HostCapabilities`），不要用统一成功掩盖降级。
