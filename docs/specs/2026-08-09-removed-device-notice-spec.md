# 被移除设备的移除通知接入 —— 实施规范

- **状态**：待实施
- **日期**：2026-08-09
- **对应决策**：`docs/adr/adr-011-offline-first-member-removal-integration.md`（本节为 ADR-011 的第 7 节补充，独立成文便于评审）
- **上游**：UniClipboard/Engine PR #24 最新提交 `ea8eea50ae92efa834358416c515e50309749017`（feat(membership): notify a removed device of its removal），以及其前置竞态修复 `a3dfc9731b051f7bf835f2b5c0ba719dd980e9ce`
- **验收标准**：以本文第 4 节每步验收 + 第 6 节测试矩阵为准

## 0. 完成定义

本规范完成 = 以下全部为真：

1. 根 `Cargo.toml` 的 `uc-engine` / `uc-observability-contract` 指向 PR #24 最新提交 `ea8eea5`，`cargo check --workspace --exclude uniclipboard --all-targets` 通过；
2. 桌面全链路（DTO → 投影 → webserver → daemon-client → CLI → 前端）透传 `MemberRemovalDto.removed`，B 侧（被移除设备）`GET /member/removal` 与 WS `member-removal.changed` 均能反映 `removed: true`；
3. 前端在 B 侧检测到 `removed: true` 时，在本机设备条目的右侧显示「此设备已被移除」状态图标；停留图标可看到重新配对（`join`）引导；
4. e2e 新增 R16–R18（见第 6 节），且 R07/R12 因竞态修复恢复确定性通过；
5. 重新准入（R05 场景）后旧 `removed` 标记清除，不误伤新实例。

## 1. 背景

### 1.1 遗留问题（本规范解决的问题）

ADR-011 集成完成后，被移除设备（B）在产品侧 **无法感知自己被移除**：

- Engine 普通意图交换在 `exchange_intents_plan` 中跳过 `locally_removed` 目标（`crates/uc-application/src/member_removal/mod.rs` `device_is_locally_removed` 分支），A 从不把 A→B 意图发给 B；
- B 的 `QueryMemberRemoval` 恒返回 `{ phase: applied, intentCount: 0 }`——与「从未移除」不可区分；
- B 收不到内容——与「网络故障」不可区分；
- B 重新 `join` 被拒时 Engine 将 `SponsorAdmissionUnavailable` 映射为 `SponsorInternal`，故意模糊化（`redeem_invitation.rs`），B 无法区分「被移除」与「sponsor 故障」。

### 1.2 上游已实现（PR #24 最新提交）

Engine PR #24 在 `3baf55a`（集成基线）之后新增两个提交，直接消除上述缺口：

**`a3dfc97` fix(membership): serialize inbound removal writes with background reconcile**

修复本仓 e2e 首次暴露的竞态：`accept_intent` / `submit_removal` / `ingest_exchange` / `handle_late_submission` 的读 - 改-写此前不持有 `state_lock`，与持锁的 `reconcile_plan` 并发时，陈旧快照会覆盖入站消息刚保存的意图（表现为并发/迟到双意图场景 `intent_count` 恒为 1、收敛停滞）。修复后所有入站入口与后台推进在同一锁内串行；入站只做本地工作，不引入锁下网络。

**`ea8eea5` feat(membership): notify a removed device of its removal**

1. 新 `RemovalNotice` 轻量事实：仅含空间沿革指纹、移除意图 ID、目标成员实例与设备、签发者成员实例与签名；**不含** 收敛摘要、成员列表、密钥、内容或因果证明。
2. 新受限通道 `removal-notice/1`：接受意图的当前成员（任一，非仅发起者）通过 `RemovalNoticePort` 向被移除设备投递；`notified_removals` 集合记录已投递意图（尽力而为，不阻塞收敛，可重试）。
3. 接收方 `handle_notice`：核对空间指纹 → 以本机保存的 `view_signing_keys` 查签发者公钥 → 验签 → 核对 `target_device_id` 是本机 → 持久化 `self_removed` / `self_removed_target`（幂等）。
4. `MemberRemovalSummary` / `MemberRemovalView` / bindings 新增 `removed: bool`；`QueryMemberRemoval` 与 `MemberRemovalChanged` 事件均携带。
5. `refresh_self_removed`：查询时惰性核对 `self_removed` 意图的目标实例与当前实例，重新准入产生新实例后自动清除旧标记；重放的旧通知无法重新锁定新实例（`self_removed_target` 对照）。

