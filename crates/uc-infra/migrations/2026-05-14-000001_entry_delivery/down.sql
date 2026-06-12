-- 回滚 2026-05-14-000001_entry_delivery。
--
-- DROP COLUMN 需要 SQLite 3.35+(本项目最低运行 3.40+,覆盖)。delivery_tracked
-- 不参与 PK / UNIQUE / FK / INDEX,可直接 DROP。

DROP INDEX IF EXISTS idx_entry_delivery_entry;

DROP TABLE IF EXISTS clipboard_entry_delivery;

ALTER TABLE clipboard_entry
DROP COLUMN delivery_tracked;
