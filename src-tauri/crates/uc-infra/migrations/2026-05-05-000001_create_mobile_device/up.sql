-- Create mobile_device: 已登记的移动端设备记录表(Phase 3 子步骤 5c, v3
-- SyncClipboard 兼容路线)。
--
-- 替代 `InMemoryMobileDeviceRepository`(进程内 HashMap),让 CLI ↔ daemon
-- 可以共享同一份设备列表,跨重启稳定。
--
-- v1/v2 迭代版本曾用 `token_hash BLOB UNIQUE` 做 Bearer token 索引;v3
-- 切到 SyncClipboard HTTP Basic Auth 后,鉴权改为 username + password_hash
-- 模型,token_hash 列整体下线。该 migration 没有上线过(仅在 feature
-- branch),直接用 v3 schema 覆盖,不需要 ALTER TABLE 兼容。
--
-- 列与 `uc_core::mobile_sync::MobileDevice` 1:1 映射:
--   device_id        := MobileDevice.device_id        (PK, did_<32hex>)
--   label            := MobileDevice.label            (用户填的可读名)
--   client_type      := MobileDevice.client_type      (wire-str:
--                                                       "ios_shortcut" 等)
--   username         := MobileDevice.username         (Basic Auth 用户名,
--                                                       UNIQUE 阻止重复)
--   password_hash    := MobileDevice.password_hash    (Argon2id PHC 字符串)
--   created_at_ms    := MobileDevice.created_at_ms    (Unix 毫秒)
--   last_seen_at_ms  := MobileDevice.last_seen_at_ms  (Option, 鉴权热路径
--                                                       回写)
--   last_seen_ip     := MobileDevice.last_seen_ip     (Option, 仅展示)
--   reported_name    := MobileDevice.reported_name    (Option, v3 永远 NULL)
--   reported_os      := MobileDevice.reported_os      (Option, v3 永远 NULL)
--
-- username 索引:鉴权热路径 `find_by_username` 走它,UNIQUE 约束本身已隐式
-- 建索引,不再额外 CREATE INDEX。

CREATE TABLE mobile_device (
    device_id       TEXT PRIMARY KEY NOT NULL,
    label           TEXT NOT NULL,
    client_type     TEXT NOT NULL,
    username        TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    created_at_ms   INTEGER NOT NULL,
    last_seen_at_ms INTEGER,
    last_seen_ip    TEXT,
    reported_name   TEXT,
    reported_os     TEXT
);
