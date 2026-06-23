# 移动端 `contentId` 去重接入设计 (sync_engine reducer)

状态：设计已过多维度对抗审查 (2026-06-23,5 维度 + 对抗验证),修正已内嵌 (见各节 `评审修正` 标注)· 作者交接给移动端团队
关联：服务端已实现 `GET /SyncClipboard.json` 的 `contentId`(commit `c6ff804d1`);
读源切 active register(commit `f01f77cce`)。本文只设计 **客户端去重 reducer** 如何
消费 `contentId`,不含服务端。

> **评审摘要**:主路径设计成立 (contentId 作不透明身份、双 Some 只看 contentId、
> Converged 真值门保持 hash-only、向后兼容回退 hash 均经代码核对站得住)。落地前
> **唯一 Blocker 是 M1**(§4.5 漏 `mark_staged_applied` 调用点);M2(§5 FFI 面)、
> M3(§4.6 测试) 为 should-fix。审查另证伪了若干"致命级"虚惊，见文末 §10。

---

## 1. 要解决的问题 (回环)

手机 push 一张 JPEG → 服务端 **异步重编码成 PNG** → 下次 `GET` 时 `hash` 从 `A` 变 `B`。
当前 reducer 的去重链全部以 `hash` 为键，`plan_after_server_get` 在
`crates/uc-mobile-proto/src/sync_engine.rs:320`:

```rust
if !hashes_equal(entry.hash.as_deref(), st.last_synced_hash.as_deref()) {
    return ServerRoute::ServerNew(...);   // B != A → 判新内容 → 建第二张卡
}
```

`contentId` 是服务端 **入库时算定、不随重编码改变** 的跨设备身份。让 reducer 在
`hash` 变了但 `contentId` 没变时判定为"同一条内容，已同步",即可消除这张多余的卡。

---

## 2. 不变量与边界 (先划清楚，避免设计跑偏)

- **device 侧没有 `contentId`。** `device_hash` / `history_head_hash` /
  `last_applied_hash` 都是设备本地或 history 记录的 hash;本机剪贴板不会有服务端
  blake3 身份。因此 `contentId` **只适用于"server entry vs 已同步状态"这一处比较**,
  不进入 Converged 真值门 (server vs device) 和 push 侧判定 —— 那两处保持 hash-only。
- **History 通道保持 hash-only。** History API wire 未变 (`HistoryRecord` 无
  `contentId`),所以 `history_head_hash` 去重不能用 `contentId`。纯经 history 浮现的
  重编码图片仍可能重复 —— 与服务端 §8 决策一致，**本次不在范围内**。
- **向后兼容。** 旧服务端不发 `contentId` → `entry.content_id == None` → 自动回退到
  现有 hash 比较，零行为变化。
- **`contentId` 当不透明字符串整体比较**(`"blake3v1:<hex>"`),不解析、不规范化大小写。
  与 `hash` 的 uppercase 规整体系 **分开**,不要混用 `hashes_equal` 的大写化逻辑。
- **服务端 `contentId` 不变量 (跨仓库硬依赖契约，M5)。** `is_already_synced` 双 Some 时
  完全忽略 hash、把同步身份押在 `contentId` 单一维度，故必须显式声明服务端契约：
  `contentId` 对同一原始字节 **稳定**、对不同字节内容 **全局不复用、一旦分配不回收**。
  由服务端 commit `c6ff804d1` 保证 (`blake3v1:<hex>` 为入库即冻结的内容哈希，按构造不碰撞、
  不回收)。服务端不在本 repo，本契约是客户端对它的依赖，任何服务端身份算法变更都要回看此处。
  > 注：不要把 `(Some,Some)` 比较"放宽"成 `a==b || hashes_equal(...)` —— 析取只会更松、
  > 无法防误吞新内容;合取又会破坏本设计要解决的重编码回环。**保持只信 `contentId`** 是对的，
  > 缺的只是上面这条契约声明。

> **评审修正 (M8)· History 通道不一致的用户可见形态**:§2 已声明 History 通道保持 hash-only、
> 重编码图片经 history 浮现仍可能重复"本次不在范围内"。补充其 **可观测形态**:同一张重编码图片，
> latest 卡去重为 1 张，但 history 列表仍是 2 条记录，用户会看到列表里有"看似重复"的两条。
> 这是有意 scope 切割，但应登记为 tech-debt(后续要么扩 `HistoryRecord` wire 带 contentId、
> 要么在 client 渲染层按 contentId 去重)。"单一真相源"原则出处见 `AGENTS.md`。

