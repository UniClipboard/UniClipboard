//! Pasting into the application that was in front before the panel opened.

use core_graphics::event::{CGEvent, CGEventFlags};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use quick_panel_core::ports::{PasteTarget, PlatformError};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// The application that was in front when the panel opened.
pub struct FrontApplication(Option<Retained<NSRunningApplication>>);

impl FrontApplication {
    pub fn capture() -> Self {
        Self(
            NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .filter(|app| app.processIdentifier() != std::process::id() as i32),
        )
    }

    fn target_is_front(app: &NSRunningApplication) -> bool {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .is_some_and(|front| front.processIdentifier() == app.processIdentifier())
    }

    /// Brings the target application back to the front and waits until it is there.
    pub fn bring_front(&self) -> Result<(), PlatformError> {
        let app = self.0.as_ref().ok_or(PlatformError::PasteTargetMissing)?;
        if Self::target_is_front(app) {
            return Ok(());
        }
        #[allow(deprecated)]
        app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        for _ in 0..60 {
            if Self::target_is_front(app) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(PlatformError::CannotReturnToTarget)
    }
}

impl PasteTarget for FrontApplication {
    /// Display name of the application the paste goes to, if there is one.
    fn name(&self) -> Option<String> {
        self.0
            .as_ref()
            .and_then(|app| app.localizedName())
            .map(|name| name.to_string())
    }

    fn check(&self) -> Result<(), PlatformError> {
        // Accessibility permission is checked without opening a system prompt.
        if !unsafe { AXIsProcessTrusted() } {
            return Err(PlatformError::NoPastePermission);
        }
        let app = self.0.as_ref().ok_or(PlatformError::NoPasteTarget)?;
        if app.isTerminated() {
            return Err(PlatformError::PasteTargetQuit);
        }
        // The panel itself is the active application while it is open; anything else in front
        // that is not the target means the user moved on.
        let ours = std::process::id() as i32;
        if NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .is_none_or(|front| {
                front.processIdentifier() != app.processIdentifier()
                    && front.processIdentifier() != ours
            })
        {
            return Err(PlatformError::FocusMoved);
        }
        Ok(())
    }

    /// When the panel is the active application, hands focus back to the target. Nothing
    /// happens if the user has already gone to another application.
    fn return_focus(&self) {
        let ours = std::process::id() as i32;
        if NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .is_some_and(|front| front.processIdentifier() == ours)
        {
            let _ = self.bring_front();
        }
    }

    fn type_text(&self, text: &str) -> Result<(), PlatformError> {
        self.check()?;
        self.bring_front()?;
        let app = self.0.as_ref().ok_or(PlatformError::PasteTargetMissing)?;
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| PlatformError::CannotCreateTypingEvent)?;
        let down = CGEvent::new_keyboard_event(source.clone(), 0, true)
            .map_err(|_| PlatformError::CannotCreateTypingEvent)?;
        let up = CGEvent::new_keyboard_event(source, 0, false)
            .map_err(|_| PlatformError::CannotCreateTypingEvent)?;
        down.set_string(text);
        up.set_string(text);
        down.post_to_pid(app.processIdentifier());
        up.post_to_pid(app.processIdentifier());
        Ok(())
    }

    fn paste(&self) -> Result<(), PlatformError> {
        self.check()?;
        self.bring_front()?;
        let app = self.0.as_ref().ok_or(PlatformError::PasteTargetMissing)?;
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| PlatformError::CannotCreatePasteEvent)?;
        let down = CGEvent::new_keyboard_event(source.clone(), 9, true)
            .map_err(|_| PlatformError::CannotCreatePasteEvent)?;
        let up = CGEvent::new_keyboard_event(source, 9, false)
            .map_err(|_| PlatformError::CannotCreatePasteEvent)?;
        down.set_flags(CGEventFlags::CGEventFlagCommand);
        up.set_flags(CGEventFlags::CGEventFlagCommand);
        // Target the retained application's PID rather than the global event stream.
        down.post_to_pid(app.processIdentifier());
        up.post_to_pid(app.processIdentifier());
        Ok(())
    }
}
