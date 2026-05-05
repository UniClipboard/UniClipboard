-- Create mobile_device: 已登记的移动端设备记录表(Phase 3 子步骤 1)。
--
-- 替代 `InMemoryMobileDeviceRepository`(进程内 HashMap),让 CLI ↔ daemon
-- 可以共享同一份设备列表,跨重启稳定。
--
-- 列与 `uc_core::mobile_sync::MobileDevice` 1:1 映射:
--   device_id        := MobileDevice.device_id        (PK, did_<32hex>)
--   label            := MobileDevice.label            (用户填的可读名)
--   client_type      := MobileDevice.client_type      (wire-str:
--                                                       "ios_shortcut" 等)
--   token_hash       := MobileDevice.token_hash       (32 字节 SHA-256,
--                                                       BLOB 节省空间;
--                                                       UNIQUE 约束阻止
--                                                       两台设备共享 token)
--   created_at_ms    := MobileDevice.created_at_ms    (Unix 毫秒)
--   last_seen_at_ms  := MobileDevice.last_seen_at_ms  (Option, 鉴权热路径
--                                                       回写)
--   last_seen_ip     := MobileDevice.last_seen_ip     (Option, 仅展示)
--   reported_name    := MobileDevice.reported_name    (Option, 客户端自报)
--   reported_os      := MobileDevice.reported_os      (Option, 客户端自报)
--
-- token_hash 索引:鉴权热路径 `find_by_token_hash` 走它,UNIQUE 约束本身
-- 已隐式建索引,不再额外 CREATE INDEX。

CREATE TABLE mobile_device (
    device_id       TEXT PRIMARY KEY NOT NULL,
    label           TEXT NOT NULL,
    client_type     TEXT NOT NULL,
    token_hash      BLOB NOT NULL UNIQUE,
    created_at_ms   INTEGER NOT NULL,
    last_seen_at_ms INTEGER,
    last_seen_ip    TEXT,
    reported_name   TEXT,
    reported_os     TEXT
);
