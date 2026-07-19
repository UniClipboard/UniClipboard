/// Whether the host connected a real system clipboard to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemClipboardWiring {
    /// Clipboard reads and writes reach the host operating system.
    Real,
    /// Clipboard reads and writes are no-ops.
    Noop,
}
