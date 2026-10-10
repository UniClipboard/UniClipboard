//! The language the system is set to.

use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

/// The display language, as a BCP 47 tag such as `zh-CN`. Windows has no locale environment
/// variables, so this reads the user's preferred UI language list and takes its first entry.
pub fn system_language() -> Option<String> {
    let mut count = 0_u32;
    let mut length = 0_u32;
    unsafe {
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length).ok()?;
    }
    if length == 0 {
        return None;
    }
    let mut buffer = vec![0_u16; length as usize];
    unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(windows::core::PWSTR(buffer.as_mut_ptr())),
            &mut length,
        )
        .ok()?;
    }
    // A multi-string: the first language ends at the first NUL.
    let end = buffer.iter().position(|unit| *unit == 0)?;
    (end > 0).then(|| String::from_utf16_lossy(&buffer[..end]))
}
