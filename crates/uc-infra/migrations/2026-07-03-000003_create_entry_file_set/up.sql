-- 新增文件类 entry 的逐行清单表。
--
-- 一条 entry 的完整文件清单在 capture 时构建一次并整体落库;每次重建都是
-- "先按 entry_id 整体删除旧行,再插入新行"(应用层保证,不在此表约束)。
-- entry 被删除时通过 FK CASCADE 清理。
CREATE TABLE entry_file_set (
    entry_id       TEXT    NOT NULL,
    line_index     BIGINT  NOT NULL,
    original_text  TEXT    NOT NULL,
    kind           TEXT    NOT NULL,
    content_hash   TEXT,
    blob_id        TEXT,
    size_bytes     BIGINT,
    exclude_reason TEXT,
    -- The composite PK's implicit index has `entry_id` as its leading column,
    -- so the "filter by entry_id, order by line_index" read path (and the
    -- DELETE-by-entry_id in the replace flow) is already covered. No separate
    -- single-column index on entry_id is needed — it would only add write
    -- amplification on every insert/delete with no read benefit.
    PRIMARY KEY (entry_id, line_index),
    FOREIGN KEY (entry_id) REFERENCES clipboard_entry(entry_id) ON DELETE CASCADE
);
