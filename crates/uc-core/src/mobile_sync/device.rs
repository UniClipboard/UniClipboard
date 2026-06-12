//! 移动端设备实体与值对象。
//!
//! `MobileDeviceId` 是登记完成后服务端为该客户端分配的稳定标识，格式约定
//! 为 `did_<32hex>`（32 字节随机的 hex 编码）。它和项目里既有的
//! `crate::ids::DeviceId`（标识桌面端 / daemon 设备）属于不同业务域，因此
//! 选择不复用而是单独建模 —— 同名只会让"哪边的设备"语义模糊。

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 服务端分配给一台移动端设备的稳定标识。
///
/// 形如 `did_<32hex>`。Adapter 决定具体生成方式（典型：32 字节 OsRng + hex
/// 编码 + 前缀），这里只把它当作不透明字符串处理。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MobileDeviceId(String);

impl MobileDeviceId {
    /// 包装 adapter 生成好的字符串，不做格式校验 —— 校验是 minter 的职责。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for MobileDeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 该移动端是用什么客户端形态接入的。
///
/// 注意：LAN 监听跑的是平台无关的 SyncClipboard 协议（HTTP Basic Auth + GET/PUT
/// `/SyncClipboard.json` + `/file/{name}`），凡是兼容该协议的客户端均可接入，
/// 凭据本身与平台无关。本枚举仅记录"管理员注册时是按哪种客户端形态分发指引的"
/// —— v1 仅 iOS 快捷指令一种官方分发方式，因此只有 `IosShortcut` 一个 variant。
/// 用户实际可以拿同一组凭据在任意 Android / 鸿蒙等第三方客户端上使用。未来若
/// 新增官方分发渠道（如 Android app）再追加 variant，协议层无需改动。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MobileClientType {
    IosShortcut,
}

impl MobileClientType {
    /// 持久化与 wire 上使用的稳定字符串值。
    ///
    /// 不要依赖 `Debug` 输出，那是给开发者读的。
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            MobileClientType::IosShortcut => "ios_shortcut",
        }
    }

    /// 从持久化的 wire 字符串恢复。`None` 表示未知值——adapter 拿到陌生值
    /// 应当作"行损坏"处理（很可能是降级回旧版本读到了新版本写入的行）。
    pub fn from_wire_str(value: &str) -> Option<Self> {
        match value {
            "ios_shortcut" => Some(MobileClientType::IosShortcut),
            _ => None,
        }
    }
}

/// 已登记的移动端设备(v3 SyncClipboard 兼容版)。
///
/// 字段语义:
/// * `device_id` —— 服务端分配的稳定标识,撤销 / 列表 UI 中作为主键。
/// * `label` —— 用户在登记时填的可读标签("我的 iPhone 15")。
/// * `client_type` —— 客户端形态(v1 仅 `IosShortcut`)。
/// * `username` —— 该设备的 Basic Auth 用户名,在所有已登记设备中**唯一**。
///   形如 `mobile_<8hex>`,daemon 内部不解读语义,但客户端在 SyncClipboard
///   shortcut 里看到的字段不那么吓人。
/// * `password_hash` —— Argon2id PHC 字符串(`$argon2id$v=19$m=...,t=...,
///   p=...$<salt>$<hash>`)。原 password 只在登记成功的瞬间一次性回显给
///   用户(写进 SyncClipboard shortcut),之后仅以此哈希存在于服务端。
///   鉴权时调 `PasswordHasherPort::verify` 比对。
/// * `created_at_ms` —— 登记时刻,Unix 毫秒。
/// * `last_seen_at_ms` / `last_seen_ip` —— 客户端最后一次合法请求的时间
///   戳与来源地址,仅作运维 / UI 展示用,未参与鉴权决策。
/// * `reported_name` / `reported_os` —— SyncClipboard shortcut **不上报**
///   设备信息(无 handshake 概念),v3 这两个字段保留但永远 `None`,留给
///   未来的 ClipboardAuto 或 v2 客户端扩展。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileDevice {
    pub device_id: MobileDeviceId,
    pub label: String,
    pub client_type: MobileClientType,
    /// Basic Auth 用户名(在所有已登记设备中唯一)。
    pub username: String,
    /// Argon2id PHC 字符串。
    pub password_hash: String,
    pub created_at_ms: i64,
    pub last_seen_at_ms: Option<i64>,
    pub last_seen_ip: Option<String>,
    pub reported_name: Option<String>,
    pub reported_os: Option<String>,
}

/// 持久化层向上传递的领域错误。
///
/// 故意把底层失败（sqlite 错、序列化错等）合并为 `Storage(String)`，以避免
/// adapter 的具体技术细节穿透到应用层；调用方需要展示给用户的错误请进一步
/// 翻译为 use-case-level 的 *Error。
#[derive(Debug, Error)]
pub enum MobileDeviceError {
    /// 业务唯一性冲突:同 `device_id` 已存在。
    #[error("mobile device already exists: {0}")]
    AlreadyExists(MobileDeviceId),

    /// 业务唯一性冲突:同 `username` 已被另一台设备占用。
    ///
    /// 理论上 8 字符 hex(`mobile_<8hex>`)的碰撞概率可忽略,但 adapter 仍
    /// 应把唯一约束在 schema 层面约束住,碰撞时这条错误能让上层把请求重试。
    #[error("mobile device username already in use")]
    UsernameCollision,

    /// 持久化技术失败 —— 文案仅用于日志 / tracing。
    #[error("mobile device storage failure: {0}")]
    Storage(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_device_id_round_trip_through_str() {
        let id = MobileDeviceId::new("did_abc123");
        assert_eq!(id.as_str(), "did_abc123");
        assert_eq!(id.to_string(), "did_abc123");
        assert_eq!(id.clone().into_string(), "did_abc123");
    }

    #[test]
    fn mobile_client_type_wire_value_is_stable() {
        // 这条断言锁住 wire 字符串 —— 一旦改动会让所有已部署的 .shortcut /
        // sqlite 行失效，必须当作 schema 迁移来处理。
        assert_eq!(MobileClientType::IosShortcut.as_wire_str(), "ios_shortcut");
    }

    #[test]
    fn mobile_client_type_wire_round_trip() {
        for variant in [MobileClientType::IosShortcut] {
            assert_eq!(
                MobileClientType::from_wire_str(variant.as_wire_str()),
                Some(variant)
            );
        }
        assert_eq!(MobileClientType::from_wire_str("unknown"), None);
        assert_eq!(MobileClientType::from_wire_str(""), None);
    }
}
