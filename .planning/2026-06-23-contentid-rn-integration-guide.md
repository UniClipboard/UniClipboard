# contentId 去重 · 移动端 (RN) 对接指南

面向 `uniclipboard-android`(RN/Expo,iOS + Android 共用 TS) 团队。
对应 Rust core 变更：本仓库 commit `feat(mobile-sync): dedup re-encoded content by stable contentId`。
配套设计：`.planning/2026-06-23-contentid-mobile-dedup-design.md`(§11 为落地状态)。

---

## 1. 这次解决什么问题

手机推一张 **JPEG** → 桌面/服务端 **异步重编码成 PNG** → 下次 `GET /SyncClipboard.json` 时
`hash` 从 `A` 变成 `B`。当前同步 reducer 全程按 `hash` 去重，会把 `B` 当成新内容，**多建一张
重复卡片**。

服务端已为每条内容分配一个 **跨重编码稳定的不透明身份** `contentId`(`blake3v1:<hex>`,入库即冻结、
不随重编码改变，见服务端 commit `c6ff804d1`)。本次让 reducer 在「`hash` 变了但 `contentId` 没变」
时判定为「同一条内容、已同步」,消除这张多余卡。

> ⚠️ **范围限定**:本次只修 **同步主卡片**(reducer 经过的 `GET /SyncClipboard.json` 通道)。
> **历史列表里的重编码重复 (同一张图两条历史记录) 不在本次范围**——历史在 RN 的 TS
> `HistoryStorage`(AsyncStorage),不经 Rust，需另走 originHash v2 / TS 去重。见 §6。

---

## 2. FFI 面变更 (BREAKING — 必须重新生成 binding)

Rust core 的 UniFFI 面有以下破坏性变更，`modules/uc-core` 的 Kotlin/Swift binding + TS `index.ts`
包装 **必须重新生成**:

| 类型 / 函数 | 变更 | 说明 |
|---|---|---|
| `ClipboardMeta`(record) | **新增字段** `content_id: Option<String>` | 服务端身份;GET 响应有、上传/legacy 为 `null`。verbatim 透传，**不要规范化/大写化** |
| `SyncRuntimeState`(record) | **新增字段** `last_synced_content_id`、`staged_content_id`(均 `Option<String>`) | 与既有 `last_synced_hash`/`staged_server_hash` 并列，**原子同写同清** |
| `PreambleSnapshot`(record) | **新增字段** `persisted_synced_content_id: Option<String>` | 跨进程 resync 用，见 §3 步骤 3 |
| `commitConverged(state, serverHash, **serverContentId**)` | **新增入参** `serverContentId: Option<String>` | 学到 contentId 的 **主路径**;传 server entry 的 contentId |
| `commitApply(state, hash, **contentId**, nowMs, cfg)` | **新增入参** `contentId: Option<String>`(位置在 `hash` 之后) | 传 server entry 的 contentId |

**签名不变、但因 record 字段变更而被覆盖的**:`commitStage` / `commitApplyFailed`(已收完整
`ClipboardMeta`,字段自动带出)、`commitPush` / `commitConsentPush`(push 不知道 contentId，内部传
`None`,无需新入参)、`planAfterServerGet`(`ServerGetSnapshot.server_entry: ClipboardMeta` 自动携带
contentId)。

---

## 3. RN 接线步骤 (`src/services/SyncEngine.ts`)

> 行号参照调研时的 `SyncEngine.ts`,以当前实际代码为准。

### 步骤 0 · 重生成 binding
rebuild mobile core → 重出 `modules/uc-core/{ios,android}` 的 Kotlin/Swift + TS `index.ts`。
确认上述新字段/新参数已出现在 TS 类型里。

### 步骤 1 · `ServerGetSnapshot` 带出 contentId
构造 `planAfterServerGet` 的 `ServerGetSnapshot` 时，`server_entry`(`ClipboardMeta`) 要带上从
`GET /SyncClipboard.json` 响应里解出来的 `contentId` 字段。**确认 GET 解析层把 `contentId` 读进了
`server_entry`**(Rust 的 `Clipboard` 已支持解码 `contentId` 键;RN 这边只要把它映射进 `ClipboardMeta`
即可)。

