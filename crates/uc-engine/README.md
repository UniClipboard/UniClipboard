# uc-engine 结构说明

`uc-engine` 是所有宿主使用完整 UniClipboard 核心的唯一入口。外部只需要理解 `Engine`、启动配置、宿主能力、操作、结果、事件和稳定错误；数据库、网络、加密、后台任务与恢复顺序都由 crate 内部负责。

## 目录

```text
src/
├── contract/                 # 稳定公开约定：配置、宿主、操作、结果、事件、错误
├── engine/                   # Engine 生命周期、事件流和在途操作管理
├── runtime/                  # 生产会话、操作路由、宿主剪贴板、文件处理与移动上传
├── operations/              # 按业务领域划分的操作处理
│   ├── space/               # 空间、邀请、解锁、恢复与重置
│   ├── clipboard/           # 捕获与恢复
│   ├── history/             # 历史、资源、投递、接收、重发与搜索
│   ├── device/              # 本机设备、成员与连接状态
│   └── settings/            # 设置、存储、加密、升级、诊断与配置迁移
├── subsystems/               # 不依赖具体基础设施的长期任务与协调逻辑
├── assembly/                 # 宿主适配、具体基础设施构建和最终组装
├── compatibility/mobile_lan/ # 用户显式选择的 LAN 兼容通道
├── dev/                      # 仅在 dev-tools feature 下编译的开发与验收操作
└── testing/                  # crate 内部宿主契约检查
```

`lib.rs` 只声明这些模块并重导出稳定公开名称。调用方不得依赖内部文件路径。

## 依赖方向

- `contract` 不引用运行、操作或组装实现。
- `engine` 只协调生命周期、事件和操作期限，具体执行交给 `runtime`。
- `runtime` 负责生产会话和路由，并把宿主剪贴板、文件处理与移动上传保留为彼此独立的内部模块；它不自行创建数据库、网络或加密实现。
- `operations` 只实现单个业务动作，不持有进程级任务，也不引用 `uc-infra` 具体类型。
- `subsystems` 只保留长期运行行为和协调逻辑，不负责选择具体适配器。
- `assembly` 是唯一允许引用 `uc-infra` 具体实现的范围，并负责把宿主能力转换成核心可用对象。
- `compatibility/mobile_lan` 不得成为完整 P2P 失败后的自动回退，也不得向主路径扩散 LAN 专用概念。
- `dev` 只在显式启用 `dev-tools` feature 时编译，不得进入正式宿主或发布产物。

## 修改位置

- 增加或调整稳定公开字段：修改 `contract/`，并保持 crate 根名称兼容。
- 增加业务操作：先在 `contract/operation.rs` 与 `contract/result.rs` 定义约定，再放入对应 `operations/` 领域，最后接入 `runtime/dispatch.rs`。
- 调整暂停、恢复或关闭行为：修改 `engine/`；会话资源的建立与释放修改 `runtime/`。
- 增加数据库、网络、文件系统或加密实现：只修改 `assembly/`。
- 调整 LAN 兼容行为：只修改 `compatibility/mobile_lan/` 及其组装接点。

不要重新建立公开 `internal` 模块，不要让外部 crate 读取源码文件来使用功能，也不要并行保留旧入口和新入口。
