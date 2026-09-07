# 设备组 GUI 验收：Engine 恢复失败调查资料

日期：2026-09-07。本文只记录已复现事实；不把测试遗漏或界面等待错误归因于 Engine。

## 修复后的状态

以下调查与源码行号保留为修复前证据，不再代表当前未提交源码。已在同级 Engine 完成结构性修复：当前已验证历史负责成员资料维护，旧分支任务不再污染新组；历史交换明确发送方证明范围，合法旁支不再误拒绝，曾被错误阻挡的成员经完整验证恢复。

- 新运行 `cross-remote` 已通过，含真实收发与重启查询：`.cache/device-group-e2e/run-mtr7ink4/`。
- 新运行五配置 `new-peer` 已通过，含 A→E 实际 GUI 接收和排除关系验证：`.cache/device-group-e2e/run-mtr7spcm/`。
- 四配置旧冻结现场克隆恢复通过：`.cache/device-group-e2e/run-mtr8kd1j/`。
- 五配置旧冻结现场克隆恢复完整通过：`.cache/device-group-e2e/run-mtr97qhm/`。早一轮 `run-mtr8o9vd` 虽业务通过，但清理退出非零，不计整轮通过。
- 原失败资料未覆盖；重跑入口见使用说明的“恢复旧故障现场”。

Engine 的红绿测试与完整回归边界见其 `.planning/structural-membership-recovery/`；产品最新矩阵以 `verification.md` 为准。不能将这两条故障修复等同于系统键盘等所有验收项完成。

## 后续 Engine 只读调查结论

已进入同级 Engine 仓库核对当前 `fe55e543` 源码。未修改 Engine 源码、未修改冻结现场的业务数据，未启动个人配置。
使用 Engine 当前类型、Postcard 解码及真实签名验证器，离线读取测试资料；密钥和解密后的完整负载只存在内存中，输出仅有测试角色、分类、数量和比较结果。

### 问题一：旧分支完成记录导致新分支成员被清理

四配置冻结现场 `run-mtqt6bkz` 的 A：

- 已验签的当前历史确认有效成员为 A/B/C。
- `pending_effects` 仍有旧的 RemoveDevice(C)，phase=Activated。
- 该旧移除事件不在当前选中历史中，C 没有待完成受限交付。
- 当前清理条件因此仍会选中 C，而成员投影实际只剩 2 条、地址只剩 1 条。

源码链路（路径相对 Engine 根目录）：

1. `crates/uc-infra/src/security/space_control_generation/mod.rs:570` 克隆旧 ledger 后替换历史及 peer reconciliation，未重建旧分支的 effect 执行范围。
2. `crates/uc-infra/src/space/adapters/membership_projection_cleanup.rs:45` 从所有 Activated 移除 effect 生成删除名单，仅排除仍有受限交付的设备，不核对事件是否属于当前分支或目标是否仍是有效成员。
3. 同文件第 57 行删除成员投影和地址。
4. `crates/uc-application/src/space/membership/query_device_trust/use_case.rs:116` 遇到已验证的有效成员缺少投影，返回 Unavailable。

这解释了为何选择后可短暂同步、重启维护后却查询失败。五配置 A 的冻结记录中也保留了旧 RemoveDevice(D)，而当前 D 仍有效；因此它同样有后续误清理风险，不能只修复网络重试。

修复需要同时审视分支切换时待执行效果的归属及清理授权。不能仅在查询层补造缺失成员，也不能把“历史中发生过移除”当作当前仍应删除的依据。

### 问题二：合法增量被完整历史摘要比较拒绝，Invalid 又阻止后续恢复

用五配置 `run-mtqtag2b` 的冻结历史直接调用当前 Core 的导出、验签和增量接收方法，无需运行 GUI 或网络，约 2 秒得到：

| 离线重放 | 结果 |
| --- | --- |
| A→D 完整历史导入 | 通过真实签名及完整历史验证 |
| A→D 增量接收 | InvalidPersistedHistory；所有传入记录验签通过；重建后的主线终点也匹配 |
| A→D 重建与发送方的差异 | 接收方额外保留 1 条旧事件：A 曾移除 D；没有缺失事件、激活回执或决定，激活基线相同 |
| A→E | 主线事件及深度相同，但完整历史摘要不同；A 没有自己的新增决定，导出增量返回 InvalidPersistedHistory |
| E→A | E 的决定可通过验签，增量接收成功，重建位置完全匹配 |

具体机制：

