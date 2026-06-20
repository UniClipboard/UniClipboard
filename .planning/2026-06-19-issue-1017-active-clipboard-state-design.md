# ActiveClipboardState 设计决策记录 — issue #1017

- 状态：**v2 定稿（已过 7 维度对抗审查 + 全部决策拍板，2026-06-19）**。核心模型成立；3 条 must-fix 已并入下文。
- 范围：**一次性做完 v1 + v2，不留中间降级态**
- 关联：issue #1017。本文在若干点上覆盖 issue 原文（见 §3）。
- 审查产物：`raised 35 / confirmed 27`（7 high），完整报告见 task `w9patvch3` 输出。

---

## 0. 一句话

把「当前哪一条 entry 是活跃剪贴板」建模成跨设备 LWW 寄存器。本机把它写进 OS
剪贴板，远端广播轻量 state 消息收敛；对端缺内容时按需 pull（复用现有 V3 加密同步链路）。

---

## 1. 核心不变量（审查认定为设计强项，保留）

> **register 前进 ⟺ OS 写入成功 ⟺ re-broadcast（同 LWW key）。三者同生同死。**

pull 失败 / 写失败 / 锁定 / 被 receive 闸门拒绝 → **OS 没写成 → 不前进、不传播**。

**审查暴露的两个现实约束（must-fix #1 / cp-1 / cp-2），不变量随之精化：**

1. **「OS 写入成功」是平台可变的、且入站当前是 fire-and-forget。**
   - X11/Wayland 的 `write_snapshot` 返回 `Ok()` 只代表「成了 selection owner + 缓存了 bytes」，**不代表对端能粘出**（Wayland `set_selection` 可被静默取消，见 `x11/writer.rs:166-192`、`wlr.rs:741-745`）。
   - 入站正常 apply 的 OS 写是 **detached `tokio::spawn`**（`apply_inbound/usecase.rs:320`），`Applied` 在写完成 **之前** 就返回、且从不观测 `Result`——这是为了避免 mobile finalize 被 1–3s 写阻塞而 **故意** 加的。
   - **裁决**：strict「⟺ OS 写成功」耦合 **只用于新的 0xC3 state 驱动路径**（register 前进 + re-broadcast 放进 spawned 写任务的成功分支里执行，不回灌阻塞 finalize）；bulk 0xC1 内容同步路径维持「register 在 capture-commit 前进、OS 写 best-effort」。平台层面 X11/Wayland 视为 best-effort，可选用 #1029 owner-lifetime 轮询做 read-back 校验后再前进（v1 先 best-effort + 记 log）。

2. **LWW key = `(activated_at_ms, activated_by)`，身份键 = `content_hash`（不是 `entry_id`）。**
   - `entry_id` 是 **每设备各自生成的 uuid v4，跨设备永不相等**——用它做「本地是否已有该 entry」「去重」一定 miss。跨设备唯一稳定键是 **`content_hash`**（现有同步去重就用它）。
   - LWW 比较：`incoming` 更新 ⟺ `incoming.ts > cur.ts || (ts == cur.ts && incoming.activated_by 字典序更大)`。
   - **断环**：仅当 `(content_hash, ts, activated_by)` **全键相同** 才判为同一 state → ignore；不要只比 `(entry_id, ts)`。

---

## 2. 锁定决策

### D1 — 寄存器语义 = 「当前 OS 剪贴板内容」；更新点 = `ClipboardWriteCoordinator::write`（**审查改写：放弃「watcher 观测点」**）
- 不变量同 §1：register 永远等于此刻 OS 剪贴板是哪条 entry。
- **❌ 原方案错误**：原 D1 想挂在 watcher 的 `on_clipboard_changed` 上「不碰 capture use case」。审查证伪：
  - 入站 / mobile 的 OS 写都是 `RemotePush` intent，watcher 在 `clipboard_watcher.rs:142-149` **短路 return**，hook 根本跑不到；
  - restore 是 `LocalRestore`，capture use case 在 `usecase.rs:159-162` 直接 `Ok(None)`，拿不到 entry_id。
  - 即「已有的单一观测点」**不存在**，照原文实现 → 入站收敛 + 本机 restore 广播两条主路径 **静默 no-op**，单测还全绿。