### 1.3 桌面侧需要做的

Engine 只负责把「removed 事实」送达 B 并暴露在查询/事件中；**展示与引导属于产品端**。本规范定义桌面侧接入：

- Rust 全链路透传 `removed` 字段；
- CLI `removal-status` 展示 `removed`；
- 前端 B 侧设备条目的「已被移除」状态图标与重新配对引导；
- e2e 覆盖通知投递、幂等、重放防护与重新准入清除。

## 2. 前置条件

- 本机可访问 `github.com/UniClipboard/Engine`（`cargo update` 需拉取 `ea8eea5` rev）；
- `bun` 可用（SDK 重新生成）；`cargo` 工具链可用；
- e2e 依赖 `cargo build -p uc-daemon -p uc-cli` 后的二进制。

## 3. Engine 信号语义（接入方必读）

| 概念 | 语义 | 安全边界 |
| --- | --- | --- |
| `RemovedNotice` | 单条「成员资格已终止」事实，定向投递给被移除设备 | 不含成员列表/摘要/代次/密钥/内容 |
| `removed: bool`（查询/事件） | 本机是否观察到自身被移出当前空间（`self_removed` 存在且目标实例匹配） | 仅布尔事实，无新状态泄露 |
| `removed` 与 `phase` 的关系 | 相互独立：B 侧 `phase` 恒 `applied/0`（B 不参与意图集合），`removed` 单独由通知置位 | — |
| 重新准入清除 | `refresh_self_removed` 在查询时惰性核对；新实例（新 `MemberInstanceId`）自动清除旧标记 | 旧通知不能锁定新实例 |

**桌面侧不得**：从 `removed` 反推收敛摘要或成员集合；不得把 `removed` 当作 `phase` 的派生值。

## 4. 实施步骤

### S01 引擎依赖升级

**文件**：`Cargo.toml`（根）

1. 两处 `rev = "3baf55a8a9bc2e61f35c7badf9ac60f806940f9c"` 改为 `rev = "ea8eea50ae92efa834358416c515e50309749017"`（注释保持 TEMPORARY 说明，追加「含 removed-notice 与入站串行化修复」）；
2. `cargo update -p uc-engine -p uc-observability-contract`；
3. `cargo check --workspace --exclude uniclipboard --all-targets`。

**预期错误清单**（S02 消除）：

- `crates/uc-daemon-contract/src/api/dto/member.rs`：`MemberRemovalDto` 缺 `removed` 字段导致投影/测试编译失败；
- `crates/uc-webserver/src/api/projection/member.rs`：`MemberRemovalSummary` 新增 `removed` 字段未映射；
- `crates/uc-webserver/src/api/event_emitter.rs`：事件 payload 未携带 `removed`；
- 相关测试中的 `MemberRemovalSummary` / `MemberRemovalDto` 字面量缺 `removed`。

**验收**：`git diff --stat` 仅 `Cargo.toml`、`Cargo.lock` 被修改；错误仅在上述文件。

### S02 DTO 与投影透传

**文件**：`crates/uc-daemon-contract/src/api/dto/member.rs`、`crates/uc-webserver/src/api/projection/member.rs`

1. `MemberRemovalDto` 新增字段（`camelCase` 序列化为 `removed`）：

   ```rust
   /// True when this device has observed its own removal from the space.
   pub removed: bool,
   ```

