# HarmonyOS 多空间运行时实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框逐项完成，原生桥先测试再改实现。

## 目标

让 HarmonyOS 客户端保留并同时运行多个 Space；加入新电脑只新增 profile，不清空当前手机配置，不影响手机与家中电脑已有连接。应用恢复前台或获得后台运行窗口时，以每个 profile 原身份恢复节点。

## 架构

Rust Node-API 层用 `profile_id -> SpaceRuntimeSlot` 的 supervisor 替换全局单例。每个 slot 独占 `CliAppRuntime`、任务、队列、取消标记和 profile 路径。ArkTS 的 `SpaceNodeService` 是 native supervisor 的薄包装；`ClipboardFeatureController` 持有 profile 列表与 active-send 选择，不再用一个 `spaceJoined` 表示全局状态。当前产品源码位于 `common/`、`features/clipboard/` 和 `products/default/`；根 `entry/` 是兼容快照，不作为主实现位置。

## 技术栈

Rust, Tokio, N-API, ArkTS, HarmonyOS Preferences, Hvigor, Deveco CLI, HDC.

## 变更文件

- 新增 `rust/uniclipboard-native/src/space_runtime_supervisor.rs`
- 修改 `rust/uniclipboard-native/src/lib.rs`
- 修改 `rust/uniclipboard-native/package/libs/index.d.ts`
- 修改 `common/src/main/ets/service/SpaceNodeService.ets`
- 新增 `common/src/main/ets/service/SpaceProfileStore.ets`
- 修改 `common/Index.ets`
- 修改 `features/clipboard/src/main/ets/viewmodel/ClipboardFeatureController.ets`
- 修改 `products/default/src/main/ets/entryability/EntryAbility.ets`
- 修改 `products/default/src/main/ets/view/compact/CompactClipboardView.ets`
- 修改 `products/default/src/main/ets/view/expanded/ExpandedClipboardView.ets`
- 修改对应中英文字符串资源

### 任务 1：用失败测试定义 native 多 runtime 生命周期

- [ ] 在 `space_runtime_supervisor.rs` 增加 Rust 单元测试：两个 profile 可同时启动；停止 A 不停止 B；重复启动 A 返回已有状态；一个 slot 失败只记录 A 错误。
- [ ] 将 runtime 创建抽象为测试可注入 factory，生产 factory 仍创建官方 `CliAppRuntime`。
- [ ] 先运行：

```powershell
cargo test --manifest-path rust/uniclipboard-native/Cargo.toml space_runtime_supervisor -- --nocapture
```

预期：模块尚不存在或测试失败。

- [ ] 实现 `SpaceRuntimeSupervisor` 与 `SpaceRuntimeSlot`，使用一个 `Mutex<HashMap<ProfileId, SpaceRuntimeSlot>>` 作为唯一状态源。
- [ ] 禁止为每项旧全局变量另建一张 map；任务、事件队列与 flags 必须归入对应 slot。
- [ ] 重跑至通过。
- [ ] 提交单一意图：`refactor: supervise Harmony space runtimes by profile`。

### 任务 2：把 Node-API 改为 profile-scoped

- [ ] 为 `startSpaceNode`、`stopSpaceNode`、状态查询、邀请、join、发送、设备列表和事件轮询增加 `profileId` 参数或显式 runtime handle。
- [ ] 在 `index.d.ts` 先改类型并增加 Rust 侧导出签名测试，使旧无 profile 调用编译失败。
- [ ] compatibility wrapper 只允许解析目录中的默认 profile，不得回退到“最后启动的 runtime”。
- [ ] 在 `lib.rs` 删除 `SPACE_RUNTIME: OnceLock<Mutex<Option<CliAppRuntime>>>` 及同职责全局变量，把调用统一委派给 supervisor。
- [ ] 运行：

```powershell
cargo test --manifest-path rust/uniclipboard-native/Cargo.toml -- --nocapture
```

预期：全部通过，不存在跨 profile 共享队列或停止标记。

- [ ] 提交单一意图：`feat: scope Harmony native APIs by profile`。

### 任务 3：实现手机 Space profile 目录与旧配置迁移

- [ ] 在 `SpaceProfileStore.ets` 定义仅包含随机 `profileId`、相对目录名、enabled 和 active-send 的目录；用户可见 Space 名称从 Engine 状态读取。
- [ ] 写 ArkTS 测试或可执行 service 测试：旧单 profile 配置首次启动被收养一次；再次启动幂等；加入新 profile 不覆盖旧目录；active-send 只改变一个字段。
- [ ] 先运行项目现有测试命令；若仓库无 ArkTS test task，则通过 `devecocli build --modules entry@default` 先证明新引用未实现而构建失败，并保留输出。
- [ ] 使用 Preferences 的单写入口保存目录；写入前校验 schema 和 profile ID 唯一性。
- [ ] 将 `SpaceNodeService` 改为接收 `profileId`，所有 native 调用在此集中附加 profile 参数。
- [ ] 提交单一意图：`impl: persist Harmony space profiles`。

### 任务 4：将 controller 从单 Space 状态改为 profile 集合

- [ ] 在 `ClipboardFeatureController.ets` 定义 `SpaceProfileViewState[]` 与 `activeSendProfileId`，用派生 getter 代替全局 `spaceJoined` 判断。
- [ ] 先写 service/controller 测试：A、B 同时 running；A join C 后 B 保持 running；切 active-send 不重启节点；删除 C 不改变 A、B。
- [ ] 删除 create/join 流程中的 replacing/switching 语义和 reset 调用，改为“创建隔离 profile -> 完成 admission -> 发布到目录”。
- [ ] 所有轮询和事件刷新按 profile 迭代，事件携带 profile ID 后合并进 UI；单 profile 错误局部显示。
- [ ] `EntryAbility.ets` 在生命周期恢复时启动全部 enabled profile；进入后台时遵守 HarmonyOS 运行窗口，不伪装永久后台能力。
- [ ] 提交单一意图：`feat: keep Harmony spaces online concurrently`。

### 任务 5：实现多空间界面和发送选择

- [ ] Compact 与 Expanded 视图增加 Space 列表、每项运行状态、当前发送标记、添加 Space 和停止/移除单项操作。
- [ ] 默认本机剪贴板只发送到 `activeSendProfileId`；显式分享界面允许多选目标 Space。
- [ ] UI 不再提示“切换将清空当前空间”；失败文案明确指出失败 profile，其他 profile 状态保持可见。
- [ ] 在浅色、深色和 Compact/Expanded 布局验证列表、错误状态和选中状态。
- [ ] 运行：

```powershell
devecocli build --modules entry@default
```

预期：ArkTS 类型检查与 HAP 构建通过。

- [ ] 提交单一意图：`feat: add Harmony multi-space controls`。

### 任务 6：重建原生包并做设备验证

- [ ] 从 HarmonyOS 仓库根运行：

```powershell
./rust/build-native.ps1
devecocli build --modules entry@default
devecocli run --module entry --product default
```

- [ ] 用 `hdc list targets` 确认设备，然后在应用不清数据的情况下让手机依次加入当前电脑和第二个测试 profile。
- [ ] 关闭并重开应用，确认两个 profile 均恢复；从当前电脑发送文本，手机收到；切换 active-send 后手机发送只进入选中 Space。
- [ ] 收集应用日志，确认没有全局 runtime 已存在、错误 profile 被 stop、admission 被旧 profile 阻塞等错误。

