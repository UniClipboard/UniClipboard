# 双通道文件同步去重 — Layer ① 实现计划

配套设计：`.planning/2026-06-25-dual-channel-file-sync-dedup-design.md`（v6，已过 Codex 5 轮）。
本文件是 **实现期的 single source of truth**：commit 序列、关键代码决策、验证点、风险。
分支：`filesyncbug`。

## 范围（用户决策 2026-06-25）

**只做 Layer ①，完整落地后真机验证去重生效，再决定是否续做 ②③。**
理由（设计 §5）：① 修复主路径（顺序到达场景），②③ 是并发 relay 兜底。

Layer ① 完整范围 = canonical hash 算对 + capture 物化 inline uri-list + EntryFileSet manifest +
全链路 compute-once + inbound 存 wire H + 存量策略。

## Recon 关键事实（已对照 rebase 后真实代码确认）

- `SystemClipboardSnapshot.file_content_digests: Vec<[u8;32]>`（`uc-core/src/clipboard/system.rs:36`）——
  无序集合，唯一消费者是 `snapshot_hash()`（`system.rs:520`）。5 个有意义的填充点：
  capture `usecase.rs:189`（**只 LocalFile**）、outbound `clipboard_outbound/mod.rs:278`、
  resend `resend_entry.rs:416`、serve_pull `serve_pull.rs:163`、inbound materializer `materializer.rs:328`。
- 分叉根因：Windows 文件复制是 **Inline uri-list** rep（非 LocalFile）→ capture 填不到 digest →
  capture 存 uri-list hash(B)；dispatch 经 `publish_file_blob_refs` 读真实文件算 content hash(A) → A≠B。
- `BlobWriterPort::write_path_if_absent(&Path) -> BlobId`（`uc-core/blob/ports/writer.rs:51`）。
  实现 `blob_writer.rs:95` 内部 `stream_hash_file` **已算出 `(ContentHash, file_size)`**（:105/:162）但丢弃，只返回 BlobId。
  → capture 拿 content digest 完全可行：暴露已算出的 ContentHash 即可，无需二次读文件。
- dispatch 在 `payload_codec.rs:100` **重算** hash（`snapshot.snapshot_hash()`）。
- inbound 落库 hash 由 capture **重算**（物化后 snapshot），**非 wire H** →（F-4 要改）。
- `find_entry_id_by_snapshot_hash` 返回 `Option`、无 `deleted_at_ms` 过滤（Layer ② 才动）。
- `conn.transaction()` 可用，范例 `clipboard_event_repo.rs:83`。
- 文件列表只存在 uri-list rep 的 inline 文本里，**无 EntryFileSet 表** → 需新建表 + migration。
- 单 Arc 注入范例：`ClipboardWriteCoordinator`（`assembly.rs:1426`）。
- `restore_selection.rs` 已有"用持久 `clipboard_event.snapshot_hash`、禁止 reconstruct 重算"先例（`GetEntrySnapshotHashPort`）。

## 实现策略：先松散后有序（过渡，有 removal plan）

- Phase 1.A/1.B 先用 **现有松散 `file_content_digests` 集合** 模型 → 快速让 capture H == dispatch H == wire H，
  达到第一个真机验证点（双 entry 消失）。
- Phase 1.C 再把模型升级为 **逐行有序贡献 + 持久 manifest**（R2-F1 结构保留 + 不可变身份 + compute-once）。
  C5 升级取代 C3 的松散填充——这是明确的 removal plan，不留并行旧逻辑。

commit 边界遵循 `architecture-rules.md`：`uc-core`/`uc-infra` 分 commit；全新 trait 的 arch commit
天然单独可编译；改现有 trait 方法仅在带安全 default 时才与 adapter 分 commit。

---

## Commit 序列

### Phase 1.A — 根因修复（capture H == dispatch H，顺序场景去重生效）✅ 已实现 + 单测验证（未提交）

决策落定（与原计划的差异）：
- C1 采用「新增窄 port `BlobContentIngestPort` + `IngestedBlob` 值对象」（全新 trait → arch commit 单独可编译，规避改现有签名的 boundary/revert 死结），**未** 改 `write_path_if_absent` 签名。
- **不在 capture 施加 size-gate**：capture 现有 LocalFile 分支本就无条件物化，size-gate 是 dispatch 带宽考量。capture 物化+hash 所有 uri-list 文件，与现有行为一致。残留：多文件 + 部分超 cap 时 capture 集={全部}、dispatch 集={eligible} 仍分叉——比现状严格更好，**由 1.C manifest 的 excluded-marker 彻底解决**。故无 C3a 独立 refactor（直接复用现有 `extract_file_paths_from_snapshot`，serve_pull/resend 已跨模块复用它，F-6 满足）。

