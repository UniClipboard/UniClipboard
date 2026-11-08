# 后端装配、身份初始化与运行隔离契约

- 状态：目标方案已确定，待实现及故障验证。
- 日期：2026-11-08。
- 依据：[0008 统一设备](decisions/0008-device-identity-and-backend-bindings.md)、[设备契约](device-contract.md)、[0007 多后端](decisions/0007-multiple-sync-backends.md)、[插件契约](plugin-contract.md)。
- 文件名保留以维持文档链接；原旧 tailcat 数据迁移方案已取消。产品未上线，不读取/导入旧 tc.* 密钥、地址、配对记录，不要求地址不变或与旧 wire 对端互通；不提供旧 Network/SDK 兼容层。本文不执行本地数据删除或重置。

## 1. 身份与数据所有权

Engine 保存稳定设备 ID、统一身份密钥、公钥绑定、设备信任、待批准/拒绝记录及设备后端关联。插件保存自己的传输密钥、仓库凭据和游标，不打开 Engine SQLite，不取得 Engine 身份私钥。tailcat 网络身份与统一设备身份分别初始化，不能复制同一密钥作为其他插件的身份。

目标私有布局：

```text
DataDir/
  unic.db                         # 设备、信任、关联、历史与去重
  unic.lock                       # Engine 全生命周期独占锁
  identity/                       # Engine 统一身份私有材料，具体格式随身份协议定义
  history/
  plugin-data/
    <backend-instance-id>/        # 按稳定配置实例隔离，目录映射由宿主管理
      runtime.lock
      ...                         # 该实例的网络密钥/凭据/游标
```

实现标识与实例标识分离：同一实现的两个实例有独立目录、运行对象、配置、授权和状态；外部插件另有独立进程。目录映射不能直接拼接未经验证的实例字符串。程序版本变化复用该实例数据，不隐式重置身份；实例改用另一个仓库/账号须撤销原关联并重新授权。

## 2. 初始化、锁与恢复

New 只做离线初始化，取得 unic.lock 后读取或首次生成 Engine 身份并原子持久化。已有身份损坏、读失败或数据不一致即失败，不能当作首次启动重新生成。具体文件编码和密钥算法随统一身份协议补齐后实施。

插件取得自身 runtime.lock 后初始化/载入私有传输身份，并持锁至端点关闭且后台任务完全退出；同一实例不能并行运行两个端点。builtin 由 Engine 取消并等待任务退出；外部插件在父进程 EOF 后取消后台任务并退出。宿主重启即使取得 unic.lock，也须等旧实例 runtime.lock 可用才能接管，不能覆盖文件或按旧 PID 杀未知进程。

目录与身份/凭据文件采用当前用户私有权限（Unix 目录 0700、文件 0600，Windows 等价 ACL），拒绝符号链接越界。持久写入使用临时文件、Sync、原子发布与必要目录同步；故障后恢复到已提交完整身份，不发布半份身份。限时无法取得锁返回 in_use/unavailable，不隐式生成另一身份绕过。

新生成的 tailcat 地址须在插件承诺可复用前持久化；地址后续更新按关联修订和身份验证规则处理，不把地址本身当作设备身份。网络密钥/格式更新、设备身份轮换、备份恢复另定显式协议，禁止失败时悄悄重建。

## 3. 显式插件装配

移除公开 Options.Network/engine.Network 与 Engine 直接装配 tcnet/session 的生产路径；网络只能通过同步后端插件提供。builtin `builtin.tailcat` 在 `internal/plugins/builtintailcat/` 实现，由宿主显式创建并传入 Engine；与 `builtin.text` 保持独立插件 ID、配置、生命周期与实例。Engine 不隐式构造 tailcat，也不提供绕过插件的备用网络。插件内部可保留 Transport 注入用于 loopback 和故障测试。

Options 区分 builtin 实例装配与外部程序装配；实现标识、实例 ID、版本、API/能力、TransportKind 以及配置、授权校验一致。builtin 使用显式传入的 Go 实例，不需要 Executable、SHA256 或 RPC 握手；外部程序描述另包含 Executable 绝对路径和精确 SHA256。具体公开类型见 SDK 契约，builtin 同步能力和实例拆分待同步补齐。启用实例恰有一个装配来源，未知、重复、缺描述或能力冲突在 New 拒绝；不使用全局 init 注册。

核心不从 PATH、当前目录、环境、网络或 installed.lock 隐式找程序；安装、TOML、文件监听和受信任程序路径解析归宿主。外部程序摘要在启动前复核，不宣称同目录可写清单独自证明程序来源。Executable 是受控装配路径例外，不进入普通 State/Event/Error。

实现注记：当前 `internal/plugins/builtintailcat/` 以 `Transport` 注入点装配 session 引擎，并通过 `engine.Options.Peer`（PeerFactory）交给 Engine；Engine 仍引用 `internal/session` 的部分类型（未达到“Engine 不引用 session”的目标），待后端业务 DTO 落地后再拆。Engine 另按实例建立私有目录并加 `runtime.lock`；进程内 sync_backend 插件已可与 builtin.tailcat 并存，进程外后端仍返回 unsupported，插件后端暂无实例目录与锁。

零后端是本机模式，不隐式启动 tailcat；设备离线查询和撤销仍可用，需要网络能力而无合适后端的调用返回 unsupported。sync.enabled=false 只关闭内容工作，必要控制连接与配对可保留。

## 4. 默认构建与分发

桌面首次默认配置显式启用 builtin tailcat 实例；用户明确禁用全部后端时不重新启用。`builtin.tailcat` 随 unic/unicd 编译与升级，不单独分发、查找或构建 unic-plugin-tailcat，不需要独立发行清单；不通过 GitHub 安装器安装/卸载，禁止第三方覆盖其保留 ID。禁用后保留设备与关联记录。

外部插件继续按受信任路径、清单与摘要装配；程序/清单缺失时报告脱敏安装错误，不下载、不自动构建、不改用 builtin 代替。移动端的外部进程支持另定，builtin 的平台可用性仍须验证。

所有启用后端启动成功才算 Engine 启动成功，失败逆序清理。builtin 生命周期归 Engine 管理，关闭须等待端点与任务退出；实例可恢复错误按预算重新创建，进程级崩溃会影响整个宿主，不能声称具备子进程故障隔离。外部后端按预算重启进程；两者均不自动重发结果不确定的同步。

撤销成功需要后端生效屏障。外部实例不响应时可以终止进程并确认回收；builtin 必须关闭端点并确认旧任务退出。builtin 无法确认停止时不返回撤销成功，Engine 停止同步服务并报告失败，不能仅凭取消 context 宣称撤销完成。

## 5. 测试与交付

- 首次统一身份与插件网络身份初始化、原子持久化、坏文件拒绝、锁竞争和崩溃恢复；不加入旧键导入或旧 wire 对端测试。
- 同实现多实例目录/运行对象/凭据隔离（外部插件另验进程隔离）；程序更新保持实例数据，实例上下文替换不能继承原关联权限。
- Engine 不导入生产 tcnet/session、不打开后端私有身份；后端不打开 Engine DB，不取得设备私钥。
- builtin tailcat 通过正常装配验证启停、长连接、任务回收、撤销屏障与默认宿主构建；外部插件用构建一次的真实测试子进程验证 RPC 握手、失败清理、EOF、重启和回收。
- 统一身份、配对、关联审批与跨后端撤销按设备契约验收；设备网络协议及对应 RPC Schema/向量完成前，不宣告后端配对交付。
