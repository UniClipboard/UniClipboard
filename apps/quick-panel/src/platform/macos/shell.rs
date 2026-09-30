//! Handing links and files over to the desktop.

use quick_panel_core::ports::PlatformError;

pub fn reveal_path(path: &str) -> Result<(), PlatformError> {
    use objc2_foundation::{NSArray, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    Ok(())
}

/// Opens a web link in the browser or a file in its default application.
pub fn open_target(target: &str) -> Result<(), PlatformError> {
    use objc2_foundation::{NSString, NSURL};
    let url = if target.starts_with("http://") || target.starts_with("https://") {
        NSURL::URLWithString(&NSString::from_str(target)).ok_or(PlatformError::InvalidLink)?
    } else {
        NSURL::fileURLWithPath(&NSString::from_str(target))
    };
    if objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err(PlatformError::CannotOpen)
    }
}
