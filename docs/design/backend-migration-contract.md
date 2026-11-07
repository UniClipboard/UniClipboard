# 后端装配、密钥与网络接口迁移契约

- 状态：迁移方案已确定，待实现及故障注入验证。
- 日期：2026-11-07。
- 依据：[0004](decisions/0004-clipboard-pipeline-lua.md)、[0007 多后端](decisions/0007-multiple-sync-backends.md)、[Engine 契约](engine-contract.md)、[插件契约](plugin-contract.md)、[P2P 业务协议](sync-backend-contract.md)。
- 本文只确定迁移方案，不执行密钥迁移、目录清理或 SDK 删除；实现时必须同批调整代码、宿主、构建/发行包和测试。

## 1. 迁移前的实际数据

当前 `internal/tcnet/tcnet.go` 使用 Engine 的 `unic.db/meta`，其中四个键共同组成 tailcat 本机网络身份：

| 旧键 | 迁移内容 |
|---|---|
| tc.server.key | 服务端 NodePrivate |
| tc.server.psk | PresharedKey |
| tc.client.key | 客户端 NodePrivate |
| tc.server.addr | 已发布完整地址，含已选 DERP region 等定位信息 |

`device.id`、`device.default_name`、peers、pending 与拒绝记录继续只在 Engine 数据库管理，不交给插件打开。网络密钥与设备 ID 必须一起保留，不能只保留 DeviceID 却悄悄换地址。不能为了诊断在文档验证记录、日志、事件或错误中输出旧/新密钥、地址或它们的摘要。

## 2. 持久目录与锁

```text
DataDir/
  unic.db                         # Engine 身份、信任、历史、去重及迁移登记
  unic.lock                       # Engine 全生命周期独占
  history/                        # Engine 持久内容，与插件数据隔离
  plugin-data/
    sync.tailcat/
      runtime.lock                # 后端运行与离线维护互斥，系统级文件锁
      identity.json               # tailcat 唯一有效密钥集合，原子替换
      migration.json              # 非秘密迁移阶段、版本、generation
    <other-plugin-id>/            # 每实例独立持久数据，不能跨实例读取句柄
```

程序安装目录仍为 `$UNIC_HOME/plugins/installed/<id>/<commit>/` 或发行包目录；持久数据以插件 ID 划分，不以程序 commit 划分，更新程序不能丢密钥。插件卸载/disable 不默认删除持久身份，删除身份需要单独显式管理动作，不与历史清理绑定。

目录权限私有（Unix 0700，identity/登记临时文件 0600；Windows 用当前用户等价 ACL），拒绝把受管理路径解析到符号链接或越出 DataDir。插件只获得自己的绝对私有目录，不继承 Engine 数据库句柄或 Engine 数据目录锁。

父进程必须先取得 unic.lock，再取得 runtime.lock 才能迁移/修复身份。迁移完成后释放 runtime.lock，启动子进程由其重新取得该锁并持有至完全退出；只有一个持 unic.lock 的父进程可执行装配，所以该交接不允许另一个 Engine 同时启动。若其他手动进程抢占 runtime.lock，则启动报 in_use，不修改文件或强杀未知进程。

父进程异常退出时，旧子进程可能尚未响应 EOF。新 Engine 即使已取得 unic.lock，也必须先确认 runtime.lock 可用，不能边等旧后端退出边覆盖身份。等待受调用 deadline 约束，失败为 in_use/unavailable；不以旧 PID 是否存在代替系统文件锁，也不根据 PID 文件杀进程。

## 3. 身份文件与校验

identity.json 是版本化、严格字段校验的私有文件：`format_version=1`、`generation`（随机 128 位不透明 ID）、`device_id`、`server_key`、`preshared_key`、`client_key`、`published_address`（首次启动前可空）。秘钥文本使用现有库的 MarshalText/UnmarshalText 编码，不自行转码或重新生成已有值。

