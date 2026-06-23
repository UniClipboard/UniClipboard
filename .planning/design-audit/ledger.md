# Design Audit Ledger

> 由 `/design-audit` 维护。`status`: open=已知未修 / accepted=认了不改 / wontfix=明确不修 / fixed=已修（再现则 regressed）。
> 下一轮审计范围 = `last_audited_commit`..HEAD 的 churn。accepted/wontfix/fixed 不再报，open 仅 carryover。

last_audited_commit: f0a33197809641e607d89635d6387e9f5d33a20f
last_audited_at: 2026-06-23

| id | severity | lens | title | status | first_seen | last_seen | note |
|----|----------|------|-------|--------|-----------|-----------|------|
| uc-application:coordinator:write:L5 | P1 | L5 | echo 防回环时间窗散落字面量 + 注释漂移（违反 [[no-timing-coupled-coordination]]） | fixed | 2026-W25 | 2026-W25 | S0 33f4ace2c → timing 模块；S2 用户已合并成单窗 |
| uc-core:self_write_ledger:ClipboardChangeOriginPort:L3 | P1 | L3 | 6 方法 catch-all 端口（2 死方法 + 2 冗余 consume 变体），调用方手编排 4 个 guard | fixed | 2026-W25 | 2026-W25 | S1 886c908d0+f0a331978 → 2 方法 SelfWriteLedgerPort |
| uc-application:cleanup:check_device_quota:L4 | P2 | L4 | 配额检查死代码当「未来功能」供着，且路径布局是错的 | open | 2026-W25 | 2026-W25 | `file_sync/cleanup.rs` `#[allow(dead_code)]`；要么实现要么删 |
| uc-application:apply_incoming:IncomingMobileBuffer:L7 | P2 | L7 | 两阶段文件上传的孤儿靠环形缓冲 (上限 16) 回绕淘汰，无 TTL sweep | open | 2026-W25 | 2026-W25 | `usecases/mobile_sync/apply_incoming.rs:246` 注释自承认 TODO |
| uc-webserver:mobile_lan:port-resolution:L2 | P2 | L2 | mobile 端口双真相源：settings 写的值 vs FNV 派生实际 bind，BindFailed 不回滚 | open | 2026-W25 | 2026-W25 | 真实状态只在 current_status；排障易被误导 |
| uc-application:apply_inbound:dedup-windows:L1 | P3 | L1 | 入站幂等 3 窗口与 echo 回环叙事混在一起，增加理解成本 | open | 2026-W25 | 2026-W25 | S3 计划正名「入站幂等」+ 独立 timing 家 |