- **✅ 新方案**：把 `entry_id: Option<EntryId>` + `ActiveClipboardRegisterPort` **透传进 `ClipboardWriteCoordinator::write`**（`coordinator.rs:54-76`，文档自述「所有程序化剪贴板写入的唯一边界」）。每次 `write_snapshot` 成功后按 intent 更新 register。逐个 call-site 把 entry_id 喂进来（**5 处**）：
  1. restore：`restore_selection.rs:70/88`（有 entry_id）
  2. inbound 正常 apply：`apply_inbound/usecase.rs:184/268`（`receiver_entry_id`）— register 前进 + re-broadcast 放进 spawned 写成功分支
  3. mobile 新内容：走 apply_inbound（同上）
  4. mobile 重复命中：`restore_adapter.rs` → `restore_entry`（LocalRestore）
  5. 真正用户 LocalCapture：额外经 watcher 成功路径接入（这条 watcher 确实能看到）
- 收益不变：LWW 天然保护对端正在用的剪贴板（对端新复制把 ts 推高，迟到旧 state 被拒）。

### D2 — 闸门模型（**审查改写：从死配置 `auto_sync` 改为 per-device `MemberSyncPreferences`**）
- **❌ 原方案错误**：原 D2 gate 在全局 `SyncSettings.auto_sync` / `content_types`。审查证伪：全仓 grep，`auto_sync` 只在 settings model/DTO/projection 出现，**capture/dispatch/ingest 全链路无人读 = 死配置**（UI 还把 `auto_sync=false` 渲染成 "Sync Paused"）。真实闸门是 **per-device `MemberSyncPreferences`**：
  - 出站：`TargetSelector::is_send_allowed` 读 `send_enabled` / `send_content_types`（`target_selector.rs:94-138`）
  - 入站：`ingest_inbound.rs:249-309` 读 `receive_enabled` / `receive_content_types`
- **✅ 新闸门矩阵**（全部走 per-device 实际闸门）：

| 触发源 | 新功能开关 | per-device 出站 | per-device 入站 |
|---|---|---|---|
| History restore → 广播 | `sync_on_restore`(默认 false) | `send_enabled` ∧ `send_content_types` | — |
| Mobile push → fan-out | 不看 sync_on_restore | `send_enabled` ∧ `send_content_types` | — |
| 入站 0xC3 state → 写 OS | — | — | **必须过 `receive_enabled` ∧ `receive_content_types`** |
| 0xC2 pull-serve | — | **不查 send-prefs（仅 member 指纹准入）** | — |

- **0xC2 pull-serve 准入（已拍板 = b）**：维持现有 `clipboard_receiver_adapter.rs:223` 的 member-fingerprint-only 准入，**不** 叠加 `is_send_allowed`。即：即便你把某 member 的 `send_enabled` 关了，它发来 pull 请求仍能拉到内容——**接受这个「向被静音成员被动外泄」的口子**（与主动 push 侧的 send 闸门不对称，是有意为之，换实现简单 + pull 语义纯粹「成员按需取」）。送出的内容仍是 D4 的 V3 加密信封、仍要 sender unlock。

- **入站必须过 receive 闸门**（修 gate-2 反向泄漏）：新 0xC3 是独立 accept handler，若不接 receive 闸门，被用户静音的 peer / 被禁类型仍能写到本机 OS。**裁决**：入站在写 OS 前过 receive 闸门；**被拒则整条丢弃**——不写 OS、**不前进 register**（保持 D1 不变量、且不让被拒条目用其 ts 压制后续合法条目）、不 re-broadcast。loop-safe（不 re-broadcast）。
- **`auto_sync` 全局总开关**：本特性 **不** 单独尊重它（否则成了全仓唯一读它的功能，自相矛盾）。若将来要做总 kill-switch，单开前置任务把它接进现有 dispatch/ingest。

