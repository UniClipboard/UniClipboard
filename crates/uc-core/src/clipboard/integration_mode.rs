/// Clipboard integration mode determines how the app interacts with the OS clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardIntegrationMode {
    /// Full integration: observe OS clipboard changes and allow paste operations
    Full,
    /// Passive mode: only allow paste operations, don't observe OS clipboard
    Passive,
}

impl ClipboardIntegrationMode {
    pub fn observe_os_clipboard(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub fn allow_os_read(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub fn allow_os_write(&self) -> bool {
        matches!(self, Self::Full)
    }
}