### 步骤 2 · commit 调用点传 contentId
- `commitConverged(...)`(truth-gate 收敛)→ 传 `serverEntry.contentId`。**这是学到 contentId 的
  主路径**(设计 §3 第 2 步):push 后第一次 GET、设备剪贴板仍是刚 push 的内容时走这里。
- `commitApply(...)`(自动应用 server entry)→ 传 `serverEntry.contentId`。
- `commitPush` / `commitConsentPush` / `commitStage` / `commitApplyFailed` / `markStagedApplied`
  **调用点不变**(内部已处理:push 传 None、stage 存 entry 的 contentId、markStagedApplied 提升)。

### 步骤 3 · `planPreamble` 填跨进程 contentId
`planPreamble` 的 `PreambleSnapshot.persisted_synced_content_id` 填 **从持久化存储读出的**
`lastSyncedContentId`(见步骤 4)。**Share Extension / 后台 push 路径不知道 contentId，要写
ABSENT(`null`),不要沿用旧值。**

### 步骤 4 · 持久化 `lastSyncedContentId`(AsyncStorage)
新增一个持久化键 `lastSyncedContentId`,与既有 `lastSyncedContentHash` **并列读写**,且二者必须
**原子同写同清**(它们要么一起更新、要么一起清空——reducer 内部已保证 `SyncRuntimeState` 里两个键
一致，你只要把它们一起落盘/读取即可)。

---

## 4. 行为契约 (写代码时务必遵守)

1. **`contentId` 是不透明整体字符串**(`blake3v1:<hex>`)——整体比较，**不解析、不规范化、不大小写
   折叠**。和 `hash` 的大写化体系**分开**,别混用 hash 的比较逻辑。
2. **双 `Some` 只看 `contentId`、忽略 `hash`**:server entry 与已同步水位线两边都有 contentId 时，
   只比 contentId(这正是跨重编码稳定的关键);任一侧缺 contentId 时回退到现有 hash 比较 (向后兼容
   legacy 服务端 / contentId 尚未学到)。
3. **push 后 `contentId` 被清空**:push 换了内容但还不知道其服务端身份，所以 push 类 commit 会把
   `last_synced_content_id` 置 `None`,等下次 GET 重新学到。**别在持久化层补一个旧 contentId。**
4. **服务端契约 (硬依赖)**:`contentId` 对同一原始字节稳定、对不同内容全局不复用、一旦分配不回收。
   由服务端保证;若服务端身份算法变更，本逻辑需回看。

---

## 5. 验证方法 (e2e)

最小复现：
1. 手机推一张 **JPEG** 图片 (`auto-apply` 开 / 关各测一遍)。
2. 让桌面/服务端把它 **重编码成 PNG**(`hash` 改变、`contentId` 不变)。
3. 下次 GET 后：**同步主卡片应只有 1 张，不应冒出第二张**。

补充检查：
- `auto-apply OFF` 时手动点应用 staged 条目后，再来一次重编码 GET,**仍不应重复**(对应 Rust 的
  `mark_staged_applied` 提升 contentId 修复)。
- legacy 服务端 (响应无 `contentId`) 行为应与改动前完全一致 (回退 hash)。

---

## 6. 不在本次范围 (需 RN 侧另立任务)

- **历史列表重编码重复 (残留 3)**:同一张重编码图，主卡片去重为 1 张，但 RN 的历史列表
  (`HistoryStorage`,按 content hash 全表查重) 仍可能是 2 条。**这不经 Rust，改 Rust 无效**。
  解决路径：优先 **originHash v2**(服务端内存 map + 客户端 TS 对账),或在 `HistoryStorage.addItem`
  的 TS 去重里引入 contentId/originHash 判重。
- **历史记录整体收敛到 Rust core**:经评估「有条件值得、非当前第一优先级」,建议在 originHash v2
  方案定调后再议 (届时走 Rust 纯逻辑、list/bytes 进出、IO 留 host 的策略;rust core 不需为
  iOS/Android 存储路径做平台适配)。

---

## 7. 一句话给到团队

> Rust core 已就绪：同步 reducer 现在用稳定的 `contentId` 去重，能消掉重编码图片在 **主卡片** 上的
> 重复。你们要做的就是 §3 的 4 步：重生成 binding → GET 解出 contentId → commit 传 contentId →
> 持久化 `lastSyncedContentId`。历史列表的重复是另一条线 (§6),不在这次。