### D3 — pull 目标 = 消息 sender；blob 子路径逐跳 **重新签 ticket**（**审查精化**）
- inline/小文本：pull from **sender**（审查认定正确——sender 必在线、直连、已物化）。
- **❌ blob 大图/文件**：审查证伪「转发原 envelope 逐跳可达」。`issue_ticket` 把 ticket **钉死在签发节点 `self.endpoint.addr()`**（`blobs.rs:285-290`），下游 downloader 直接拨 ticket 里的 provider（`blobs.rs:488-516`）——B 转发 A 的原 envelope，C 会去拨 A，A 离线/不直连就失败。
- **✅ 裁决**：中继节点 B 必须用 **本机已物化的 blob 重新 `issue_ticket`（钉自己）+ 重编码 V3 envelope** 再发下游 C（B 已物化是不变量保证的）。`activated_by` 仅作 LWW tiebreaker / UI，不作 pull 目标。

### D4 — pull 内容走「decrypt → re-encrypt → 重发」（**审查改写：原「免解密直发」是事实错误**）
- **❌ 原方案错误**：原 D4 称「原样发存储里现成的加密 blob、免解密直发、零额外加密代码」。审查全部证伪：
  - **落盘格式 ≠ wire 格式**：盘上是 `UCBL`（`encrypted_blob_store.rs:37`）/ JSON `EncryptedBlob`；wire 是 `V3 'UC3\0'`（`chunked_transfer.rs:45`，per-chunk AEAD，AAD=`transfer_id‖chunk_index`）。接收方 **硬拒** 非 `V3_MAGIC`（`chunked_transfer.rs:462-463`）→ 直接 `InvalidFormat`。
  - 现有唯一再发路径 `ResendEntryUseCase` 就是 **decrypt-then-reencrypt 且必须 unlock**：`reconstruct_snapshot_from_entry`（读盘→解密→物化明文）→ `encode_snapshot_with_blob_refs_to_v3_bytes` → `TransferCipherPort::encrypt`（**新 `Uuid::new_v4` transfer_id**），锁定时返回 `NotUnlocked`。
- **✅ 新方案**：pull serve **复用这条现成出站链路**（reconstruct → encode V3 → TransferCipher.encrypt 新 transfer_id）。
  - **新增硬约束**：**serve pull 要求 sender 处于 unlock**（要解密 + 物化）→ 锁定 sender **服务不了 pull**（`NotUnlocked`）。这 **强化了 D5**，但也意味着 pull 多一个失败模式（sender 锁定）。
  - 拆两条子路径：(a) inline/小文本复用 V3 inline ciphertext；(b) blob 走 D3 的「中继重签 ticket + 重编码」。
  - 「字节级零拷贝直发」若仍想要，是 **独立新设计项**（需 存储↔wire 格式桥 + AAD 重绑），非零代码，不进本特性。

### D5 — 砍掉 passive relay；锁定设备 = 完全惰性（**审查认定为强项，不变**）
- 锁定设备收到 state：不更新 register、不写 OS、不 re-broadcast，纯丢弃 + log。
- 与红线「传输=存储=master_key、锁定无法解密」一致，并被 D4 反向印证（锁定连 pull 都服务不了）。

### D6 — pull 失败处理 + **收敛保证范围收窄**（**审查精化**）
- OS 没写成（pull 超时/sender 离线/写失败/被闸门拒）→ 不前进、不 re-broadcast、记 log。**不做专门 retry 循环**。pull 超时 **10s**。
- **❌ 收敛范围**：审查证伪「无条件 O(n)/链式可达」。peer-online resync 是 presence 驱动，presence/addr **只覆盖直连 peer**（`presence_monitor.rs` / `peer_addr_repo`）。链式 A–B–C（A 不连 C）中，pull 瞬时失败或桥 B 在 broadcast→pull 窗口离开，C 可能被永久饿死。
- **✅ 裁决（v1）**：**收敛保证范围收窄到直连/星形拓扑**；非直连子树收敛标记为 **best-effort（不保证）**，与 D6 无 retry 一致。文档不再宣称无条件链式可达。（可选增强：桥上加有限 re-pull + peer-online 按 content_hash re-pull-by-want，留 v2+。）

