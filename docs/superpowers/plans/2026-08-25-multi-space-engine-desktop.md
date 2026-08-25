# 桌面多空间运行时实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框逐项完成，每个行为先写失败测试。

## 目标

让桌面端保留多个 Space，每个 Space 同时运行独立 Engine runtime；加入新 Space 只新增 profile，不替换或删除已有 profile。系统剪贴板默认只发送到当前选中的 Space，所有运行中 Space 的入站内容都可接收。

## 架构

daemon 是本机多空间目录和 runtime 生命周期的唯一权威。目录只保存随机 `profile_id`、启动策略和当前发送目标等非内容元数据；Space 名称、设备名和剪贴板内容仍由各自 Engine 的加密存储提供。每个 profile 使用独立数据根、SQLite、blob、密钥、网络身份、admission 状态和 profile 锁。GUI 仅通过 daemon v2 API 与 WebSocket 观察和操作 supervisor，不直接打开 Engine 或数据库。

## 技术栈

Rust, Tokio, Axum, Serde, OpenAPI, React 19, Redux Toolkit, Vitest, WebSocket.

## 变更文件

- 新增 `apps/daemon/src/daemon/space_catalog.rs`
- 新增 `apps/daemon/src/daemon/space_runtime_supervisor.rs`
- 修改 `apps/daemon/src/daemon/mod.rs`
- 修改 `apps/daemon/src/daemon/host.rs`
- 修改 `apps/daemon/src/daemon/startup_recovery.rs`
- 新增 `crates/uc-daemon-contract/src/api/dto/v2/spaces.rs`
- 修改 `crates/uc-daemon-contract/src/api/dto/v2/mod.rs`
- 新增 `crates/uc-webserver/src/api/v2/spaces.rs`
- 修改 `crates/uc-webserver/src/api/v2/mod.rs`
- 新增 `crates/uc-daemon-client/src/http/spaces_v2.rs`
- 修改 `crates/uc-daemon-client/src/http/mod.rs`
- 修改 `tests/e2e/src/daemon.rs`
- 新增 `tests/e2e/tests/multi_space.rs`
- 新增 `src/api/daemon/spaces.ts`
- 新增 `src/store/spacesSlice.ts`
- 新增 `src/components/spaces/SpaceSelector.tsx`
- 新增 `src/components/spaces/__tests__/SpaceSelector.test.tsx`
- 修改 `src/App.tsx`
- 修改设备与发送入口中当前直接使用单 Space 状态的组件

### 任务 1：定义用户可观察的多空间 API

- [ ] 在 contract 中从用例出发定义 `SpaceProfileSummaryDto`、`CreateSpaceProfileRequestDto`、`JoinSpaceProfileRequestDto`、`SetActiveSendSpaceRequestDto` 与 `SpaceRuntimeStateDto`。
- [ ] Wire 字段统一 camelCase；枚举结构字段同时声明 `rename_all_fields`，增加逐字序列化测试。
- [ ] API 行为固定为：`GET /v2/spaces`、`POST /v2/spaces`、`POST /v2/spaces/join`、`PUT /v2/spaces/active-send`、`DELETE /v2/spaces/{profileId}`。删除只删除显式指定 profile，并要求 runtime 已停止。
- [ ] 先写 contract 序列化测试并运行：

```powershell
cargo test -p uc-daemon-contract spaces -- --nocapture
```

预期：新 DTO 尚未实现，测试编译失败。

- [ ] 实现最小 DTO 与 wire 测试，重跑至通过。
- [ ] 提交单一意图：`arch: define multi-space daemon contract`。

### 任务 2：实现本地 Space 目录

- [ ] 在 `space_catalog.rs` 写测试：空目录首次启动收养现有单 Space 数据为第一个 profile；第二次启动幂等；新增 profile 不改动第一个 profile；设置发送目标只改变目录中的目标 ID。
- [ ] 目录文件仅持久化随机 ID、相对 profile 目录名、启用状态和 active-send 标记。用户可见名称从运行中的 Engine 查询，不写入明文目录。
- [ ] 使用同目录临时文件、`sync_all` 和原子替换；沿用平台正确的父目录同步规则。
- [ ] 先运行：

```powershell
cargo test -p uc-daemon space_catalog -- --nocapture
```

预期：模块不存在或行为测试失败。

- [ ] 实现 `SpaceCatalog::load_or_migrate`、`add_profile`、`set_active_send`、`remove_profile`；所有 ID 查找返回 typed error。
- [ ] 重跑至通过。
- [ ] 提交单一意图：`impl: persist daemon space catalog`。

### 任务 3：实现每 Space 一个 Engine runtime 的 supervisor

- [ ] 在 `space_runtime_supervisor.rs` 写异步测试，使用 fake runtime factory 验证：启动目录中全部 enabled profile；单个 profile 失败不停止其他 profile；停止只影响指定 profile；同一 profile 不会重复启动。
- [ ] 先运行：

```powershell
cargo test -p uc-daemon space_runtime_supervisor -- --nocapture
```

预期：模块或测试失败。

