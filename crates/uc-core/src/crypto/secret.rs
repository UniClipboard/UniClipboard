use std::fmt;
use std::ops::Deref;
use zeroize::Zeroize;

/// A sensitive string that must never be logged, cloned, or serialized.
///
/// 敏感字符串：
/// - 不可 Clone
/// - 不可 Serialize / Deserialize
/// - 不可 Debug / Display 输出真实内容
/// - Drop 时清零内存
pub struct SecretString {
    inner: String,
}

impl SecretString {
    /// Create a new SecretString.
    ///
    /// 创建一个敏感字符串。
    pub fn new(value: String) -> Self {
        Self { inner: value }
    }

    /// Borrow the inner secret as &str.
    ///
    /// 只允许通过借用方式读取。
    pub fn expose(&self) -> &str {
        &self.inner
    }

    /// Consume and return the inner String.
    ///
    /// 显式消耗，用于必须转交所有权的场景（谨慎使用）。
    pub fn into_inner(mut self) -> String {
        let mut tmp = String::new();
        std::mem::swap(&mut self.inner, &mut tmp);
        tmp
    }
}

/* ===========================
 * Trait implementations
 * ===========================
 */

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl Deref for SecretString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.expose()
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::new(value.to_string())
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.inner.zeroize();
    }
}