2. 投影 `impl IntoApiDto<MemberRemovalDto> for MemberRemovalSummary` 增加 `removed: self.removed`；
3. 测试更新：既有构造补 `removed: false`；新增 `removed_flag_is_passed_through_from_summary`（`removed: true` → DTO `removed == true`）。

**验收**：`cargo test -p uc-daemon-contract -p uc-webserver projection::member` 通过。

### S03 webserver 事件透传

**文件**：`crates/uc-webserver/src/api/event_emitter.rs`

`MemberRemovalChanged` 匹配臂的 `serde_json::json!` 增加 `"removed": event.removed`；测试 `member_removal_changes_notify_device_screens_with_full_progress` 补断言 `payload["removed"] == false`，`complete_removal_event_serializes_counts_and_digest` 补 `removed` 字段。

**验收**：`cargo test -p uc-webserver event_emitter` 通过。

### S04 重新生成 OpenAPI schema 与前端 SDK

```bash
bun run gen:api
```

**验收**：

```bash
git grep -n '"removed"' schema/openapi.json src/api/generated/types.gen.ts  # 存在于 MemberRemovalDto
```

### S05 daemon-client / CLI

**文件**：`crates/uc-daemon-client/src/http/member.rs`、`apps/cli/src/commands/member.rs`

1. daemon-client 无需改动（DTO 自动携带 `removed`）；
2. CLI `render_human` 在 `removal.removed` 为 true 时输出醒目行（`ui::warn` 或独立 glyph）：

   ```text
   ⚠  this device has been removed from the space; re-pair to rejoin
   ```

   `--json` 输出原样（DTO 含 `removed`）。

**验收**：

```bash
cargo test -p uc-daemon-client -p uc-cli
```

### S06 前端 API 层与 store

**文件**：`src/api/daemon/member.ts`、`src/store/slices/devicesSlice.ts`

1. `MemberRemoval` 接口新增 `removed: boolean`；`toMemberRemoval` 映射 `removed`；
2. `isNoMemberRemoval` 语义不变（`phase === 'applied' && intentCount === 0`），**与 `removed` 正交**——被移除设备的空状态是 `removed: true` 而 `isNoMemberRemoval` 也为 true，页面须优先判 `removed`；
3. 新增 `isDeviceRemoved(removal: MemberRemoval): boolean` 返回 `removal.removed`；
4. store 无需新字段（`MemberRemoval` 类型透传即可）。

**验收**：`bunx tsc --noEmit`。

### S07 前端页面（B 侧状态图标 + 重新配对引导）

**文件**：`src/pages/DevicesPage.tsx`、`src/i18n/locales/{en-US,zh-CN,zh-TW,ja-JP,pt-BR,ru-RU}.json`

1. `DevicesPage` 的本机设备条目右侧显示状态图标；`memberRemoval.removed === true`、移除进行中和需要恢复时均使用对应图标，图标的悬停说明使用各自的标题和描述。不要在页面顶部新增设备移除状态的 `Alert`。
2. 所有设备条目右侧均显示状态图标：在线、离线、正在移除、需要恢复和已被移除五种状态使用同一位置和交互方式；设备名称保留在左侧，状态图标不作为新的可点击操作。
3. 本机被移除后保留现有设备列表和详情面板，移除状态仅影响本机条目的状态图标；不在前端临时过滤对端设备。已被本机移除的对端应由 Engine 的有效设备列表过滤，避免刷新后重新出现。

4. i18n 六个 locale 的 `devices.memberRemoval` 节点新增：

   ```json
   "deviceRemoved": {
     "title": "This device was removed",
     "description": "This device is no longer part of the space. Re-pair with an invitation to rejoin."
   }
   ```

   各语言自行翻译（中文参照 ADR-011 第 4 节风格）。

**验收**：`bun run build`；`bun run test`（相关文件）；验证本机被移除时没有页面顶部提示、图标悬停可看到重新配对说明，且所有已列出的设备均有右侧状态图标；i18n key 校验。