- `crates/uc-core/src/membership/versioned_membership_history/history.rs:171` 的位置摘要覆盖完整持久历史，包含并非当前主线的已知事件及各端已收到的决定。
- `crates/uc-core/src/membership/versioned_membership_history/exchange/suffix.rs:128` 从接收方历史克隆 sender_projection；增量不会删除接收方合法保留的其他分歧事件。
- 同文件第 169 行却要求这个投影的完整摘要与发送方完全相同，因此上述合法 A→D 更新被拒绝。这个结论不是猜测签名失败：实际签名已通过，差异已定位到一条旧分歧事件。
- `crates/uc-application/src/space/membership/handle_history_message/use_case.rs:316` 把此类接收失败统一转换为 Invalid，发送方也在 `synchronize_history/target_use_case.rs:511` 保存 Invalid。
- 同一 `target_use_case.rs:139` 在周期恢复选择中直接排除 Invalid。冻结 ledger 确认 A 对 E、E 对 A 都保存为 Invalid，所以即使当前 E→A 更新已经能够合法接收，也不会靠正常周期恢复完成。

修复应统一增量交换的证明范围与摘要含义，区分合法分歧/状态变化与真正无效的资料，并明确可恢复拒绝后的推进路径；不能跳过签名验证或把所有 Invalid 强制改为健康。

**证据边界**：已精确复现 A→D 的误拒绝、A/E 保存为 Invalid，以及当前 E→A 本可成功但周期任务跳过的状态。原始 A/E 第一次互标 Invalid 对应的具体报文，已有日志没有目标身份/传输关联，不能精确还原；不能把 A→D 重放结果直接冒充那次 A→E 报文的完整因果证明。

### 只读诊断产物

目录：`.cache/device-group-e2e/engine-investigation-20260907/`，包括 `main.rs`、本机诊断构建清单及 `four.jsonl` / `five.jsonl`。
该工具是本机一次性诊断，不代替 Engine 正式回归测试。SQLite 以 immutable/read-only 打开，只接受无待重放 WAL 的冻结数据库；没有启动 Engine 运行期、没有发网络请求、没有执行清理或用户选择。

从 Engine 根目录重跑：

```sh
cargo build --offline --manifest-path ../desktop/.cache/device-group-e2e/engine-investigation-20260907/Cargo.toml --target-dir target
target/debug/membership-readonly-probe ../desktop/.cache/device-group-e2e/run-mtqt6bkz/run.json
target/debug/membership-readonly-probe ../desktop/.cache/device-group-e2e/run-mtqtag2b/run.json
```

以下保留最初交接时的复现流程与证据，根因认知以上述后续调查为准。

## 已确认的问题：选择远端组后重启，设备组查询不可用

### 版本和边界

- Engine：`fe55e5436aa830a5c382d1f14a6ba98733273f1a`，工作区干净；包含 `0f3a5843` 展示资料与稳定性变更。
- Desktop：`4af47d01da55ca9e605c44cde39bf4559ce55788` 加当前未提交的适配与 GUI 改动。
- Cargo 实际解析到本机同级 Engine；每轮 `build.json` 保存源码状态及实际 GUI/后台程序校验值。
- 本机四个独立测试配置，正常加密与准入，基线全部已确认。未使用个人 a/b/c/d。
- 未修改 Engine，未绕过成员历史或将缺失资料伪装成健康关系。

### 一条命令复现

先按 `docs/guides/device-group-gui-testing.md` 构建当前后台与 E2E GUI，再执行：

```sh
npx --yes --package=node@24 node e2e/conflict-suite.mjs four cross-remote
```

工具从只读基线复制新资料，打开四个真实窗口，执行以下步骤，失败后停止测试进程并保留资料：

1. 确认 A/B/C/D 属于同一组，成员关系已确认。
2. 暂停其他测试后台，A 移除 C；暂停 A，恢复 B，B 移除 D；恢复所有后台。
3. A 窗口显示不同移除的选择，选择 B 的远端组，即 A/B/C。
4. 若 B 还有独立事项，在 B 的窗口选择同一组。
5. 检查 A 的有效成员与所选名单一致；A 向 B 发送专用文本，在 B 的 GUI 历史中看到；对 D 的显式发送不被接受。
6. 重启 A 的后台，再重启第三方 C 的后台，重新查询 A 的设备组。

预期：查询可用，已完成事项不重开；若有独立新事项，提供可解释的资料。
实际：A 的设备组查询持续返回 503；同一后台其他状态查询正常。GUI 保留错误与重查入口，无法完成恢复验收。

### 已保留的证据

主要目录：`.cache/device-group-e2e/run-mtqs98x2/`。
另一次独立复现：`.cache/device-group-e2e/run-mtqprc6c/`。
最终程序再次复现：`.cache/device-group-e2e/run-mtqt6bkz/`。
该轮 `screenshots/failure-a.png` 同时确认 Desktop 已显示读取失败及“重新检查”入口；没有将查询失败隐藏为正常主界面。

| 文件 | 用途 |
| --- | --- |
| `build.json` | 实际运行版本和程序校验值 |
| `decisions.jsonl` | 使用测试角色及本轮事项编号的选择前后对照 |
| `screenshots/cross-completed.png` | 远端组选择已完成 |
| `screenshots/G01-received-history.png` | B 窗口实际收到专用文本 |
| `screenshots/failure-a.png` | 重启后的失败界面 |
| `failure-diagnostics.json` | 各配置的公开查询结果分类 |
| `engine-events.jsonl` | 各配置按 UTC 时间合并的 Engine 事件；删除身份、名称、消息正文和鉴权资料 |
| `engine-evidence.json` | 日志数量及无法读取的角色；本轮无缺失 |