---

## 3. "何时学到 C":时间线 (本设计最关键的一点)

push 那一刻客户端不知道 `contentId`(服务端才算)。所以 `last_synced_content_id`
必须在 **push 之后的某次 GET** 学到。完整时间线：

| 步骤 | 事件 | reducer 动作 | `last_synced_hash` | `last_synced_content_id` |
|---|---|---|---|---|
| 1 | 设备 push JPEG(hash A) | `commit_push(A)` | `A` | **`None`**(push 不知道 C) |
| 2 | GET(重编码前):entry hash A, contentId C | hash A == synced A → 非 ServerNew → **Converged**(device 仍是该 JPEG)→ `commit_converged(A, C)` | `A` | **`C`**(在此学到) |
| 3 | 服务端重编码 JPEG→PNG | —(异步，客户端无感) | `A` | `C` |
| 4 | GET(重编码后):entry hash B, contentId C | hash B ≠ synced A,**但 contentId C == synced C** → 非 ServerNew → 不建卡 ✓ | … | … |

**正常路径：`contentId` 在第 2 步的 Converged commit 学到。**
- 前提：第 2 步时设备剪贴板仍是刚 push 的内容 (device_hash == server_hash → Converged 成立)。
- 边界：若用户在 push 与重编码之间 (几秒窗口) 又复制了别的东西，第 2 步走 push 侧
  `DoPush` 而非 Converged → C 学不到 → 第 4 步回退 hash → 仍可能重复。
  这是可接受的退化;§6 给一个可选增强消除它。
  > **评审修正 (M4)**:原文写的 `SkipAlreadySynced` 是分支名错误。`SkipAlreadySynced`
  > (`sync_engine.rs:353-354`) 严格要求 `device_hash == last_synced_hash`,而"又复制了别的
  > 东西"恰恰保证 `device(X) ≠ synced(A)` → 实际落 `PushDecision::DoPush`(`:359`)。结论
  > (此路径学不到 C、第 4 步仍可能重复) 不变，仅分支名订正。

---

## 4. 状态与改动点 (实现清单)

### 4.1 `Clipboard` wire codec(`uc-mobile-proto/src/clipboard_doc.rs`)
- 加字段 `pub content_id: Option<String>`,serde:
  `#[serde(rename = "contentId", default, skip_serializing_if = "Option::is_none")]`。
- **现状是 bug**:没这个字段 → serde 默默丢弃服务端发来的 `contentId`。
- 注意 `clipboard_doc.rs` 头部的 BYTE-CRITICAL 字段顺序不变量：`contentId` 作为新增
  可选字段 **放在 `hash` 之后的明确位置**(不要写"末尾或 hash 之后"这种二义措辞),
  同步更新 BYTE-CRITICAL 注释块 (`clipboard_doc.rs:12-27`) 使其含 `contentId`。
  golden vector 测试相应更新 (它是 client 解码侧，与服务端 `SyncClipboardDoc` 是两份，
  见服务端交接说明)。
  > **评审修正 (M8)· golden 影响澄清**:加 `contentId` 是低风险改动，现有 golden 测试
  > **不会破**——client 字段顺序不影响跨端互操作，server wire `SyncClipboardDoc` 本就与
  > client 字段顺序不同，JSON 解码不依赖顺序。§8 需显式列出：更新
  > `encode_emits_fields_in_swift_declaration_order` golden，并保留一条"旧 fixture 无
  > `contentId` 键仍按原字段顺序解码"的回归断言。
- **评审修正 (M8)· 机械改动量提示**:`Clipboard` 未派生 `Default`,新增字段需手动更新
  uc-mobile-proto 内约 8 处逐字段字面量 (`clipboard_doc.rs` 的 `new`/`from_text`/`publish_*`、
  `sync_engine.rs` 测试夹具) 以及 `history_log.rs` 的 `Clipboard::new` 调用点。落地时把这批
  机械改动一并预期，避免逐个编译错误来回。

