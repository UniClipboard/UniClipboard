//! Process-wide application state.

/// Runs the process as a background app: no Dock icon or menu bar, like a menu bar utility.
///
/// GPUI selects the regular activation policy just before it calls the launch callback, so this
/// has to run inside that callback rather than earlier.
pub fn run_as_background_app() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(main_thread) = MainThreadMarker::new() else {
        tracing::warn!("Not on the main thread; keeping the regular activation policy");
        return;
    };
    let switched = NSApplication::sharedApplication(main_thread)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    if !switched {
        tracing::warn!("Could not switch to the accessory activation policy");
    }
}

pub fn activate_app() {
    if let Some(main_thread) = objc2::MainThreadMarker::new() {
        #[allow(deprecated)]
        objc2_app_kit::NSApplication::sharedApplication(main_thread)
            .activateIgnoringOtherApps(true);
    }
}