### D7 — Debounce（不变）
- 出站 restore 广播合并 **~300ms**；peer-online resync 合并 **~1.5s**。

### D8 — 持久化 / 重启（**审查加固：条件原子 UPSERT + activated_at_ms 定义 + 启动 reconcile**）
- SQLite **单行表** `{content_hash, entry_id, activated_at_ms, activated_by}`（身份键以 content_hash 为主，见 §1）。
- **条件原子写**（修 cp-5，从原 §6-TBD 提升为锁定）：register 前进必须是 **单条条件语句**
  `UPDATE register SET ... WHERE ? > activated_at_ms OR (? = activated_at_ms AND ? > activated_by)`，
  让 LWW-loser 在 SQL 层成为 no-op；**禁止** copy-paste 现有 `peer_address_repo.rs:59-80` 那种无条件 `do_update()` LWW 覆盖。或：所有 register 变更串行化到单 actor 做 in-process CAS。
- **`activated_at_ms` 定义**（修 reg-2/reg-5）：= **本次激活事件** 发生时的 wall-clock，**与 `created_at` / `snapshot.ts_ms` 无关**；dedup-resurface（重新复制旧条目，`touch_entry` 只 bump active_time）**必须重戳为 now**，不得沿用 `created_at`。入站则逐字采用 S 的 ts（§4 line 132），「一次激活戳一次、所有副本继承」规则同样套到 capture/restore。
- **重启**：持久行只作 **untrusted 基线**；启动时 **读实际 OS 剪贴板 hash 与持久行 reconcile**（D1 既然说 register==OS），**不** 盲信旧行直接当基线（修 cp-3 的崩溃回归窗口）。不回写 OS、不主动广播。

### D9 — LWW 时钟（**审查加固：未来 ts 守卫**）
- wall-clock ms（i64）+ `activated_by` 字典序 tiebreaker，全序。接受 v1 时钟漂移风险。
- **新增**：拒绝「比本机 wall-clock 超前 > X 秒」的 incoming ts，限制快钟设备压制真实激活（X 待定，建议 ~300s）。

### D10 — peer-online 对称收敛（不变）
- 检测到 peer 上线 → 本机把当前 register 发给它；两端各发、靠 LWW 互相收敛（debounce 见 D7）。

### D11 — resend 共存（不变）
- `ResendEntryUseCase` 不动、并存、不废弃。其 reconstruct/encode/encrypt 链路被 D4 复用。

### D12 — 线格协议（**审查精化：0xC2 多路复用要改 read_frame**）
- state 消息字段：`{content_hash, entry_id, activated_at_ms, activated_by}`，postcard。
- state 新 ALPN `uniclipboard/active-clipboard/0`，magic **0xC3**。
- **pull 0xC2 的归属要显式定**：若复用 clipboard ALPN，现有 `read_frame`（`clipboard_wire.rs:307`）**硬拒非 0xC1**，复用就 **必须改 read_frame + receiver 按 magic 解复用**；或干脆把 0xC2 定成 **独立 ALPN sibling**（更干净）。**裁决建议：独立 sibling ALPN**，不动现有 0xC1 帧。
- **威胁模型（已知接受项）**：明文 state 里 `content_hash` = 无盐 `blake3(plaintext)`，对 in-space 方是内容确认/关联预言机——但 **现有 0xC1 `WireHeaderV2.content_hash` 已经在明文里送同样的 hash**，故 0xC3 **不引入新暴露**。停用「不可逆 hash 故无害」措辞；可选 future：`keyed_hash(master_key, ...)`。

---

## 3. 与 issue #1017 原文的偏离（实现以本文为准）