父端迁移器与内置后端共享平台无关的身份文件基础设施，解析/验证逻辑不得复制两份。迁移时解码全部密钥，校验非零值及其类型，解析旧地址并核对服务端公钥、PSK、region/协议参数与密钥集合一致；generation/device_id 也必须与迁移登记匹配。只比较完整文件字节或摘要不足以证明密钥与地址一致。

后端启动沿用旧地址固定的 region、端口 7777 及当前地址构造参数；生成实际端点后，在开放内容会话前核对地址与已发布地址逐字节相同。不能因为库升级、网络位置变化或 region 不可用而发布新地址覆盖旧值；不一致则关闭刚启动端点并启动失败。尾部格式或 canonicalization 变化也必须先设计显式地址迁移，不能当成无害字符串变化。

首次启动还没有 published_address 时，后端生成并原子持久化首次地址后才报告 Start 成功。此后同样必须保持该地址；保存失败关闭端点，不返回一个无法在重启后恢复的身份。正常启动不回写旧 Engine 的 tc.* 键，不双写两份活动身份。

## 4. 可恢复的迁移状态机

迁移在 `New` 的本机准备阶段、任何插件启动和网络访问之前执行，遵守 New 不联网的约定。仅当显式装配 sync.tailcat 且其要启用时进行；本机模式或该后端禁用时保留旧 tc.* 原样，不启动迁移。

Engine 数据库的 `plugin_migrations` 表记录插件 ID、迁移版本、generation、状态 prepared/activated，以及 source_commitment（对带版本、长度前缀的规范化完整源密钥集合和设备 ID 计算 SHA-256，用于检测源变化）。不重复存储秘密原文，不计算可公开展示的单地址摘要；source_commitment 也是私有恢复材料，禁止进入日志/事件/公开错误。plugin-data 的 migration.json 只记录同样的非秘密定位信息，作为辅助恢复材料，Engine 登记仍是权威。不能宣称跨 SQLite 与文件系统有单一事务，必须通过幂等步骤恢复。

1. **检查/快照**：持两把锁，事务内读取旧四键和设备 ID；分类完整、首次未启动、损坏或与现有目标冲突。生成 generation，在 Engine 事务中写 prepared/source_commitment，四个旧键保持不动。
2. **暂存**：在目标目录同文件系统创建唯一临时 identity 文件，写入完整密钥集合、Flush/Sync、严格复读校验；创建私有 migration.json。临时文件不得出现在安装 staging 或可被普通列表读取的位置。
3. **发布**：目标不存在时原子发布 identity.json，持久化目录变更；若目标已存在，只在相同 generation/设备/密钥/地址均一致时继续，否则报 conflict，不覆盖未知身份。Unix 使用同目录原子替换与目录同步，Windows 需实现等价原子持久化流程；不能只用普通覆盖写。
4. **激活**：在 Engine 事务中再次确认源键未变且目标复读有效，写 activated。只有此步骤提交成功才允许独立后端启动。磁盘/事务失败保留 prepared 和旧键，下一次 New 复读恢复；不回退进程内网络。
5. **首次运行确认**：子进程按第 3 节核对实际地址，成功 Start 才算运行验收通过。程序/握手/地址失败不抹掉已迁移身份，修复程序后继续使用同一 generation，不生成新的网络身份。

首次激活后保留旧 tc.* 四键为只读恢复材料，本阶段不自动删除。后续移除旧密钥副本必须另定版本与恢复策略，不能用“已迁移”直接做不可恢复删除。迁移失败不要求删除已经正确落盘的目标，恢复时只清理本迁移 generation 明确拥有且未引用的临时文件。

## 5. 缺失、损坏与中断处理

