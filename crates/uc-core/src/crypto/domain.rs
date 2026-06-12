//! 加密领域类型。
//!
//! 这里只放**领域概念**——不涉及任何具体密码学算法、密钥层级、持久化格式。
//! 所有基础设施细节（具体 KDF/AEAD 实现、密钥物料、元数据持久化）
//! 位于 `uc-infra/src/security/`。
//!
//! 本模块暂不定义任何 port trait；port 形状由后续 usecase 实现反向驱动。

use crate::crypto::secret::SecretString;
use crate::ids::SpaceId;
use std::fmt;
use zeroize::Zeroize;

// ---------------------------------------------------------------------------
// Passphrase
// ---------------------------------------------------------------------------

/// 用户输入的口令。
///
/// 承载"用户秘密"语义，drop 时内存自动清零。不可 Clone / Serialize / Display。
pub struct Passphrase(SecretString);

impl Passphrase {
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::new(value.into()))
    }

    /// 借用内部字节以供 adapter 在可控生命周期内使用。
    pub fn expose(&self) -> &str {
        self.0.expose()
    }
}

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Passphrase([REDACTED])")
    }
}

impl PartialEq for Passphrase {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl From<String> for Passphrase {
    fn from(v: String) -> Self {
        Self::new(v)
    }
}

impl From<&str> for Passphrase {
    fn from(v: &str) -> Self {
        Self::new(v)
    }
}

// ---------------------------------------------------------------------------
// ActiveSpace
// ---------------------------------------------------------------------------

/// 已解锁的空间句柄。
///
/// 对领域层不透明：内部只携带 `SpaceId`，真正的密钥物料由 adapter 侧的
/// 会话存储以 `SpaceId` 为键维护。
///
/// **语义契约**：拿到 `ActiveSpace` 意味着"该空间在当前进程已解锁"——
/// 领域代码不应直接构造它，构造动作应发生在实现了 unlock 能力的 adapter 中。
pub struct ActiveSpace {
    space_id: SpaceId,
}

impl ActiveSpace {
    /// 由 adapter 在完成解锁流程后构造。
    ///
    /// 领域代码请勿直接调用此构造器——类型名本身是"已解锁"的担保。
    pub fn new(space_id: SpaceId) -> Self {
        Self { space_id }
    }

    pub fn space_id(&self) -> &SpaceId {
        &self.space_id
    }
}

impl fmt::Debug for ActiveSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveSpace")
            .field("space_id", &self.space_id)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Plaintext
// ---------------------------------------------------------------------------

/// 明文字节容器。
///
/// Drop 时自动清零；不可 Clone / Serialize。
pub struct Plaintext(Vec<u8>);

impl Plaintext {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 显式消耗并取出底层 `Vec<u8>`——会绕过自动清零，仅在必须转交所有权时使用。
    pub fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

impl fmt::Debug for Plaintext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Plaintext([REDACTED; {} bytes])", self.0.len())
    }
}

impl Drop for Plaintext {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl From<Vec<u8>> for Plaintext {
    fn from(v: Vec<u8>) -> Self {
        Self::new(v)
    }
}

// ---------------------------------------------------------------------------
// Ciphertext
// ---------------------------------------------------------------------------

/// 密文字节容器。
///
/// 不带算法标签——具体算法由 adapter 决定并在持久化时自描述。
/// 密文本身可公开，不需要 zeroize。
#[derive(Clone, PartialEq, Eq)]
pub struct Ciphertext(Vec<u8>);

impl Ciphertext {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl fmt::Debug for Ciphertext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ciphertext({} bytes)", self.0.len())
    }
}

impl From<Vec<u8>> for Ciphertext {
    fn from(v: Vec<u8>) -> Self {
        Self::new(v)
    }
}

// ---------------------------------------------------------------------------
// Aad
// ---------------------------------------------------------------------------

/// Associated Authenticated Data 字节容器。
///
/// AAD 是公开的业务元数据，用于把密文绑定到特定上下文（例如条目 id、空间 id）。
/// 不带算法细节，不需要 zeroize。
#[derive(Clone, PartialEq, Eq)]
pub struct Aad(Vec<u8>);

impl Aad {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Aad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Aad({} bytes)", self.0.len())
    }
}

impl From<Vec<u8>> for Aad {
    fn from(v: Vec<u8>) -> Self {
        Self::new(v)
    }
}

impl From<&[u8]> for Aad {
    fn from(v: &[u8]) -> Self {
        Self::new(v.to_vec())
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_debug_is_redacted() {
        let pp = Passphrase::new("super-secret");
        let dbg = format!("{:?}", pp);
        assert!(!dbg.contains("super-secret"));
        assert!(dbg.contains("REDACTED"));
    }

    #[test]
    fn passphrase_equality_by_content() {
        let a = Passphrase::from("abc");
        let b = Passphrase::from("abc");
        let c = Passphrase::from("xyz");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn active_space_holds_space_id() {
        let id = SpaceId::from("550e8400-e29b-41d4-a716-446655440000".to_string());
        let active = ActiveSpace::new(id.clone());
        assert_eq!(active.space_id(), &id);
    }

    #[test]
    fn plaintext_debug_is_redacted() {
        let pt = Plaintext::new(b"hello world".to_vec());
        let dbg = format!("{:?}", pt);
        assert!(!dbg.contains("hello"));
        assert!(dbg.contains("REDACTED"));
        assert!(dbg.contains("11 bytes"));
    }

    #[test]
    fn plaintext_len_matches() {
        let pt = Plaintext::from(vec![1, 2, 3, 4]);
        assert_eq!(pt.len(), 4);
        assert!(!pt.is_empty());
    }

    #[test]
    fn ciphertext_is_opaque_but_inspectable() {
        let ct = Ciphertext::new(vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(ct.len(), 4);
        assert_eq!(ct.as_bytes(), &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn ciphertext_debug_shows_length_only() {
        let ct = Ciphertext::new(vec![1, 2, 3]);
        let dbg = format!("{:?}", ct);
        assert!(dbg.contains("3 bytes"));
    }

    #[test]
    fn aad_round_trip() {
        let bytes = b"space:abc|v2".to_vec();
        let aad = Aad::from(bytes.clone());
        assert_eq!(aad.as_bytes(), &bytes[..]);
        assert_eq!(aad.len(), bytes.len());
    }

    #[test]
    fn aad_equality_by_content() {
        let a = Aad::new(vec![1, 2, 3]);
        let b = Aad::new(vec![1, 2, 3]);
        let c = Aad::new(vec![4, 5, 6]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