### 4.2 `SyncRuntimeState`(`sync_engine.rs:92`)
- 加 `pub last_synced_content_id: Option<String>`(镜像新的 App Group 持久化键
  `lastSyncedContentId`)。
- 加 `pub staged_content_id: Option<String>`(与 `staged_server_hash` 并列)。
- `Default` 补 `None`。

### 4.3 去重比较 (核心)
新增纯函数：
```rust
/// server entry 是否就是"已同步"的那条内容。
/// content_id 是权威身份:两边都有 content_id 时只看它(忽略 hash,跨重编码稳定);
/// 任一侧缺 content_id 时回退到现有 hash 比较(向后兼容 / C 尚未学到)。
fn is_already_synced(entry: &Clipboard, st: &SyncRuntimeState) -> bool {
    match (entry.content_id.as_deref(), st.last_synced_content_id.as_deref()) {
        (Some(a), Some(b)) => a == b,
        _ => hashes_equal(entry.hash.as_deref(), st.last_synced_hash.as_deref()),
    }
}
```
改 `plan_after_server_get`(`:319-323`):
```rust
if let Some(entry) = &snap.server_entry {
    if !is_already_synced(entry, st) {
        return ServerRoute::ServerNew(plan_server_new(st, snap.auto_apply, entry));
    }
}
```
> Converged 真值门 (`:307-317`)**不改** —— 它是 server-hash vs device-hash,device 侧无
> content_id。

### 4.4 staged 去重 (`plan_server_new` `:329-343`)
`already_staged` 同理优先按 content_id:
```rust
let already_staged = match (entry.content_id.as_deref(), st.staged_content_id.as_deref()) {
    (Some(a), Some(b)) => a == b,
    _ if entry_has_hash => st.staged_server_hash.as_deref()
        .is_some_and(|s| hashes_equal(Some(s), entry.hash.as_deref())),
    _ => st.staged_entry.as_ref() == Some(entry),  // hashless 兜底,见下方 M6 注释
};
```
> **评审修正 (M6)· hashless 兜底比较域被动扩大**:`Clipboard` 派生了 `PartialEq`
> (`clipboard_doc.rs:77`),新增 `content_id` 字段后 `==` 会把 `content_id` 纳入逐字段比较。
> 代码文本未变，但"维持现状"这句不准确——全结构体相等的比较域扩大了。窄边界、低概率
> (需服务端对同一 hashless 内容两次观察发出不同/缺失 `content_id`),且只多一次 stage、
> 不污染水位线。处理:hashless 兜底比较时 **排除 `content_id`**(或自定义内容相等),
> 并加一条 staged 单测 (hashless + content_id 一侧有一侧无)。
>
> **评审修正 (M1 附带)· contentId 命中但 hash 漂移**:`already_staged` 经 `(Some C, Some C)`
> 命中后，`staged_server_hash`/`staged_entry` 仍是旧的 `A`/旧 bytes;若 hash 已漂移到 `B`,
> 后续 `mark_staged_applied`(见 §4.5 M1) 会写出陈旧 bytes 和陈旧水位线。命中但 hash 漂移时
> 应 **刷新 `staged_server_hash`/`staged_entry` 为当前 server entry**。

### 4.5 commit 点 (在哪写 `last_synced_content_id`)
把 `advance_synced` 扩成同时收 content_id，并保证它与 `last_synced_hash` **一致**:
```rust
fn advance_synced(st, hash: Option<&str>, content_id: Option<&str>) {
    st.last_synced_hash = upper_nonempty(hash);
    st.last_synced_content_id = content_id.map(str::to_string); // 不做大写化
}
```
各 commit 传值：
- `commit_converged(st, server_hash, server_content_id)` —— **学到 C 的主路径**;签名加
  `content_id`,从 server entry 取。(`:378`)
- `commit_apply(st, hash, content_id, …)` —— apply 的是 server entry，带其 content_id。(`:389`)
- `commit_push` / `commit_consent_push` —— 设备 push,**无 content_id → 传 `None`**。
  关键：必须把 `last_synced_content_id` 置 `None`(push 换了内容但还不知道其服务端身份),
  否则残留的旧 C 会误判。(`:430` / `:461`)
- `commit_stage` / `commit_apply_failed` —— 同时存 `staged_content_id = entry.content_id`。
  (`:413` / `:406`)
