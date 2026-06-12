#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsVersion {
    V1,
}

impl SettingsVersion {
    pub const CURRENT: SettingsVersion = SettingsVersion::V1;

    /// Convert the settings version into its numeric representation.
    ///
    /// # Returns
    ///
    /// The numeric version: `1` for `V1`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::version::SettingsVersion;
    ///
    /// let v = SettingsVersion::V1;
    /// assert_eq!(v.as_u32(), 1);
    /// ```
    pub fn as_u32(self) -> u32 {
        match self {
            SettingsVersion::V1 => 1,
        }
    }
}
