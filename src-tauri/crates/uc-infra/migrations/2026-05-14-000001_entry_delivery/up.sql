-- 新增 entry 投递结果表 + 在 clipboard_entry 上加追踪标志。
--
-- delivery_tracked = 0 表示该 entry 在本机投递追踪机制启用前已存在,
-- 视图层应据此把它标记为"历史 entry,投递信息未知",而不是合成 Pending。
-- 全部已有数据默认为 0(未追踪);新机制下创建的 entry 在应用层显式置 1。

ALTER TABLE clipboard_entry
ADD COLUMN delivery_tracked INTEGER NOT NULL DEFAULT 0;

-- 一条 entry 对单个对端的最新投递结果。重新投递会以 INSERT OR REPLACE
-- 覆盖旧结果;entry 被删除时通过 FK CASCADE 清理。
-- target_device_id 不引用 trusted_peer:解除配对后历史记录保留,视图层
-- 自行决定是否过滤"已离开的对端"。
CREATE TABLE clipboard_entry_delivery (
    entry_id          TEXT    NOT NULL,
    target_device_id  TEXT    NOT NULL,
    status            TEXT    NOT NULL,
    reason_detail     TEXT,
    updated_at_ms     BIGINT  NOT NULL,
    PRIMARY KEY (entry_id, target_device_id),
    FOREIGN KEY (entry_id) REFERENCES clipboard_entry(entry_id) ON DELETE CASCADE
);

CREATE INDEX idx_entry_delivery_entry
ON clipboard_entry_delivery (entry_id);