- **`mark_staged_applied`(`:580-590`)—— 🔴 M1 Blocker，原清单遗漏的第 5 个调用点。**
  用户手动点击 staged 条目转为已应用时走这里，它在 `:581` 已 clone `staged_server_hash`,
  必须 **顺手 clone `staged_content_id`** 一并传给 `advance_synced`(把
  `staged_content_id → last_synced_content_id` 提升，与已有的 `staged_server_hash →
  last_synced_hash` 提升对称)。

> **🔴 评审修正 (M1)· must-fix Blocker**:`advance_synced`(定义 `:704`) 共有 **5 个** 调用点
> ——`:379`(converged)/ `:395`(apply)/ `:441`(push)/ `:467`(consent_push)/
> `:584`(mark_staged_applied)。原清单只覆盖前四个，漏了 `mark_staged_applied`。改签名后此处
> 要么编译不过、要么被迫传 `None`。后果:**auto-apply OFF 的用户每次手动 apply 一张图，
> `last_synced_content_id` 被清成 `None` → 下次 GET 重编码后的 `B` 无 `C` 可比 → 回退 hash →
> 仍误建第二张卡**;本设计要消灭的回环从 auto-apply 漏到 manual-apply 路径。修复源数据已具备
> (`staged_content_id` 由上面 `commit_stage` 存入)。**实现时务必明确 `advance_synced` 改签名后
> 全部 5 个调用点的 `content_id` 取值。**

### 4.6 跨进程持久化 (App Group)
- 新持久化键 `lastSyncedContentId`(native shell 读写，与 `lastSyncedContentHash` 并列)。
- `PreambleSnapshot`(`:141`) 加 `persisted_synced_content_id: Option<String>`;
  `plan_preamble` 的 cross-process resync(`:222-229`) 在 fold `persisted_synced_hash` 时
  **一并** fold content_id，保持两者一致 (Share Extension 走 push 路径、不知道 C，会写
  content_id 缺省 → fold 后 `last_synced_content_id = None`,与"push 后待学"一致)。
> **评审修正 (M8)· fold 闸门与原子同写契约**:现有 fold 被包在
> `if !hashes_equal(persisted, current)` 条件内 (`:222-229`)。**不要** 只把 `content_id`
> 塞进同一个 if 体——否则 hash 相同但 `persisted content_id` 变了的跨进程场景不触发回填，
> 残值不一致 (跨进程残值是历史高发误判源)。正确做法:fold 闸门改成 **两键整体快照比较**
> (hash 或 content_id 任一不同即 resync),而非"hash 不等做闸门、content_id 搭车"。
> 同时写死持久化契约：`lastSyncedContentHash` 与 `lastSyncedContentId` 必须 **原子同写同清**;
> Share Extension push 路径须把 content_id 键 **显式写为 absent**,而非不碰旧值。
> 另注：`plan_preamble` resync 比较 hash 用的是 `hashes_equal` 的大写化，`content_id`
> **不可混用同一比较**,按 §2 不透明整体相等。

---

## 5. FFI 面 (uc-mobile，涉及 UniFFI → iOS 重新接绑定)
- `ClipboardMeta`(`uc-mobile/src/client.rs:158`) 加 `pub content_id: Option<String>`;
  `from_proto` / `into_proto`(`:175-199`) 透传。`get_latest_with`(`:1086`) 已解码到
  `ProtoClipboard`,字段加上后自动带出。
- `reducer.rs` 里 `SyncRuntimeState` / `PreambleSnapshot` 的 FFI 镜像同步加字段。
- ⚠️ `ClipboardMeta` 是 `uniffi::Record`,加字段会改 Swift/Kotlin 生成初始化签名 →
  iOS 壳所有构造点要更新 + 重新出 `uc-mobile-v*` xcframework。
