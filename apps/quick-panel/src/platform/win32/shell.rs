//! Handing links and files over to the desktop.

use std::os::windows::process::CommandExt;

use quick_panel_core::ports::PlatformError;
use windows::core::{w, HSTRING};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Opens a web link in the browser or a file in its default application.
pub fn open_target(target: &str) -> Result<(), PlatformError> {
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW reports success with a value above 32.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(PlatformError::CannotOpen)
    }
}

/// Shows `path` selected in a File Explorer window.
pub fn reveal_path(path: &str) -> Result<(), PlatformError> {
    // Explorer parses its own command line: `/select,` and the quoted path are one argument, and
    // it exits with status 1 even when it worked, so the exit status says nothing.
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn()
        .map(|_| ())
        .map_err(|_| PlatformError::CannotOpen)
}
