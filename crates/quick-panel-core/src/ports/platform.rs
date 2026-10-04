//! Interfaces to the operating system.

use std::fmt;

/// Why a platform operation could not be done. The `Display` text is what the user is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    /// The platform has no implementation of this feature yet.
    Unsupported,
    PanelWindowInaccessible,
    PreviewWindowInaccessible,
    UnsupportedWindowKind,
    PanelWindowClosed,
    PreviewWindowClosed,
    PreviewLayerNotReady,
    MainThreadRequired,
    NoDisplay,
    InvalidLink,
    CannotOpen,
    /// Automatic paste needs an accessibility permission that has not been granted.
    NoPastePermission,
    /// The panel was opened without another application in front.
    NoPasteTarget,
    /// The application the paste goes to has quit.
    PasteTargetQuit,
    /// The user moved to another application after opening the panel.
    FocusMoved,
    CannotReturnToTarget,
    /// The paste target is missing at the moment of pasting, after the earlier check passed.
    PasteTargetMissing,
    CannotCreateTypingEvent,
    CannotCreatePasteEvent,
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsupported => crate::text::t().platform.unsupported,
            Self::PanelWindowInaccessible => crate::text::t().platform.panel_window_inaccessible,
            Self::PreviewWindowInaccessible => {
                crate::text::t().platform.preview_window_inaccessible
            }
            Self::UnsupportedWindowKind => crate::text::t().platform.unsupported_window_kind,
            Self::PanelWindowClosed => crate::text::t().platform.panel_window_closed,
            Self::PreviewWindowClosed => crate::text::t().platform.preview_window_closed,
            Self::PreviewLayerNotReady => crate::text::t().platform.preview_layer_not_ready,
            Self::MainThreadRequired => crate::text::t().platform.main_thread_required,
            Self::NoDisplay => crate::text::t().platform.no_display,
            Self::InvalidLink => crate::text::t().platform.invalid_link,
            Self::CannotOpen => crate::text::t().platform.cannot_open,
            Self::NoPastePermission => crate::text::t().platform.no_paste_permission,
            Self::NoPasteTarget => crate::text::t().platform.no_paste_target,
            Self::PasteTargetQuit => crate::text::t().platform.paste_target_quit,
            Self::FocusMoved => crate::text::t().platform.focus_moved,
            Self::CannotReturnToTarget => crate::text::t().platform.cannot_return_to_target,
            Self::PasteTargetMissing => crate::text::t().platform.paste_target_missing,
            Self::CannotCreateTypingEvent => crate::text::t().platform.cannot_create_typing_event,
            Self::CannotCreatePasteEvent => crate::text::t().platform.cannot_create_paste_event,
        })
    }
}

impl std::error::Error for PlatformError {}

/// What the running platform can do. The interface hides what is missing instead of failing on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Pasting into the application that was in front before the panel opened.
    pub auto_paste: bool,
    /// Opening the panel next to the pointer instead of in the middle of the screen.
    pub cursor_anchor: bool,
    /// Preview window drawn with a pointer towards the selected row.
    pub shaped_preview: bool,
    /// Opening a link or file, and showing a file in the file manager.
    pub open_and_reveal: bool,
}

/// The application a paste goes to, captured when the panel opens.
pub trait PasteTarget {
    /// Display name of the application, if there is one.
    fn name(&self) -> Option<String>;
    /// Whether a paste could be delivered now. Never opens a permission prompt.
    fn check(&self) -> Result<(), PlatformError>;
    /// Hands focus back to the target if this process still has it.
    fn return_focus(&self);
    /// Sends the paste shortcut to the target.
    fn paste(&self) -> Result<(), PlatformError>;
    /// Types `text` into the target.
    fn type_text(&self, text: &str) -> Result<(), PlatformError>;
}
