#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardIntegrationMode {
    Full,
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