- [x] **C1 `arch`**：`crates/uc-core/src/blob/ports/content_ingest.rs` 新增 `BlobContentIngestPort` + `IngestedBlob{blob_id,content_hash,size_bytes}`；`ports/mod.rs` 导出。`cargo check -p uc-core` 绿。
- [x] **C2 `impl`**：`uc-infra/blob/blob_writer.rs` 抽 `ingest_path_inner`（含已算出的 content_hash/size），`write_path_if_absent` 委托它，新增 `impl BlobContentIngestPort`。零重复。
- [x] **C3 `feat`**：capture 字段 `blob_writer: BlobWriterPort` → `blob_ingest: BlobContentIngestPort`；LocalFile 分支改 `ingest_path`；新增 `derive_file_content_digests`（LocalFile rep content_hash + Inline uri-list 逐个 `ingest_path`，失败跳过 warn）。wiring：`StoragePorts.blob_content_ingest` + assembly 单实例双 trait-object + 3 capture 站点。
- [x] **V1 单测**（`clipboard_capture::usecase::tests`）：`inline_uri_list_identity_is_device_independent`（两设备不同路径同文件 → 同 snapshot_hash，且 ≠ 路径文本 hash）；`inline_uri_list_ingest_failure_is_skipped`。20 passed。
- [ ] **V1 真机**：Windows 复制文件到 macOS，dashboard 单条（留待 1.B/1.C 后一起验）。
- 备注：`uniclipboard`(src-tauri bin) build script 因缺 sidecar daemon 二进制失败，是 pre-existing 打包前置，与本改动无关；其余全 workspace `cargo check`/`--tests` 绿。

### Phase 1.B — inbound 存 wire H（F-4）

- [ ] **C4 `feat`**：`apply_inbound` 持久化用 wire envelope 的 `snapshot_hash`，不用本机 materialized 重算。
      InboundCapture 契约传入权威 hash（capture 落 event 时用它，而非 `snapshot.snapshot_hash()`）。在持久化点断言。
- **验证点 V2**：单测——注入与本机重算不同的 H，inbound 仍存 wire H。

### Phase 1.C — 结构保留 + 持久 manifest + compute-once（完整形态）

- [ ] **C5 `arch`+`impl`**：uc-core `content_identity` 逐行有序贡献（R2-F1/R4-F1）。
      升级 `file_content_digests` → 有序行级结构（`FileListIdentity{ lines: Vec<FileLineIdentity{file|literal|excluded}> }`）；
      改 5 填充点产出有序结构。碰撞测试。
- [ ] **C6 manifest**：`EntryFileSet` 行级 manifest 一等公民。
      - `arch`：uc-core 领域模型 + reader/builder port。
      - migration：新表 `entry_file_set`（entry_id, line_index, original_bytes, kind, file_digest, blob_id, size, exclude_reason；PK(entry_id,line_index)；FK→clipboard_entry ON DELETE CASCADE）。
      - `impl`：uc-infra repo。
      - `feat`：capture 持久化 manifest；`content_identity`/dispatch/serve/restore-broadcast 从同一份 manifest 读。
- [ ] **C7 `feat`**：dispatch 按 entry_id 读存储 hash 作唯一 wire identity（envelope/header/delivery/register/广播一致），重算仅断言（F-5）；serve_pull 读 manifest **不重 plan**；Resend 只发 manifest `file` 行、不发 `excluded` 行（R2-F2）。
- [ ] **C8 `feat`**：存量无 manifest 旧文件 entry「不可 serve / 不可 resend，直到重新捕获」（R5-F4）；本地 restore 仍走现有 reconstruct。
- **验证点 V3**：碰撞测试（混合 http/comment/顺序 uri-list 不撞）；时间漂移（capture 后改 max_file_size/删文件，dispatch/serve 按持久集）；存量 entry 被拒 serve、本地 restore 仍可用。

---

## 风险 / 坑

- **F-6 共享 helper**：capture 与 dispatch 的文件排除规则（size-gate / metadata 失败排除）必须同一份，否则 digest 集再分叉。C3a 必须先抽 helper。
- **commit 可编译**：每个 commit 必须 `cargo check --workspace` 过（revert-safety）。
- **mobile 不在范围**：mobile 入站走独立 LAN HTTP 协议，本次只动桌面 iroh 链路（设计 §4.0 范围说明）。
- **lint-staged 副作用**：改 Cargo.toml 会重写 + stage crates/AGENTS.md；改 *.md 会被 prettier/CJK 重写。拆 atomic commit 前预期。
- **uc-core 纯度**：content_identity / FileListIdentity 是领域类型，doc 不得引用协议/上层模块名。
- **混合版本**（设计 §6）：彻底修复需两端都升级；alpha 接受混合期偶发重复，发版说明注明。

## 进度

- 2026-06-25：recon 完成（6 路），实现计划成文。下一步 C1。