2026-09-07 05:12:35.079Z 起，A 的 Engine 查询日志反复出现：

```json
{"role":"a","target":"uc_engine::operations::space::device_group_choice","span":"api.member.get_device_group_choices","error_kind":"device_trust_unavailable"}
```

最终诊断同时确认：

| A 的查询 | 结果 |
| --- | --- |
| `/encryption/state` | 200，initialized=true，sessionReady=true |
| `/member/device-group-choices` | 503，runtime_unavailable |
| `/member/protection` | 200，mode=ready |

因此不能用未解锁、整个后台未启动或 GUI 无响应解释此轮失败。尚未证明 Engine 内部哪一份恢复资料缺失，不能把下列线索当作已定位根因。

### 建议 Engine 侧核查的位置

以下路径均相对同级 Engine 仓库：

- `crates/uc-application/src/space/membership/query_device_trust/use_case.rs`：查询依据已验证历史中的 active 成员读取 observations；缺失资料及下层账本不可用均可归为 Unavailable。
- `crates/uc-infra/src/space/adapters/device_trust_observations.rs`：成员投影读取不到时不会产生 observation。
- `crates/uc-infra/src/security/v3_membership_branch_transition/mod.rs`：远端分支的暂存、晋升和恢复。
- `crates/uc-infra/src/security/space_control_generation/mod.rs`：目标账本、成员关系和确认进度的生成与持久化。
- `crates/uc-engine/src/assembly/wire/mod.rs`：实际关系存储使用 control database，不应凭猜测改成其他数据库。

建议把第 6 步缩成 Engine 自身的真实多实例回归测试，并在查询失败分支增加不含业务身份的错误分类，以区分已验证历史不可读与成员投影缺失。不能在 Desktop 补造设备资料或隐藏已完成事项来绕过。

## 第二条复现：五配置逐项选择后，A 的查询不可用

```sh
npx --yes --package=node@24 node e2e/conflict-suite.mjs four new-peer
```

此命令实际启动五个配置：A 暂停，B 邀请全新 E，等待 B/C/D/E 准入关系确认；B 移除 C，A 独立移除 D，随后恢复所有后台。
A 的远端候选显示此前本机快照没有的 E。新增 E 与移除 C 可能是不同事项，测试按实际顺序在各窗口逐项完成，保留 E 并接受移除 C；C 的退出经过二次确认。同一设备再次出现已完成的 issueId 会明确失败，不会无限重复选择。

最新证据 `.cache/device-group-e2e/run-mtqsmtmg/`：

- `decisions.jsonl` 记录 A 完成远端历史选择、随后接受移除，B 保留对应组、C 确认退出、E 接受移除。
- `screenshots/new-peer-e-4-completed.png` 显示 E 的选择完成。
- 最终 A 的 `/member/device-group-choices` 返回 503 / runtime_unavailable；B/C/D/E 同一路由均返回 200，issues 为空。
- 五个配置均 initialized=true、sessionReady=true；A 的 protection 为 ready。
- `engine-events.jsonl` 含 A 的 66 条 `device_trust_unavailable`。该文件没有缺失角色。
- 本轮不需主动重启即可失败；尚未执行到要求的 A→E 实际文本同步，因此 G04 不通过。

与远端组重启案例错误分类相同，但不能仅据此断言内部根因相同。修复需要分别重跑两个案例。

最终程序再次运行的结果为 `.cache/device-group-e2e/run-mtqtag2b/`：五个配置的查询均成功、issues 均为空，
但 A 对 E、E 对 A 的关系仍为 `unverifiable` / `paused_unverifiable`，没有达到可同步条件。
因此同一五配置场景至少观察到两种失败表现：资料查询不可用，或资料可查询但双方无法验证关系。
本轮超时为 `new peer is confirmed on both sides`，没有执行到文本发送步骤，不能记作 G04 通过。

## 已排除的两类误判

- 测试等待“重新查询”时，窗口已经自动变成“完成”：属于测试等待竞争，已修复；不属于 Engine 选择未完成。
- 五配置加入 E 后又移除 C，可能依次出现“加入历史分歧”和“移除 C”两件事。只完成第一件便等待 A/E 同步是不完整操作；旧超时不能单独作为 Engine 故障证据。上节使用补齐逐项选择后的新结果。

## 分享与重跑限制

`run.json`、基线及 userdata 含专用测试身份和凭据，只供受控本机调查，不直接公开打包。公开分享使用本文、脱敏事件、结果分类及专用测试名称截图。Engine 版本改变后基线校验会拒绝旧版本，需明确归档后重新生成；详细命令见使用说明。
