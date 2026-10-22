#![cfg(windows)]

use anyhow::{ensure, Context, Result};
use gpui::{font, px, Application};
use windows::Win32::Graphics::Gdi::LOGFONTW;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETICONTITLELOGFONT, SPI_SETICONTITLELOGFONT,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

const MISSING_FONT: &str = "UniClipboardMissingFont";

fn icon_font() -> Result<LOGFONTW> {
    let mut value = LOGFONTW::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETICONTITLELOGFONT,
            std::mem::size_of::<LOGFONTW>() as u32,
            Some(&mut value as *mut _ as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
    }
    Ok(value)
}

fn set_icon_font(value: &LOGFONTW) -> Result<()> {
    unsafe {
        // Neither persist the setting nor broadcast it to other applications.
        SystemParametersInfoW(
            SPI_SETICONTITLELOGFONT,
            std::mem::size_of::<LOGFONTW>() as u32,
            Some(value as *const _ as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
    }
    Ok(())
}

struct RestoreIconFont(LOGFONTW);

impl Drop for RestoreIconFont {
    fn drop(&mut self) {
        if let Err(error) = set_icon_font(&self.0) {
            eprintln!("Could not restore the test session's icon font: {error}");
        }
    }
}

#[test]
#[ignore = "Temporarily changes the session icon font; run only on a disposable Windows runner"]
fn unavailable_system_font_uses_gpui_fallback() -> Result<()> {
    ensure!(
        std::env::var("UC_WINDOWS_FONT_REGRESSION").as_deref() == Ok("1"),
        "Set UC_WINDOWS_FONT_REGRESSION=1 only on a disposable Windows runner"
    );
    let original = icon_font()?;
    let restore = RestoreIconFont(original);
    let mut unavailable = original;
    unavailable.lfFaceName.fill(0);
    for (target, value) in unavailable
        .lfFaceName
        .iter_mut()
        .zip(MISSING_FONT.encode_utf16())
    {
        *target = value;
    }
    set_icon_font(&unavailable)?;
    ensure!(
        icon_font()?.lfFaceName == unavailable.lfFaceName,
        "Windows did not expose the unavailable font required for this regression"
    );

    // GPUI captures the system UI font during application construction.
    let app = Application::new();
    drop(restore);
    ensure!(
        icon_font()?.lfFaceName == original.lfFaceName,
        "Could not restore the original session font"
    );
    let text = app.text_system();
    for name in [
        ".SystemUIFont",
        "UniClipboardMissingExplicitFont",
        "Segoe UI",
    ] {
        let id = text.resolve_font(&font(name));
        let resolved = text
            .get_font_for_id(id)
            .context("Resolved font is not cached")?;
        ensure!(
            resolved.family.as_ref() != MISSING_FONT,
            "Unavailable font was selected"
        );
        ensure!(
            text.advance(id, px(14.), 'A')?.width > px(0.),
            "Resolved font cannot measure a Latin glyph"
        );
        ensure!(
            id == text.resolve_font(&font(name)),
            "Font selection is unstable"
        );
    }
    Ok(())
}
