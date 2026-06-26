# Design Audit Ledger

> 由 `/design-audit` 维护。`status`: open=已知未修 / accepted=认了不改 / wontfix=明确不修 / fixed=已修（再现则 regressed）。
> 下一轮审计范围 = `last_audited_commit`..HEAD 的 churn。accepted/wontfix/fixed 不再报，open 仅 carryover。

last_audited_commit: 6ca795ee0eb5e51a681bbd795e81f9617da89985
last_audited_at: 2026-06-23

| id | severity | lens | title | status | first_seen | last_seen | note |
|----|----------|------|-------|--------|-----------|-----------|------|
| uc-application:apply_inbound:index_for_search:L7 | P2 | L7 | snapshot 二次深拷贝 (OS write clone 后 index_for_search 再 clone)，图片 MB 级 | fixed | 2026-W26 | 2026-W26 | S0 a0fd40476：snapshot_for_write 改 Arc 共享给 index + OS write(try_unwrap)，对齐 watcher；29 ApplyInbound 测试绿 |
| uc-application:apply_inbound:search_live_index:L6 | P2 | L6 | search 索引副作用塞进 use case 内，本地路径却在编排层做（不一致） | open | 2026-W26 | 2026-W26 | **deferred**：干净形态需 persisted-mode indexer(§2.6)，与 P2-4 捆绑成 PR；现在做会污染 ApplyOutcome 枚举 |
| uc-application:projection:build_mime_loop:L4 | P2 | L4 | build_from_capture/build_from_persisted ~80 行 MIME 解析逐字重复 | fixed | 2026-W26 | 2026-W26 | S0 3deb0a1de：抽 SearchableContent::ingest + into_pipeline_input，两方法各 ~18 行；663+9 测试绿 |
| uc-infra:sqlite_index:source_filter:L6 | P2 | L6 | search adapter 跨表查 clipboard_event.source_device（其余四过滤字段已反规范化） | open | 2026-W26 | 2026-W26 | **deferred**：反规范化需 bump index_version 触发全量重建 (用户可感知)，单独 PR + 真机验证；与 P2-2/§2.6 捆绑 |
| uc-application:apply_inbound:god-object:L1 | — | L1 | （驳回）6-Option god object — 实为 4 Option，execute() 长但本质必需 | wontfix | 2026-W26 | 2026-W26 | 对抗核实判 is_accidental=false；记录以免下轮重审 |
| uc-application:clipboard_live_index:construct-dup:L4 | — | L4 | （驳回）两处构造 indexer 重复 — 仅 2 处、daemon 已内部共享，收益太低 | wontfix | 2026-W26 | 2026-W26 | 对抗核实判不进报告；记录以免下轮重审 |
| uc-application:coordinator:write:L5 | P1 | L5 | echo 防回环时间窗散落字面量 + 注释漂移（违反 [[no-timing-coupled-coordination]]） | fixed | 2026-W25 | 2026-W25 | S0 33f4ace2c → timing 模块；S2 用户已合并成单窗 |
| uc-core:self_write_ledger:ClipboardChangeOriginPort:L3 | P1 | L3 | 6 方法 catch-all 端口（2 死方法 + 2 冗余 consume 变体），调用方手编排 4 个 guard | fixed | 2026-W25 | 2026-W25 | S1 886c908d0+f0a331978 → 2 方法 SelfWriteLedgerPort |
| uc-application:cleanup:check_device_quota:L4 | P2 | L4 | 配额检查死代码当「未来功能」供着，且路径布局是错的 | open | 2026-W25 | 2026-W25 | `file_sync/cleanup.rs` `#[allow(dead_code)]`；要么实现要么删 |
| uc-application:apply_incoming:IncomingMobileBuffer:L7 | P2 | L7 | 两阶段文件上传的孤儿靠环形缓冲 (上限 16) 回绕淘汰，无 TTL sweep | open | 2026-W25 | 2026-W25 | `usecases/mobile_sync/apply_incoming.rs:246` 注释自承认 TODO |
| uc-webserver:mobile_lan:port-resolution:L2 | P2 | L2 | mobile 端口双真相源：settings 写的值 vs FNV 派生实际 bind，BindFailed 不回滚 | open | 2026-W25 | 2026-W25 | 真实状态只在 current_status；排障易被误导 |
| uc-application:apply_inbound:dedup-windows:L1 | P3 | L1 | 入站幂等 3 窗口与 echo 回环叙事混在一起，增加理解成本 | open | 2026-W25 | 2026-W25 | S3 计划正名「入站幂等」+ 独立 timing 家 |
