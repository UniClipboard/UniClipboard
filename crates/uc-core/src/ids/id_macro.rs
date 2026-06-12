//! Common macro for implementing ID wrapper types.

macro_rules! impl_id {
    ($($name:ident),* $(,)?) => {
        $(
            impl $name {
                pub fn new() -> Self {
                    Self(uuid::Uuid::new_v4().to_string())
                }

                pub fn from_string(s: String) -> Self {
                    Self(s)
                }

                pub fn from_str(s: &str) -> Self {
                    Self(s.to_string())
                }

                pub fn inner(&self) -> &String {
                    &self.0
                }

                pub fn into_inner(self) -> String {
                    self.0
                }
            }

            impl Default for $name {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "{}", self.0)
                }
            }

            impl From<String> for $name {
                fn from(s: String) -> Self {
                    Self(s)
                }
            }

            impl From<&str> for $name {
                fn from(s: &str) -> Self {
                    Self(s.to_string())
                }
            }

            impl AsRef<str> for $name {
                fn as_ref(&self) -> &str {
                    &self.0
                }
            }

            impl Into<String> for $name {
                fn into(self) -> String {
                    self.0
                }
            }

            impl std::ops::Deref for $name {
                type Target = String;

                fn deref(&self) -> &Self::Target {
                    &self.0
                }
            }
        )*
    };
}

pub(crate) use impl_id;
