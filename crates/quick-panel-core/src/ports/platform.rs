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
        use crate::text::platform as t;
        f.write_str(match self {
            Self::Unsupported => t::UNSUPPORTED,
            Self::PanelWindowInaccessible => t::PANEL_WINDOW_INACCESSIBLE,
            Self::PreviewWindowInaccessible => t::PREVIEW_WINDOW_INACCESSIBLE,
            Self::UnsupportedWindowKind => t::UNSUPPORTED_WINDOW_KIND,
            Self::PanelWindowClosed => t::PANEL_WINDOW_CLOSED,
            Self::PreviewWindowClosed => t::PREVIEW_WINDOW_CLOSED,
            Self::PreviewLayerNotReady => t::PREVIEW_LAYER_NOT_READY,
            Self::MainThreadRequired => t::MAIN_THREAD_REQUIRED,
            Self::NoDisplay => t::NO_DISPLAY,
            Self::InvalidLink => t::INVALID_LINK,
            Self::CannotOpen => t::CANNOT_OPEN,
            Self::NoPastePermission => t::NO_PASTE_PERMISSION,
            Self::NoPasteTarget => t::NO_PASTE_TARGET,
            Self::PasteTargetQuit => t::PASTE_TARGET_QUIT,
            Self::FocusMoved => t::FOCUS_MOVED,
            Self::CannotReturnToTarget => t::CANNOT_RETURN_TO_TARGET,
            Self::PasteTargetMissing => t::PASTE_TARGET_MISSING,
            Self::CannotCreateTypingEvent => t::CANNOT_CREATE_TYPING_EVENT,
            Self::CannotCreatePasteEvent => t::CANNOT_CREATE_PASTE_EVENT,
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
