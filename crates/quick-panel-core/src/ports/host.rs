//! The GUI that supervises the panel process.

/// Something only the GUI can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRequest {
    ShowMainWindow,
    OpenSettings,
}

pub trait HostLink {
    /// Asks the GUI to do something. The error is what the user is shown.
    fn send(&self, request: HostRequest) -> Result<(), String>;
}
