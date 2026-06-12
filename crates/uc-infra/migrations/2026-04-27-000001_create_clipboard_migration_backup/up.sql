-- Create clipboard_migration_backup: switch-space 重加密迁移期间的备份集合。
--
-- 仅在一次 switch-space 流程的"phase 1 prepared"到"phase 4 commit"窗口内
-- 持有数据；正常空闲态下应该是空表。
--
-- 列含义：
--   event_id              := 主表 clipboard_event.event_id
--   representation_id     := 主表 clipboard_snapshot_representation.id
--   migration_ciphertext  := 用一次性 migration_key (XChaCha20-Poly1305 V1)
--                            重加密后的 inline_data 字节，wire format 同
--                            BlobCipherPort::Ciphertext（serde_json::to_vec
--                            (&EncryptedBlob)）
--
-- 复合主键 (event_id, representation_id) 与主表相同，phase 3 写回主表时
-- 一对一覆盖。无外键约束：phase 3 完成后这张表会被清空，没必要让 SQLite
-- 替我们做引用完整性。

CREATE TABLE clipboard_migration_backup (
    event_id              TEXT NOT NULL,
    representation_id     TEXT NOT NULL,
    migration_ciphertext  BLOB NOT NULL,
    PRIMARY KEY (event_id, representation_id)
);
