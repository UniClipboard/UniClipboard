use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClipboardOrigin {
    Local,
    Remote,
}

impl ClipboardOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClipboardOrigin::Local => "local",
            ClipboardOrigin::Remote => "remote",
        }
    }
}

impl From<&str> for ClipboardOrigin {
    /// Create a ClipboardOrigin from a string slice.
    ///
    /// Maps the string `"local"` to `ClipboardOrigin::Local` and `"remote"` to
    /// `ClipboardOrigin::Remote`. Any other value yields `ClipboardOrigin::Local`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::ClipboardOrigin;
    /// let origin = ClipboardOrigin::from("remote");
    /// assert_eq!(origin, ClipboardOrigin::Remote);
    /// ```
    fn from(s: &str) -> Self {
        match s {
            "local" => ClipboardOrigin::Local,
            "remote" => ClipboardOrigin::Remote,
            _ => ClipboardOrigin::Local, // Default to Local for unknown values
        }
    }
}

impl From<String> for ClipboardOrigin {
    /// Converts an owned string into a `ClipboardOrigin`.
    ///
    /// Maps the exact string `"local"` to `ClipboardOrigin::Local` and `"remote"` to
    /// `ClipboardOrigin::Remote`. Any other value defaults to `ClipboardOrigin::Local`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::ClipboardOrigin;
    ///
    /// let o1 = ClipboardOrigin::from("local".to_string());
    /// assert_eq!(o1, ClipboardOrigin::Local);
    ///
    /// let o2 = ClipboardOrigin::from("remote".to_string());
    /// assert_eq!(o2, ClipboardOrigin::Remote);
    ///
    /// let o3 = ClipboardOrigin::from("unknown".to_string());
    /// assert_eq!(o3, ClipboardOrigin::Local);
    /// ```
    fn from(s: String) -> Self {
        match s.as_str() {
            "local" => ClipboardOrigin::Local,
            "remote" => ClipboardOrigin::Remote,
            _ => ClipboardOrigin::Local, // Default to Local for unknown values
        }
    }
}
