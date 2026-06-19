//! `DeviceId` —— 设备身份的值对象。
//!
//! `DeviceId` 是一个 `Copy` 值对象,代表系统中某台设备的稳定标识。
//! 设计为 `Copy` 是为了让该标识能像普通值一样被传递、复制、比较,
//! 而不必在每个使用点散布 `.clone()`。
//!
//! 内部以 `ArrayString<DEVICE_ID_MAX_BYTES>` 存储,从而在不进行堆分配的
//! 前提下获得 `Copy` 能力;超过该上限的输入在 `new()` 与反序列化路径上
//! 被显式拒绝(panic / serde error),不会被静默截断 —— 契约是
//! "device_id 是有限规模的稳定标识,不是任意长度字符串"。
//!
//! `Serialize` / `Deserialize` 以裸字符串往返,等价于一个等长字符串值。

use arrayvec::ArrayString;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 单个 `DeviceId` 允许的最大字节数(UTF-8 字节,非字符数)。
///
/// 超出该上限的输入在 `DeviceId::new` 与反序列化路径上被显式拒绝。
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

    /// Fallible constructor for untrusted input. Returns `None` when the
    /// candidate exceeds `DEVICE_ID_MAX_BYTES`, instead of panicking like
    /// [`new`](Self::new). Use this at trust boundaries (e.g. decoding a
    /// device id off the wire) where an over-long value is a rejectable
    /// input rather than a local contract violation.
    pub fn try_new(id: impl AsRef<str>) -> Option<Self> {
        ArrayString::from(id.as_ref()).ok().map(Self)
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
    fn try_new_accepts_in_bounds_and_rejects_overlong() {
        let ok = DeviceId::try_new("peer-x").expect("in-bounds id");
        assert_eq!(ok.as_str(), "peer-x");

        let too_long = "x".repeat(DEVICE_ID_MAX_BYTES + 1);
        assert!(
            DeviceId::try_new(&too_long).is_none(),
            "over-long id must be rejected, not truncated or panicked"
        );
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