### S08 e2e 测试矩阵扩展

**文件**：`tests/e2e/src/member_removal.rs`

| 编号 | 测试函数名 | 断言要点 |
| --- | --- | --- |
| R16 | `removed_target_observes_self_removed_flag` | R01 完成后，B 侧 `member removal-status --json` 的 `removed == true`；A 侧 `removed == false`；B 侧 `intentCount == 0`（正交性） |
| R17 | `removed_notice_is_idempotent_and_replay_safe` | B 离线时 A remove B；B 上线收到通知 `removed: true`；重复通知不改变状态（幂等）；直接重放旧通知（无新意图）不产生新意图计数 |
| R18 | `re_admission_clears_removed_marker` | R16 后 B `join` 重新配对（R05 路径）→ B 侧 `removed == false`；`intentCount` 不增长；新实例不受旧标记影响 |
| R07/R12 | 恢复确定性（上游 `a3dfc97` 修复竞态） | 移除 `KNOWN ENGINE RACE` 注释，多次运行稳定通过 |

另：R01 的 B 侧断言可增强为同时校验 `removed == true`（替代「空状态」弱断言）。

**验收**：

```bash
cargo build -p uc-daemon -p uc-cli
cargo test --manifest-path tests/e2e/Cargo.toml member_removal -- --ignored
```

### S09 全量验证

```bash
cargo check --workspace --all-targets --locked
cargo test --workspace --exclude uniclipboard
cargo fmt --check
git diff --check
bun run lint
bunx tsc --noEmit
bun run test
bun run build
```

**验收**：全部通过。

## 5. 提交拆分建议

按「单一意图」拆 4 个 commit（顺序执行，每个独立可编译）：

1. `chore(engine): bump Engine to PR #24 removal-notice commit (ea8eea5)` —— S01；
2. `refactor(member-removal): carry the removed flag through the daemon API` —— S02–S04（Rust 透传 + schema 重生成）；
3. `feat(cli): surface self-removal in member removal-status` —— S05；
4. `feat(devices): show a removed-device notice with re-pair guidance` —— S06–S07（前端）；
5. `test(e2e): removal notice matrix and re-admission clearing` —— S08。

## 6. 风险与回退

- **Engine PR 未合并**：rev 指向 `ea8eea5`；Engine 发布后切回发布标签（同 ADR-011 第 1 节流程）。
- **通知是尽力而为**：B 在通知投递前永久离线 → B 重新上线时 `removed` 仍为 false（Engine 不保证必达）。产品侧接受该语义；如需更强保证（如 B 上线后主动拉取），需 Engine 新能力，超出本规范。
- **`removed` 与空状态正交**：前端须先判 `removed` 再判 `isNoMemberRemoval`，避免被移除设备显示「无移除」误导。
- **回退**：`git checkout main -- Cargo.toml Cargo.lock` 回到上一 Engine；代码层改动集中在 S02–S07 文件。

## 7. 安全分析

- 通知内容最小化：仅空间指纹 + 意图 ID + 目标/签发者实例 + 签名，无任何当前状态信息；与 spec 015「被移除设备得不到当前成员、密钥、内容或收敛结果」一致。
- 接收侧验证链：空间指纹 → 视图成员公钥 → 签名 → 目标设备核对，任一失败拒绝且不改状态（失败关闭）。
- 重放防护：`self_removed` 幂等 + `self_removed_target` 对照当前实例；重新准入产生新实例后旧标记惰性清除。
- 桌面侧不透传 Engine 内部 ID：`removed` 仅布尔，前端不展示意图 ID / 签发者。

## 8. 开放问题（产品确认）

1. 提示文案是否提及「被谁移除」（通知含签发者，但产品可先不暴露）。
2. CLI human 输出的提示级别（`ui::warn` vs 常规行）与 `--json` 的兼容性（不改 JSON 形状）。
3. 已被本机移除的对端何时从设备列表消失：需要 Engine 输出有效设备列表，不能由前端自行推断。
