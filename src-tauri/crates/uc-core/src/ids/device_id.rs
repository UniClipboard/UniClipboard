//! `DeviceId` —— `Copy`-able 设备身份。
//!
//! 为什么是 `Copy`:
//! 设备 id 出现在大量值传递路径(`ClipboardChangeOrigin::RemotePush`
//! 携带的 `from_device`、各 use case 的入参、tracing field、跨 span 的
//! 闭包捕获 …)。如果它只是 `String` wrapper,每条流水线都要散布
//! `.clone()` 才能在多次借用之间复用,可读性与维护成本都被 hit。把
//! `DeviceId` 设计为 `Copy`,核心业务代码就能像传 `Uuid` 一样自然传值。
//!
//! 为什么用 `ArrayString<64>` 而不是 `String`:
//! `String` 拥有堆分配,无法实现 `Copy`。项目里见到的最长 device_id
//! 形态是 `mobile_sync:<MobileDeviceId>` ≈ 48 字节,留出 64 字节余量
//! 把存储改成栈上定长数组,从而获得 `Copy` 能力。超长输入会在 `new()`
//! 与反序列化路径上显式拒绝(panic / serde error),不被静默截断 ——
//! 默契是"device_id 是有限规模的稳定标识,不是任意长度字符串"。
//!
//! Wire / DB 兼容:`Serialize`/`Deserialize` 仍以裸字符串往返,不变更
//! 任何外部存档或协议格式。

use arrayvec::ArrayString;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 单个 device_id 允许的最大字节数(UTF-8 字节,非字符数)。
///
/// 选 64 是因为见到的最长 device_id 形态约 48 字节
/// (`mobile_sync:<MobileDeviceId>`),64 字节给可预见的 prefix 留余量,
/// 同时维持栈占用合理。需要超过该上限时应优先重构 device_id 命名,
/// 而非提高该常量。
pub const DEVICE_ID_MAX_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId(ArrayString<DEVICE_ID_MAX_BYTES>);

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl DeviceId {
    /// 构造 `DeviceId`。超过 `DEVICE_ID_MAX_BYTES` 字节会 panic ——
    /// 这是契约违反,代表上游生成 device_id 的代码出 bug 或命名规则
    /// 突破假设,需要修正,而非静默截断。
    pub fn new(id: impl AsRef<str>) -> Self {
        let s = id.as_ref();
        let arr = ArrayString::from(s).unwrap_or_else(|_| {
            panic!(
                "device id exceeds {DEVICE_ID_MAX_BYTES} bytes (got {} bytes): {s:?}",
                s.len()
            )
        });
        Self(arr)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Serialize for DeviceId {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.as_str().serialize(ser)
    }
}

impl<'de> Deserialize<'de> for DeviceId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s: std::borrow::Cow<'de, str> = Deserialize::deserialize(de)?;
        ArrayString::from(s.as_ref()).map(DeviceId).map_err(|_| {
            serde::de::Error::custom(format!(
                "device id exceeds {DEVICE_ID_MAX_BYTES} bytes (got {} bytes)",
                s.len()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_short_id() {
        let id = DeviceId::new("a3a88f53-e2b8-4503-87bb-c91844e16a6f");
        assert_eq!(id.as_str(), "a3a88f53-e2b8-4503-87bb-c91844e16a6f");
        // 反复 copy 不需要 .clone()
        let copies: [DeviceId; 3] = [id, id, id];
        assert!(copies.iter().all(|c| c.as_str() == id.as_str()));
    }

    #[test]
    fn round_trip_mobile_sync_prefix() {
        // 项目里见到的最长 device_id 形态。
        let id = DeviceId::new("mobile_sync:did_0123456789abcdef0123456789abcdef");
        assert!(id.as_str().len() <= DEVICE_ID_MAX_BYTES);
    }

    #[test]
    #[should_panic(expected = "device id exceeds")]
    fn rejects_overlong_id() {
        let too_long = "x".repeat(DEVICE_ID_MAX_BYTES + 1);
        let _ = DeviceId::new(too_long);
    }

    #[test]
    fn serde_round_trip_matches_plain_string() {
        let id = DeviceId::new("peer-x");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"peer-x\"");
        let back: DeviceId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}