- [ ] 实现 `SpaceRuntimeSupervisor`，核心形状：

```rust
pub struct SpaceRuntimeSupervisor {
    runtimes: HashMap<ProfileId, SpaceRuntimeSlot>,
    active_send: ProfileId,
}
```

- [ ] `SpaceRuntimeSlot` 独占 profile 路径、锁、Engine handle、任务取消 token 和最后错误；状态变更只经过 supervisor 方法。
- [ ] `startup_recovery.rs` 调用 `start_enabled_profiles`，每个 runtime 自行执行 admission 与 membership recovery。
- [ ] 重跑测试至通过。
- [ ] 提交单一意图：`impl: supervise one Engine runtime per space`。

### 任务 4：实现加入新 Space 而不替换旧 Space

- [ ] 在 supervisor 测试中增加 `join_creates_isolated_profile_without_stopping_existing_profiles`。
- [ ] 测试断言旧 runtime handle、profile 目录和 active membership 未变化，新 profile 在独立目录完成 join。
- [ ] 先运行该测试，预期因现有 switch 语义失败。
- [ ] 将 join 用例实现为：创建未启用的 profile 目录、启动隔离 runtime、完成 join、原子加入目录并启用；失败时只清理该次未发布 profile，旧 profile 不受影响。
- [ ] 不复用“清空当前 profile 后切换”的旧路径；完成迁移后删除该路径的调用。
- [ ] 重跑 supervisor 与 startup recovery 测试。
- [ ] 提交单一意图：`feat: add spaces without replacing existing profiles`。

### 任务 5：实现剪贴板路由

- [ ] 写 dispatcher 测试：本机捕获只发送给 active-send Space；显式多选发送按所选 profile 集合广播；各 Space 的入站事件都进入统一事件流并携带 `profileId`；任一发送失败不会阻断其他显式目标。
- [ ] 先运行：

```powershell
cargo test -p uc-daemon multi_space_clipboard_routing -- --nocapture
```

预期：现有单 runtime dispatcher 无法满足测试。

- [ ] 将 clipboard capture 的 Engine 目标解析收敛到 supervisor；不得在 API handler、GUI 和 runtime 内各维护一份 active-send 判断。
- [ ] WebSocket 事件新增 `profileId`，不包含明文内容之外的额外敏感元数据，沿用现有加密与脱敏规则。
- [ ] 重跑路由测试。
- [ ] 提交单一意图：`feat: route clipboard events across spaces`。

### 任务 6：接入 daemon HTTP 与客户端

- [ ] 在 `spaces.rs` 写 handler 测试，覆盖 list、add、join、set-active-send、stop/remove、未知 ID 与某 runtime 失败时的状态返回。
- [ ] 先运行 `cargo test -p uc-webserver spaces_v2 -- --nocapture`，预期路由缺失。
- [ ] handler 只调用 supervisor 门面；transport projection 集中在 `api/v2/spaces.rs`，不把 DTO 传入业务层。
- [ ] 在 daemon client 增加一一对应的 typed 方法与响应测试。
- [ ] 运行：

```powershell
cargo test -p uc-webserver spaces_v2 -- --nocapture
cargo test -p uc-daemon-client spaces_v2 -- --nocapture
```

预期：全部通过。

- [ ] 分别提交 `feat: expose multi-space daemon API` 与 `feat: add multi-space daemon client`。

### 任务 7：实现桌面 Space 选择界面

- [ ] 在 `SpaceSelector.test.tsx` 写测试：列出全部 Space 及在线状态；切换 active-send 不停止其他 Space；加入成功后旧 Space 仍显示在线；失败状态只标记对应 Space。
- [ ] 运行：

```powershell
npx vitest run src/components/spaces/__tests__/SpaceSelector.test.tsx
```

预期：组件不存在或测试失败。

- [ ] `spacesSlice.ts` 是 GUI 内唯一 Space 视图状态；服务端目录和 runtime 状态仍为权威，前端不得额外持久化 profile 列表。
- [ ] `SpaceSelector.tsx` 使用语义化按钮、稳定 `profileId` key 和无障碍标签；当前发送 Space 明确标识，所有运行中 Space 显示在线或错误状态。
- [ ] 修改 setup/join 流程为调用“添加 Space”，成功后刷新列表，不再触发替换确认或全局 reset。
- [ ] 运行 targeted Vitest、`npm run lint` 和 `npm run build`。
- [ ] 提交单一意图：`feat: manage active spaces in desktop UI`。

### 任务 8：桌面端到端验收

- [ ] 在 `tests/e2e/tests/multi_space.rs` 写测试：迁移旧 profile；依次加入 B、C；重启 daemon；三个 runtime 恢复；active-send 保持；删除 C 不影响 A、B。
- [ ] 先在未完成 supervisor 的提交上确认测试失败。
- [ ] 运行：

```powershell
cargo build -p uc-daemon -p uc-cli
cargo test --manifest-path tests/e2e/Cargo.toml --test multi_space -- --ignored --nocapture
```

预期：完整实现后通过，日志中没有全局 reset 或 profile 覆盖。