- **🟠 评审修正 (M2)· 漏列的 FFI 破坏点**:`reducer.rs:518` 的
  `commit_converged(state, server_hash: String)` 与 `:526` 的
  `commit_apply(state, hash: Option<String>, …)` 是独立的 `#[uniffi::export]` 包装函数，
  且只收标量 hash、拿不到 entry 的 `content_id`。要透传 `content_id`,这两个的**入参必须新增
  `content_id`** —— 同样改 UniFFI 生成的 Swift/Kotlin 函数签名，属 FFI 破坏性变更、需重出
  xcframework。**范围仅这两个**:`commit_stage`/`commit_apply_failed`(已收完整
  `entry: ClipboardMeta`)、`commit_push`/`commit_consent_push`(传 `None`、无新入参) 签名都
  **不变**,其破坏性已被上面 `ClipboardMeta` Record 变更覆盖，不要并入。
  > `ServerGetSnapshot.server_entry: Option<ClipboardMeta>` 在 `ClipboardMeta` 拿到字段后
  > 自动携带，无需单独列入。

---

## 6. 可选增强 (消除 §3 的边界)
让 "已同步但 hash 未变" 的路径也能学到 C:在 `plan_after_server_get` 命中
`is_already_synced`(经 hash 命中) 且 `entry.content_id.is_some()` 而
`st.last_synced_content_id.is_none()` 时，发一个轻量 commit 仅回填
`last_synced_content_id`。这样即便第 2 步没走 Converged，也能在任何"内容没变"的 tick
学到 C。代价:plan 是纯函数，得新增一个 `commit_learn_content_id` 由 shell 调用，或让该
tick 返回一个 "learn" 信号。**建议先不做，等真机验证 §3 主路径覆盖率后再定。**

> **评审修正 (M8)· 真要做时写到可实现粒度**:当前所有 commit 都不经过"hash 命中
> already_synced"这条路径，故本增强需要新载体——二选一并写死:(a) 扩 `ServerRoute` 形状多带
> 一个 `learn_content_id: Option<String>` 信号由 shell 落地，或 (b) 新增
> `commit_learn_content_id(st, content_id)`。另需给"是否要做"一个客观判定标准 (如真机统计
> §3 主路径 Converged 命中率低于某阈值才做),而非凭感觉。

---

## 7. Swift parity
sync_engine 标注 "Swift sources are the NORMATIVE reference"。以上每条状态/commit/比较
改动都要在 `SyncEngine.swift` 同步，并保持 Rust port 与 Swift 逐行对应;App Group 持久化
键、`ClipboardMeta` 的 Swift init 同改。建议 Rust 与 Swift 同一轮改、共用本设计。