| 状态 | 处理 |
|---|---|
| 四键均无，且无旧配对/待批准记录和无迁移/目标文件 | 作为首次网络初始化，父端离线生成完整密钥集合，published_address 为空，原子激活后子进程首次发布地址 |
| 三个密钥完整、地址无，且无旧信任记录 | 视为旧版本已 New 但从未成功 Start，保留三密钥，首次发布地址；不重生成 |
| 缺少任一密钥、坏编码、缺地址但已有旧设备记录 | unavailable，保留现场，不以随机新身份“修复” |
| prepared + 目标未发布 | 重新验证源集合的 source_commitment，按同一 generation 续做；源已变化则 conflict |
| prepared + 完整且匹配目标 | 复读验证后完成激活，不重新生成/覆盖 |
| activated + 有效匹配目标 | 只使用目标身份；旧键不得参与正常运行或覆盖新文件 |
| activated + 目标缺失/损坏 | 启动失败，不自动用旧键重建或重新迁移；由明确修复流程复核恢复材料后处理 |
| 任一阶段 + 未登记的目标身份或另一设备 generation | conflict，禁止合并两个身份集合 |
| 其他后端启动失败、程序不存在或摘要不符 | 启动按生命周期逆序清理；所有已迁移身份保持，不删除目录、不退回旧网络 |

旧密钥仍保留不等于支持直接运行旧 daemon。旧程序不认识新历史 schema、后端状态与迁移登记，不能保证兼容；需要降级时停止全部进程，恢复迁移前一致的完整 DataDir 备份及匹配程序/配置，而不是只复制 identity.json 或几项 SQLite 键。备份与恢复均为显式维护动作，不在常规 New 中自动复制整个数据目录。

## 6. Options.Network 的迁移

`Options.Network Network` 和公开 `engine.Network` 不能跨 RPC 传递，在独立后端切换的 P5 提交中移除；不保留一个忽略输入的字段，也不偷偷用其走第二条生产网络路径。P1 的内部过渡适配只允许在 P5 前存在，P5 完成后边界测试禁止 Engine 直接装配 tcnet/session。

目标装配 API：

```go
type PluginProcessSpec struct {
    ID            string
    Version       string
    APIVersion    int
    Capabilities  []string
    TransportKind string // 仅 sync_backend 使用
    Executable    string // 宿主已解析的绝对路径
    SHA256        string // 精确二进制摘要，启动前复核
}

// 新增到 Options，类型均为公开 SDK 类型。
// PluginProcesses []PluginProcessSpec
// Plugins         PluginSettings
```

PluginSettings 定义 Order/Instances/OnError；实例 Enabled/Events/Config，标准同步策略生效值统一由 Settings.BackendPolicies 管理，宿主将 TOML `.sync` 映射到该表，不能再存第二份不同策略。启用的 Go 实例必须在 PluginProcesses 中有且只有一个描述，Lua 由宿主读脚本传入 Options.LuaPlugins；禁用的已安装描述不启动、不访问其目录。未知/重复 ID、Spec 冲突和缺描述在 New 拒绝。上面的字段概要以[SDK 扩展类型契约](sdk-extension-contract.md) 的命名 PluginID/PluginCapability/TransportKind 类型及完整装配定义为准。

核心不从 PATH、当前工作目录、环境变量、网络或磁盘 installed.lock 隐式发现程序。Executable 是装配用的本机路径例外，不进入 State/Event/Error；核心只在本机初始化/启动边界读取已给定程序与私有目录，安装、TOML、文件监听仍归宿主。不同插件只收到自己的私有目录，传输正文仍只用流句柄，不能把该装配路径例外扩展到内容 API。

**零值语义显式改变**：迁移后 SDK 仅给 DataDir、没有启用插件时为本机历史模式，不隐式寻找 tailcat 可执行文件；New/Start 可本机工作，网络门面 unsupported。桌面宿主在没有用户插件配置的首次默认配置中显式装配/启用发行版 tailcat，因此桌面默认同步行为保持。用户明确禁用全部后端时不得被默认值重新启用；SDK 的此项变化须在发布说明登记。

外部调用者原自定义 Network 的替换方案是实现独立 sync_backend 并传进程描述，或使用后端内部 Transport 测试/嵌入适配；不承诺原 Network 接口有透明等价替代。移动端不能启动子进程的运行方式仍需平台决策，本次不自动退回进程内第三方插件。

## 7. 默认程序发现、构建与分发

