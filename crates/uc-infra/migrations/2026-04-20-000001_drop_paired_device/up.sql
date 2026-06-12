-- Phase 4b PR-5: Drop the legacy paired_device table.
--
-- 所有运行时消费者已在 Phase 3 / Phase 4b PR-4 切到 space_member
-- (MemberRepositoryPort) 与 trusted_peer (TrustedPeerRepositoryPort)，
-- 该表自 PR-4 起无任何读写方。彻底移除表 + schema.rs 条目后，
-- Rust 侧 PairedDevice / PairingState / PairedDeviceRepositoryPort 类型族一并下线。
DROP TABLE paired_device;
