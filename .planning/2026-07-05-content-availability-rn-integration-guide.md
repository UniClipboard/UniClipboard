> **⚠️ 本指南已被取代（2026-07-05）**：`computeSnapshotHash` / `MobileSyncClient.isContentAvailable`
> 已在 push/pull 同步 SDK 这轮工作中从 FFI 删除（push 路径根本不再需要存在性检查，见新设计
> `.planning/2026-07-05-mobile-push-pull-sdk-design.md` §6.3/§12）。如果还没接入本指南，
> 请直接跳到 `.planning/2026-07-05-mobile-push-pull-sdk-rn-integration-guide.md`，不要再接这两个 API。
> 本文件仅作历史存档保留。

# content-availability 可靠探测 · 移动端 (RN) 对接指南

面向 `uniclipboard-android`(RN/Expo,iOS + Android 共用 TS) 团队。
对应 Rust core 变更：本仓库新增 `uc-content-hash` crate + `uc-mobile` 两个新 FFI 导出
(`compute_snapshot_hash` / `MobileSyncClient.is_content_available`)。

---

## 1. 这次解决什么问题

排查发现：`SyncClipboardClient.putContent` 上传前调用 `getRecord(profileId)` 探测"服务端是否
已有这条",拿到 200 就跳过图片/文件字节上传。但 `GET /api/history/{profileId}` 对 Image/File
**故意放行 hash 漂移**(服务端可能对图片重新编码，导致 hash 变化，详见服务端
`history.rs` 的 `current_profile_type_allows_hash_drift`)——只要当前活跃剪贴板 type 匹配，
无论 hash 是否对得上都会返回 200。

后果：连续拍两张不同照片时，第二张会被误判"已存在",字节从未真正上传，但本地记录被标成
`Synced`——**静默数据丢失**,不只是显示重复。

根因：`getRecord` 这个存在性检查本身就不可靠，不是"漂移容忍"这个设计意图之外的滥用。

---

## 2. Rust core 新增了什么

| 类型 / 函数 | 说明 |
|---|---|
| `compute_snapshot_hash(bytes: Vec<u8>) -> String`(自由函数，和 `uc_mobile_init` 同级) | 对给定字节计算 `"blake3v1:<hex>"` 内容身份，和服务端 `uc-core` 用的是同一份算法 (`uc-content-hash` crate，新增的零依赖叶子 crate),不是重新发明的一套 |
| `MobileSyncClient.isContentAvailable(server, snapshotHash: String) -> Bool` | 查询新增的 `GET /api/mobile-sync/content-availability?snapshotHash=...` 端点。`true` = 服务端确实持有这份 **一模一样** 的内容，且当前可用 (不是残缺上传或已被删除的本地文件) |

这两个能力 **不属于** SyncClipboard 协议兼容面 (`/api/history/*`、`/SyncClipboard.json`)。它们是
本项目自有的扩展端点，专门给我们自己的移动客户端用，不受"不能破坏 SyncClipboard 兼容性"的
约束，未来可以自由演进。

---

## 3. FFI 面变更 (BREAKING — 必须重新生成 binding)

新增 1 个自由函数 + `MobileSyncClient` 新增 1 个方法，`modules/uc-core` 的 Kotlin/Swift binding
+ TS `index.ts` 包装 **必须重新生成** 才能看到：

- `computeSnapshotHash(bytes: Uint8Array): string`
- `client.isContentAvailable(server: ServerConfig, snapshotHash: string): Promise<boolean>`

---

## 4. RN 接线步骤

### 步骤 0 · 重生成 binding
rebuild mobile core → 重出 `modules/uc-core/{ios,android}` 的 Kotlin/Swift + TS `index.ts`。
确认上述新函数/新方法已出现在 TS 类型里。

### 步骤 1 · 用 `isContentAvailable` 替换 `getRecord` 的存在性判断
`SyncClipboardClient.putContent` 里原本这段 (伪代码):