桌面默认后端为 unicd 所在受信任发行目录的同级 `unic-plugin-tailcat`（Windows 为 `.exe`），宿主读取同目录发行清单核对 ID/Version/API/能力/TransportKind/摘要后传入绝对路径。发行清单与三个程序由同一构建流水线生成；摘要用于固定内容，不宣称单靠同目录可写清单解决程序来源信任。

不从当前目录或 PATH 猜测同名程序，不因未安装而下载、自动 go build 或回退进程内。程序/清单缺失时宿主报告脱敏的安装错误；SDK 描述不合法在 New 拒绝，程序启动/握手失败按启动生命周期清理。GUI/CLI 不在错误中输出秘密配置或文件路径。

开发构建须把 `unic`、`unicd`、`unic-plugin-tailcat` 放在同一个输出目录并生成对应清单；P5 同批更新 README、AGENTS、CI、发行包及 macOS 打包布局，不能只让开发机因 PATH 中恰好有程序而通过。内置 tailcat 禁止第三方占用其保留 ID；第三方已安装插件由桌面安装锁显式选择版本，不允许覆盖正在使用的程序。

多个后端各自使用 plugin-data/<id>，只有内置 sync.tailcat 有旧 tc.* 导入规则，不能把其密钥复制给 WebDAV 或其他后端。程序升级复用同一数据目录，密钥存储格式升级需自己的版本与恢复方案，禁止重装即重置身份。

## 8. 测试入口迁移与验收

`internal/tcnet` 从 `*store.Store` 依赖改为内部身份存储抽象，真实实现读写插件 identity.json；核心迁移器读旧 SQLite，但运行中后端不读 Engine 库。session 抽离对 Engine store 的依赖，以 SyncHost 验证/信任/准入为权威，不能把旧数据库句柄传给新进程。

后端内部保留标准库 Transport（Listen/Dial/Addr 等）测试注入，供 TCP loopback、故障网络和旧 wire 对端回归；不再作为公开 engine.Options 参数。Engine/桌面契约测试使用构建一次、真实常驻的测试后端程序，经相同 RPC/描述/摘要校验运行，控制通过显式测试配置传入，不依赖继承环境凭据或生产可选 `--test-network` 后门。

测试 helper 可位于 `internal/plugintest/`，只供仓库测试装配，不能形成 Engine 的生产导入路径；测试程序 ID 使用测试实例 ID，策略按实际 ID 映射，不在安装器注册第三方 sync.*。启动失败、取消阻塞、孤儿清理等原 fakeNet 用例必须保留其行为断言，不能只删除不再编译的 Network 测试。

必须覆盖：

- 旧完整数据库迁移后 server/client 密钥、PSK、完整地址、DeviceID 与信任记录保持；私有目录/权限与多后端隔离正确。
- 未启动旧实例、缺单键、坏编码、地址/密钥不匹配、跨设备目录、重复迁移与目标冲突全部按表处理。
- prepared 写入、文件 Sync、原子发布、目录同步、激活事务、首次 Start 每个边界故障/崩溃后重试，既不丢身份也不并行开启两个端点。
- 父进程死而旧子进程仍占锁，新实例不能覆盖身份；EOF/取消后回收完成才允许接管，不按 PID 文件杀未知进程。
- SDK 无描述为本机模式；桌面默认明确装配 tailcat；全部禁用不被重启默认值覆盖；路径/摘要/握手错误无隐式回退。
- 真实测试后端与旧 wire 双节点文本/HTML/PNG/文件回归、多后端独立生命周期及默认全局开关/策略语义。
- Engine 依赖中无生产 tcnet/session 直接装配、桌面工具包、TOML 或文件监听；SDK 无 Network 后门。
- 错误/事件/日志和测试失败输出不含密钥、地址、凭据路径或这些秘密的摘要；文档验收只记匿名测试结果。

完成本方案文档不等于密钥已迁移；P5 退出条件须包含故障注入、升级前后地址匿名相等断言及干净安装分发验证。