> **评审修正 (M8)· 审查盲区声明**:`SyncEngine.swift` **不在本 repo**(在并行维护的 iOS
> UniClipboard 仓库),本设计所有 Swift parity 断言无法在本 repo 验证。运行真相已核实：
> 决策逻辑 (`plan_*`/`commit_*`) 跑在 Rust(经 UniFFI),Swift 只是 execution shell
> (`reducer.rs:9-17`);"Swift normative"是移植溯源，iOS 去重实际走的就是这份 Rust——
> 不存在另一份独立生效的 Swift reducer(见 §10 证伪项 #4)。

---

## 8. 测试计划
- `clipboard_doc` golden:`contentId` 解码/省略/`null`-不发/缺省可解码 (对齐服务端 4 个
  契约测试)。
- `sync_engine` 单测 (纯函数，易覆盖):
  - **回环主用例**:synced=(A,C),server=(B,C) → 非 ServerNew(核心断言)。
  - synced=(A,C),server=(B,**C2**) → ServerNew(真不同内容)。
  - 旧服务端:server.content_id=None → 退化到 hash 比较，行为同改前。
  - push 后 commit:`last_synced_content_id` 被清成 None。
  - Converged commit 学到 C:commit_converged(A,C) 后 state.last_synced_content_id==C。
  - staged:staged=(_,C),server=(B,C) → already_staged=true。
- FFI round-trip:`ClipboardMeta` content_id 过 proto ↔ FFI 往返。

### 8.1 评审补强：测试矩阵缺口 (M3 + M7)
> 原 §8 矩阵存在以下缺口，落地前一并补：
- **🟠 M3 · cross-process resync fold 零覆盖**:§4.6 的 fold content_id 逻辑 §8 完全没测。
  需先给 `PreambleSnapshot` 加 `persisted_synced_content_id` 字段，再补两条 `plan_preamble`
  用例:(a) hash 与 content_id 一并提供 → fold 后两者一致;(b) Share Extension 场景
  (`persisted_synced_content_id=None` 但 `persisted_synced_hash=Some`)→ fold 后
  `last_synced_content_id` 被置 `None` 而非保留旧值。(现有 `preamble_cross_process_resync_*`
  只断言 hash。)
- **`(Some,None)` fallback 分支零覆盖**:现仅测 entry 侧 None(旧服务端),缺
  `entry.content_id=Some(C) ∧ st.last_synced_content_id=None`(C 未学到的退化路径，正是 §3
  边界判定关键)。补：`synced=(A,None), server=(B,Some(C)) → ServerNew`。
- **`commit_push` 清空后紧接 GET 的序列测试**:现只测单点 `last_synced_content_id==None`,
  缺端到端序列证明清空真的阻止误判。补：`commit_push` 清 C → 再用 `content_id=Some(oldC)`
  的 entry 跑 `plan_after_server_get`,断言不误命中。
- **staged 负向用例**:现只有正向 `staged=(_,C),server=(B,C)→true`,补对称
  `server=(B,C2)→false` + `server.content_id=None` fallback 回退用例。
- **FFI round-trip 两态 + passthrough**:现只写 Some 单条 happy-path。补 `content_id=None`
  态往返一致，并断言 `into_proto`/`from_proto` 对 `content_id` **纯透传**(不大写化/规范化，
  与 §2 一致;`client.rs:175-199` 对 hash/size 有非平凡映射，需钉死 content_id 不走那套)。
- **golden 字段顺序回归**:见 §4.1 M8——更新 swift-declaration-order golden，并保留旧 fixture
  无 contentId 键的字段顺序回归断言。

---

## 9. 落地顺序建议
1. `clipboard_doc.rs` 加字段 + golden(纯解码，零风险;注意 §4.1 M8 机械改动量)。
2. `sync_engine.rs` 状态 + `is_already_synced` + commit 点 (**含 M1 的 5 个 `advance_synced`
   调用点**)+ 单测 (协议关键，重点评审)。
3. `uc-mobile` FFI Record + reducer 镜像 (**含 M2 的 `commit_converged`/`commit_apply` 包装签名**)。
4. 同步 `SyncEngine.swift` + App Group 键 + Swift 调用点;出新 xcframework。
5. 真机验证 §3 主路径;按覆盖率决定是否做 §6 增强。

---

## 10. 评审结论汇总 (2026-06-23 多维度对抗审查)

### 落地前 must-fix 清单
- [ ] **🔴 M1(Blocker)** — §4.5 commit 清单补 `mark_staged_applied`(`sync_engine.rs:584`),
      明确 `advance_synced` 改签名后全部 5 个调用点的 `content_id` 取值。否则 auto-apply OFF
      用户的去重在 manual-apply 路径完全失效。
- [ ] **🟠 M2(Major)** — §5 补 `reducer.rs` 的 `commit_converged`/`commit_apply` 两个
      `#[uniffi::export]` 包装函数签名变更属 FFI-breaking(范围仅这两个)。
- [ ] **🟠 M3(Major)** — §8.1 补 §4.6 cross-process resync fold `contentId` 的两条
      `plan_preamble` 用例 (含 Share Extension 缺省 → None 场景)。

### Minor/Nit(文档精度,落地前顺手清)
- **M4** §3 分支名 `SkipAlreadySynced` → `DoPush`。
- **M5** §2 补服务端 `contentId` 不碰撞/不回收契约。
- **M6** §4.4 hashless 兜底 `PartialEq` 比较域被动扩大，排除 `content_id` 比较。
- **M7** §8.1 测试矩阵四缺口。
- **M8** §2/§4.1/§4.6/§6/§7 一组措辞/记录/契约瑕疵。

### 对抗验证后被证伪/降级的项 (作者可放心，这些担心是多余的)
1. **【证伪】"主路径 `commit_converged` 拿不到 contentId、致命设计缺口"** —— 错。reducer
   架构是 plan→I/O→commit,shell 调 `commit_converged` 时仍持有 `ServerGetSnapshot.server_entry`
   (`reducer.rs:376`),直接读 `entry.content_id` 传参即可 (即 §4.5 第 124 行"从 server entry
   取"的字面含义)。**不必扩 `ServerRoute::Converged` 变体。**
2. **【证伪】"`commit_push` 必须在 silent-skip 早退分支也清 C，否则残留旧 C"** —— 错且修法
   有害。早退分支 (`:436-440`) 含义是"什么都没 push",此时 `last_synced_hash` 同样不动，
   两字段一起冻结在一致 watermark 上才是正确行为;强清 content_id 而保留旧 hash 反而破坏
   "content_id 与 hash 一致"不变量。
3. **【证伪】"§3 第 2 步学 C 链路走不通，Converged 不携带 content_id"** —— 错，同 #1。
4. **【证伪】"§7 Swift parity 是双实现真相分裂、无冲突裁决"** —— 错。决策逻辑跑 Rust,
   Swift 只是 execution shell(`reducer.rs:9-17`);"Swift normative"是移植溯源，不是另有
   一份独立生效的 Swift reducer。
5. **【证伪】"§5 ClipboardMeta 传播面评估偏乐观、漏 ServerGetSnapshot"** —— 错。§5 已逐名列
   `from_proto`/`into_proto` 且"iOS 壳所有构造点要更新"已兜底;`ServerGetSnapshot.server_entry`
   在 `ClipboardMeta` 拿到字段后自动携带。
6. **【降级 major→minor】"§4.3 (Some,Some) 忽略 hash 是高危丢内容"** —— 因 `contentId` 实为
   内容哈希，丢内容回归风险极低，降为"补契约声明"(见 M5)。
7. **【降级 major→nit】"§4.6 fold 破坏 `plan_preamble` 的 staged_entry 比较"** —— 张冠李戴。
   `plan_preamble` 只 fold `last_synced_hash` 字符串，不触碰 staged_entry;唯一 staged_entry
   `PartialEq` 比较在 plan_server_new(`:337`),已被 M6 处理。
8. **【降级 major→nit】"§4.1 golden vector 冲突评估不足"** —— 核心危害论证基于一处事实错误
   (把 application 层模型当 server wire);加 contentId 低风险，现有 golden 不破。

---

## 11. 落地状态与载体修正 (2026-06-23 实现后回填)

### 11.1 本仓库 Rust 侧：已实现并通过验证

设计 §9 落地顺序的 **步骤 1–3(本仓库 Rust 改动) 已全部完成**,含全部 must-fix(M1/M2/M3) 与 minor(M4–M8):

- **`uc-mobile-proto/src/clipboard_doc.rs`**:`Clipboard` 新增 `content_id: Option<String>`
  (`#[serde(rename = "contentId", default, skip_serializing_if)]`,置于 `hash` 后);更新
  BYTE-CRITICAL 注释;`new`/`from_text`/`publish_*` 字面量补 `None`;新增 golden 测试
  (verbatim 编解码、None 省略、**旧 server 无 contentId 回归**、显式 null、字段顺序)。
- **`uc-mobile-proto/src/sync_engine.rs`**:状态加 `last_synced_content_id` / `staged_content_id`;
  `PreambleSnapshot` 加 `persisted_synced_content_id`;新增 `is_already_synced`(双 Some 只看
  contentId);`plan_after_server_get` 改用之;`plan_server_new` 的 `already_staged` 优先 contentId,
  hashless 兜底用 `clipboard_eq_ignoring_content_id`(**M6**:排除 content_id);`advance_synced`
  扩签名 + **5 个调用点全覆盖**(converged/apply 传 entry 身份、push/consent_push 传 None、
  **`mark_staged_applied` 提升 staged_content_id —— M1 Blocker 已修**);`commit_stage`/
  `commit_apply_failed` 存 staged_content_id;`reset_runtime_state` / `handle_active_server_changed`
  原子同清;跨进程 fold 改 **两键整体快照闸门**(**M8**);新增完整去重测试矩阵。
- **`uc-mobile/src/client.rs`**:`ClipboardMeta` 加 `content_id`,`into_proto` 显式透传 (verbatim)、
  `from_proto` 读出 + 两态 passthrough 测试。
- **`uc-mobile/src/reducer.rs`**:`SyncRuntimeState`/`PreambleSnapshot` FFI 镜像加字段 (双向 From);
  **`commit_converged`/`commit_apply` 两个 `#[uniffi::export]` 包装签名新增 content_id 入参
  —— M2,FFI-breaking**。

验证：`uc-mobile-proto` 271 测试 + `uc-mobile` 78 测试全过;`uc-application`/`uc-webserver` 编译
通过;`cargo fmt` + `clippy` 干净。

### 11.2 🔴 载体修正 (覆盖 §7 的过时断言)

**实现后调研发现：移动端主力是 RN/Expo 跨平台 app(仓库 `uniclipboard-android`,iOS+Android 共用
一套 TypeScript),不是本设计 §7 假设的 iOS Swift app。** 影响：

- **§7「Swift sources are NORMATIVE」已过时。** 真正的 execution shell 是 RN 的 TS
  (`src/services/SyncEngine.ts`),不是 Swift。决策逻辑 (`plan_*`/`commit_*`) 跑在 Rust、经
  UniFFI 暴露 (`modules/uc-core/{ios,android}` binding),RN 的 TS 当 shell 调用——这与设计
  「reducer 跑 Rust、shell 是壳」的架构一致，只是壳从 Swift 换成 TS。
- 另存在一个 **独立的老 iOS Swift app**;`uc-mobile-proto/src/history_log.rs`(2001-date blob /
  App Group UserDefaults) 是给它写的，**与 RN 的 `HistoryStorage`(AsyncStorage / 自有 schema)
  不兼容，对 RN 是孤儿代码**。

### 11.3 RN 接线清单 (让本次 contentId 去重在 RN 真正生效)

本次 Rust 改动是 FFI-breaking 前置，RN repo `uniclipboard-android` 需：

1. **重生成 binding**:rebuild mobile core → 重出 `modules/uc-core/{ios,android}` Kotlin/Swift +
   TS `index.ts` 包装 (新增 `ClipboardMeta.content_id`、`SyncRuntimeState` 两个新字段、
   `PreambleSnapshot` 一个新字段，改 `commitConverged`/`commitApply` 签名)。
2. **改 `SyncEngine.ts`**:`commitConverged`/`commitApply` 调用点传 `server_entry.content_id`;
   `ServerGetSnapshot` 带出 GET 响应里的 contentId;`planPreamble` snapshot 填
   `persisted_synced_content_id`;**AsyncStorage 持久化 `lastSyncedContentId`**(与
   `lastSyncedContentHash` 并列读写)。

工作量：小–中，纯 TS + 一次 binding 重生成。

### 11.4 残留项归属与未做项

- **【未做 · 本仓库 Rust】M1-附带 staged 字节刷新**(contentId 命中但 hash 漂移时刷新
  `staged_server_hash`/`staged_entry`):未实现。原因:plan 函数是纯函数、该 no-op tick 路径无
  commit 落点。**水位线一致性已由 `staged_content_id` 存入 + `mark_staged_applied` 提升保证**
  (contentId 主导去重，不会出现多余卡);残留仅手动 apply 时可能写旧版字节，二者解码同图、纯视觉
  无害。彻底消除需给 reducer 增 learn 载体 (同 §6 性质)。
- **【未做 · 本仓库 Rust】§6 可选增强**(hash 命中 already_synced 时回填 contentId):按设计「先不做，
  待真机覆盖率」,维持。
- **【归属 RN · 残留 3 历史重编码重复】**:历史在 RN 的 TS `HistoryStorage`(AsyncStorage),
  **不经 Rust core**;改本仓库 `history_log.rs` 对 RN 零效果 (schema 不兼容)。解决路径仅在 RN repo:
  (优先)**originHash v2**「服务端内存 map + 客户端 TS 对账」(`uniclipboard-android` 的
  `.planning/.../task_plan.md`),或在 `HistoryStorage.addItem` 的 TS 去重引入 contentId/originHash
  判重。**本设计 §2/§8 的 History tech-debt 在 RN 主力下登记到 RN repo，不在本仓库。**
- **【评估结论 · 历史收敛到 Rust core】**:经跨仓库评估，「有条件值得、非当前第一优先级」。现有
  `history_log.rs` 只覆盖 RN 需求 ~30–40% 且 schema 不兼容 (=大重写非接线);唯一强动机是 originHash
  对账三处统一;推荐存储抽象走**策略 a(Rust 纯逻辑、list/bytes 进出、IO 留 host)**,**rust core
  不需要为 iOS/Android 存储路径做平台适配**(路径由 host 吸收，难点是 schema 差异非路径)。建议在
  originHash v2 方案定调前不启动历史收敛。
