# Windows 配对恢复与标题栏修复实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框逐项完成，不得跳过失败测试和真实设备验证。

## 目标

修复 Windows 上原子替换状态文件后错误同步父目录导致的 `space transition storage failed`，保证配对事务在进程中断后可以收敛，并将桌面窗口关闭按钮向左移动一个 Tailwind 间距等级。

## 架构

Engine 继续作为配对状态机与持久化的唯一事实来源。文件替换的跨平台差异封装在 `uc-infra`：Windows 使用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` 提供落盘保证，Unix 在重命名后同步父目录。恢复逻辑只通过现有 admission 与 membership recovery 入口推进或回滚事务，不在桌面端复制状态判断。桌面仓只固定 Engine 提交并修改 GUI 样式。

## 技术栈

Rust, Tokio, Windows API, Cargo, React 19, TypeScript, Vitest, Tailwind CSS, Tauri.

## 变更文件

Engine 仓库：

- 修改 `crates/uc-infra/src/security/admission_space_transition.rs`
- 修改 `crates/uc-application/src/space/convergence/admission/tests.rs`
- 修改 `crates/uc-application/src/space/convergence/admission/transaction.rs`
- 按测试需求修改 `crates/uc-application/src/space/convergence/admission/completion_recovery.rs`
- 修改 `crates/uc-engine/tests/space_membership_auto_pairing_e2e.rs`

桌面仓库：

- 修改 `Cargo.toml`
- 修改 `Cargo.lock`
- 修改 `src/components/TitleBar.tsx`
- 修改 `src/components/__tests__/TitleBar.test.tsx`

### 任务 1：锁定 Windows 原子写入回归

- [ ] 在 Engine fork 的独立工作树创建 `codex/fix-windows-admission-recovery` 分支，并确认工作树干净。
- [ ] 在 `crates/uc-infra/src/security/admission_space_transition.rs` 的测试模块增加 `write_new_file_replaces_existing_file_on_windows_without_directory_open_error`：先写旧文件，再调用 `write_new_file`，断言返回成功、最终内容正确、临时文件不存在。
- [ ] 在 Windows 仓库根目录运行：

```powershell
cargo test -p uc-infra write_new_file_replaces_existing_file_on_windows_without_directory_open_error -- --nocapture
```

预期：测试失败，错误来自尝试把父目录当普通文件打开或同步。

- [ ] 保留失败输出作为根因证据，不修改测试去适配错误行为。

### 任务 2：按平台修复持久化提交

- [ ] 在 `admission_space_transition.rs` 将“替换文件”和“同步父目录”拆成私有平台函数，调用点只有一个。
- [ ] Windows 路径在 `MoveFileExW` 成功后直接返回成功；Unix 路径继续在 `rename` 后打开并 `sync_all` 父目录。
- [ ] 最小实现形状：

```rust
fn commit_replacement(temp: &Path, destination: &Path) -> Result<(), StorageError> {
    replace_file_atomically(temp, destination)?;
    sync_parent_directory_if_supported(destination)
}

#[cfg(windows)]
fn sync_parent_directory_if_supported(_: &Path) -> Result<(), StorageError> {
    Ok(())
}
```

- [ ] 运行：

```powershell
cargo test -p uc-infra admission_space_transition -- --nocapture
cargo test -p uc-infra write_new_file_replaces_existing_file_on_windows_without_directory_open_error -- --nocapture
```

预期：新增回归测试与现有 transition 测试全部通过。

- [ ] 提交单一意图：`fix: persist admission transitions on Windows`。

### 任务 3：证明中断后的 admission 可以收敛

- [ ] 在 `crates/uc-application/src/space/convergence/admission/tests.rs` 增加两个用例：`sponsor_recovery_finishes_durable_candidate_after_restart` 和 `sponsor_accepts_next_candidate_after_recovery_converges`。
- [ ] 第一个用例在 durable candidate 写入后模拟停止，重建 runtime 后调用公开恢复入口，断言 pending transaction 被完成且成员历史一致。
- [ ] 第二个用例在恢复完成后发起新的 invitation，断言不会返回 `another workspace admission is already in progress`。
- [ ] 先运行：

```powershell
cargo test -p uc-application sponsor_recovery_finishes_durable_candidate_after_restart -- --nocapture
cargo test -p uc-application sponsor_accepts_next_candidate_after_recovery_converges -- --nocapture
```

预期：至少一个测试失败，证明现有恢复路径没有把 durable candidate 推进到终态。

- [ ] 在 `transaction.rs` 与 `completion_recovery.rs` 收敛恢复入口：先重放已持久化 membership effects，再提交 admission completion；失败保持可重试，成功清除对应 attempt，不允许直接清空整个 Space。
- [ ] 恢复决策使用明确的 attempt ID 和 base history；不得把一个新候选覆盖到另一个未判定候选上。
- [ ] 重跑上述测试，并运行：

```powershell
cargo test -p uc-application space::convergence::admission -- --nocapture
```

预期：全部通过。

- [ ] 提交单一意图：`fix: converge interrupted sponsor admissions`。

### 任务 4：增加 Engine 端到端连续配对测试

- [ ] 在 `crates/uc-engine/tests/space_membership_auto_pairing_e2e.rs` 增加 `sponsor_pairs_two_devices_sequentially_without_reset`。
- [ ] 测试创建 A 空间，依次加入 B、C；每次等待成员历史一致，最后断言 A、B、C 均为 active member，且 A 的 profile 未重建。
- [ ] 先确认测试在未合入修复的提交上失败，再切回修复提交运行：

```powershell
cargo test -p uc-engine --test space_membership_auto_pairing_e2e sponsor_pairs_two_devices_sequentially_without_reset -- --nocapture
```

预期：修复提交上通过。

- [ ] 提交单一意图：`test: cover sequential device admission`。

### 任务 5：固定桌面 Engine 修复提交

- [ ] 将 Engine 修复推送到用户 fork，记录不可变提交 SHA。
- [ ] 在桌面根 `Cargo.toml` 只更新统一 Engine `rev`，运行 `cargo update` 生成对应 `Cargo.lock`。
- [ ] 运行：

```powershell
cargo check --workspace
cargo test -p uc-webserver -p uc-daemon-client
```

预期：依赖解析到新的 Engine SHA，检查和定向测试通过。

- [ ] 提交单一意图：`fix: adopt recovered Engine admission flow`。

### 任务 6：移动关闭按钮

- [ ] 在 `src/components/__tests__/TitleBar.test.tsx` 增加断言：Windows 窗口控制区关闭按钮容器包含 `mr-4` 且不再包含 `mr-2`。
- [ ] 运行：

```powershell
npx vitest run src/components/__tests__/TitleBar.test.tsx
```

预期：测试先因当前 `mr-2` 失败。

- [ ] 在 `src/components/TitleBar.tsx` 将关闭按钮右侧间距改为 `mr-4`，不改变最小化、最大化与拖拽区域。
- [ ] 重跑测试并运行 `npm run lint`。
- [ ] 提交单一意图：`fix: inset Windows close control`。

### 任务 7：本计划验收

- [ ] 检查 Engine 与桌面 diff，不包含格式化噪声或无关文件。
- [ ] 运行 Engine 回归测试、桌面 targeted tests、`cargo check --workspace` 与 `npm run lint`。
- [ ] 用真实 Windows daemon 先加入手机，再在不删除任何 profile 的情况下生成第二个 invitation；确认日志中没有 `space transition storage failed` 或 `another workspace admission is already in progress`。