```ts
existingRecord = await this.getRecord(profileId);   // 拿 200 就以为"已存在"
if (existingRecord && !existingRecord.isDeleted) {
  // skip data upload
}
```

改成：

```ts
const snapshotHash = computeSnapshotHash(payloadBytes);
const available = await client.isContentAvailable(server, snapshotHash);
if (available) {
  // skip data upload — 见下面 §5 的适用范围限制
}
```

**不要** 再调用 `getRecord(profileId)` 做存在性判断——那条检查在 Image/File 上本来就不可靠。

### 步骤 2 · `putContent` 的上传腿本身不变
无论走哪条判断，真正上传字节 (`POST /api/history` 那一段) 的逻辑不用改——这次只换探针，
不换上传机制本身。

---

## 5. ⚠️ 适用范围限制 (务必读完再接线)

`isContentAvailable` 回答的是 **"服务端是否持有这份一模一样的内容"**,不是
**"跳过上传这个动作本身安全吗"**。这是两个不同的问题：

- `find_entry_id_by_snapshot_hash` 匹配的是 **任意** 一条曾经出现过的内容记录，不一定是
  **当前活跃** 的那条。
- 服务端的"当前活跃剪贴板"寄存器 (会同步广播给其他设备) 只有走完整的 `PUT /SyncClipboard.json`
  才会推进。
- 如果你因为 `available: true` 就整个跳过上传 (包括不再发起任何请求),而这份内容并不是
  服务端当前活跃的那条，其他设备就永远不会知道"这份内容现在又变成当前剪贴板了"——这会引入
  一种新的、更隐蔽的问题 (不是数据丢失，而是跨设备不同步)。

**目前安全的用法**:仅用于避免"确定是刚做过的重复动作"这类场景 (比如同一次上传因为超时/取消
被重试，或者极短时间内对同一内容的重复触发)。**不要** 把它当作"游标：只要内容存在过就永远不用
再管"的通用去重开关。

**真正做到"内容已存在就能安全跳过完整上传、同时正确让它变成当前活跃内容"**,需要服务端再新增
一个"按内容引用注册，不用重传字节"的能力——现在还没有 (现有 `PUT /SyncClipboard.json` 的两段式
协议要求文件必须先真的走过 `PUT /file/{name}` 暂存)。如果之后需要恢复完整的带宽优化，这是
前置工作，应该另开一个 issue，不在这次范围内。

---

## 6. 验证方法

最小复现 (对应最初报告的 bug 场景):
1. 手机连续拍两张 **不同** 照片。
2. 第二张：`computeSnapshotHash` 得到的 hash 与第一张不同 → `isContentAvailable` 应返回
   `false` → 走真实上传 → 服务端应该收到第二张的真实字节 (不再是"误判已存在，数据丢失")。

补充检查：
- 同一张照片短时间内因客户端重试导致 `putContent` 被调用两次：第二次 `isContentAvailable`
  应返回 `true`(因为第一次已经真实上传成功，服务端确实持有这份内容),可以安全跳过重传。
- legacy 服务端 (没有 `/api/mobile-sync/content-availability` 路由) 会返回 404 —
  `isContentAvailable` 调用应映射成错误而不是静默返回 `true`,调用方应该 fallback 成"直接上传"
  (即：探测失败时，按"未探测到"处理，不要按"已存在"处理——这是安全默认值的方向)。

---

## 7. 一句话给到团队

> Rust core 新增了 `computeSnapshotHash` + `isContentAvailable` 两个能力，取代 `putContent`
> 里原本基于 `getRecord` 的不可靠存在性判断——那条判断在 Image/File 上会把"当前活跃剪贴板 type
> 匹配"误判成"这份具体内容已存在",造成连续两次不同上传时第二次静默丢数据。新探针按内容哈希
> 精确判断，失败模式安全 (判断错了最多多传一次，不会再丢数据)。但它只回答"内容是否存在",
> 不能替代完整的上传流程去决定"能不能跳过整个动作"——见 §5，别把它当成万能去重开关。