| 点 | issue 原文 | 决策 |
|---|---|---|
| 范围 | v1/v2 分阶段、v1 无 pull、缺 entry 静默 skip | **v1+v2 一起做，pull included，无降级态** |
| 寄存器输入 | 只列 restore/mobile/peer | **所有本机 OS 写都更新，经 `ClipboardWriteCoordinator` + 透传 entry_id**（非 watcher；入站/mobile 是 RemotePush 对 watcher 不可见，restore 在 watcher 是 Ok(None)） |
| Passive 模式 | 更新 LWW + re-broadcast、跳过 OS 写 | **砍掉，锁定纯惰性** |
| pull 目标 | 隐含向激活方拉 | **向 sender 拉**；blob 子路径中继 **重签 ticket** |
| pull 加密 | 不加密 | **复用 V3 decrypt→reencrypt 链路**（非「免解密直发」，**serve 要 unlock**） |
| 出站闸门 | `sync_on_restore` 唯一闸门 | **per-device `MemberSyncPreferences`**（`auto_sync` 是死配置）+ 入站过 receive 闸门 |

---

## 4. 入站 / 出站流程（伪码，已按审查修正）

```text
出站（本机 restore，闸门通过）:
  闸门 = sync_on_restore ∧ is_send_allowed(peer)  // per-device send_enabled/send_content_types
  restore 写 OS 成功 → update register{content_hash,entry_id,now_ms,self}
                     → broadcast state(0xC3) to allowed peers (debounce 300ms)

入站（收到 peer 的 state S，sender = P）:
  if 锁定: drop + log; return                                  // D5
  if !S.is_newer_than(register): ignore; return                // 全键比较，断环
  if !receive_allowed(P, S):    drop + log; return             // D2 入站闸门：不写不前进不传播
  if 本地有 content_hash(S.content_hash):                       // 注意:键是 content_hash 不是 entry_id
      reconstruct → write OS (spawned)
  else:
      pull from P (10s, 复用 V3 decrypt→reencrypt; blob 走中继重签 ticket)   // D3/D4
      if 失败: log; return                                     // D6: 不前进/不传播
      store entry → write OS (spawned)
  on OS 写成功(spawned 成功分支):
      conditional-UPSERT register = S(同 key)                  // D8 SQL 层 CAS
      → re-broadcast state(同 key) to allowed peers            // 核心不变量

peer-online（检测到 peer Q 上线）:
  把本机当前 register 发给 Q（debounce 1.5s，对称）            // D10
```

---

## 5. 技术锚点（recon + 审查核对）

**核心 seam（must-fix）**
- 写入边界（register 更新点）：`crates/uc-application/src/clipboard_write/coordinator.rs:54-76`（`ClipboardWriteCoordinator::write`，唯一程序化写边界，3 intent）。
- watcher 短路（**不能** 挂这）：`apps/daemon/src/daemon/workers/clipboard_watcher.rs:142-149`（RemotePush 短路 return）。
- capture 短路：`crates/uc-application/src/clipboard_capture/usecase.rs:159-162`（LocalRestore → Ok(None)）。
- 入站 detached 写：`crates/uc-application/src/usecases/clipboard_sync/apply_inbound/usecase.rs:320`（tokio::spawn，先返回 Applied）；`receiver_entry_id` 在 `:184/268`。
- restore entry_id：`crates/uc-application/src/usecases/clipboard_restore/restore_selection.rs:70/88`。
- mobile 重复→restore：`crates/uc-application/src/facade/mobile_sync/restore_adapter.rs`（→ `ClipboardRestoreFacade::restore_entry`）。

**闸门（must-fix #2）**
- 出站 per-device：`crates/.../target_selector.rs:94-138`（`is_send_allowed` / `send_enabled` / `send_content_types`）。
- 入站 per-device：`crates/.../ingest_inbound.rs:249-309`（`receive_enabled` / `receive_content_types`）。
- pull-serve 准入现状：`crates/uc-infra/src/network/iroh/clipboard_receiver_adapter.rs:223-253`（仅 member-fingerprint，不查 send-prefs）。

**crypto/wire（must-fix #3）**
- 落盘加密：`crates/uc-infra/src/security/encrypted_blob_store.rs:37`（UCBL）。
- wire V3：`crates/uc-infra/src/clipboard/chunked_transfer.rs:45`（UC3）/`:462-463` 硬拒非 V3 /`:417` fresh transfer_id。
- blob ticket 钉签发节点：`crates/uc-infra/src/network/iroh/blobs.rs:285-290`（issue_ticket）/`:488-516`（download 拨 provider）。
- 现成再发链路：`resend_entry.rs`（reconstruct → encode V3 → encrypt）。

**新增必改 edit-site（should-fix #4，原 §5 漏列）**
- ALPN 安装：`crates/uc-bootstrap/src/.../space_setup.rs`（`install_active_clipboard` 在 `IrohNodeBuilder::bind` 与 `spawn` 之间）+ `SpaceSetupAssembly` struct + `runtime_assembly.rs`（穿 receiver+dispatch 两个 port）。`node.rs` 单独 `.accept()` 编译不出可达路径。
- 帧解复用：`clipboard_wire.rs::read_frame` + `IrohClipboardReceiverHandler`（若 0xC2 复用 CLIPBOARD_ALPN）；推荐独立 sibling ALPN 则免改。
- `sync_on_restore` 8 层穿透 + round-trip 测：uc-core model+defaults → daemon-contract DTO → webserver projection ×2 → app `SettingsView` → app `SettingsPatch`+apply 分支 (`models.rs:589-597` 静默丢点) → TS view → TS patch-builder(`settings.ts:340` 静默丢点)。schema_version 维持 1（仅 serde-default，无 migration）。

**约定**
- ports：`crates/uc-core/src/ports/`，intent 拆分，`#[async_trait]`，域 error。use case 持 `Arc<dyn Port>`。
- 持久化：Diesel `executor.run(|conn| ...)`，条件 UPSERT 参照 `file_transfer_repo.rs:68-104`（**不要** 抄 `peer_address_repo.rs:59-80` 的无条件覆盖）。migrations `crates/uc-infra/migrations/`。

---

## 6. PR / 实现顺序（**审查锁定：写边界先行**）

核心不变量制造了硬前置：任何 broadcast/串联 PR 读的 register，只有 register 写边界落地后才会前进。**禁止** 先合广播 PR（否则单测全绿、整机永不收敛）。锁定顺序：

1. **PR1（写边界先行）**：register 单行表 + `ActiveClipboardRegisterPort`（条件原子 UPSERT）+ `ClipboardWriteCoordinator::write` 透传 entry_id + 5 个 call-site 接 register。**不变量的写侧先建好。**
2. PR2:0xC3 ALPN（独立 sibling）+ codec + install_active_clipboard 穿透（space_setup/SpaceSetupAssembly/runtime_assembly）。
3. PR3：入站 receiver → receive 闸门 → register → re-broadcast（含 spawned 成功分支）。
4. PR4：restore facade 出 entry_id + 闸门出站广播 + `sync_on_restore` 8 层穿透。
5. PR5：peer-online resync（接 presence_monitor）。
6. PR6：持久化 load-baseline + 启动 reconcile。
7. PR7：mobile fan-out 闸门 + 删 `MobileDuplicateRestorePort`（cutover）。
8. PR8：pull 子系统（0xC2 serve + client：D4 链路 + D3 blob 重签 + 10s timeout）。

> pull（PR8）放在 restore/收敛主线之后；若想早验「双方都有」的 happy path，PR1–PR5 即可端到端跑通（缺内容才需 PR8）。

---

## 7. 仍待定（均为实现期细节，非需求阻塞）

- 入站被闸门拒后是否需要任何「告知对端我没收」的反馈（v1 倾向不需要）。
- `activated_at_ms` 未来 ts 守卫阈值 X（建议 ~300s）。
- 0xC3 / pull serve 是否加埋点（telemetry）。
- 是否对 0xC3 state 路径在 X11/Wayland 上做 owner-lifetime read-back 校验（v1 先 best-effort）。
