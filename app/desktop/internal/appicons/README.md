# Desktop icons

`unic-logo.svg` is the supplied artwork. The platform assets preserve its black field, white cat path, and aspect ratio.

| Platform | Artwork and delivery |
|---|---|
| macOS | `icon.icns` contains 16, 32, 128, 256 and 512 point images at 1× and 2×. The 1024 px master uses an inset 824 px rounded square for the existing macOS 13+ bundle. Launch Services supplies the packaged Dock icon. The unbundled binary uses `icon-macos.png`. |
| Windows | `icon.ico` contains 15 standard Win32 sizes from 16 to 256 px (`16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128` as 32-bit DIB and `256` as PNG) featuring the white pixel-cat with dark outline and drop shadow. Both executable architectures embed the ICO and PE `VERSIONINFO` metadata (from `release/products.toml`) via `tools/package/windows-icons.sh`, and the NSIS installer uses the same ICO. The main window uses the matching 256 px `icon-windows.png` through mygo. |
| Linux | `icon-linux.svg` uses a 128-unit canvas, a 96-unit field and a 4-unit lower face. Installed packages provide the scalable SVG and 48 and 512 px fallbacks in the hicolor theme. AppImage uses the 512 px PNG at its root. Linux windows and the About view use the same 512 px PNG. |

Platform references:

- [Apple app icon guidance](https://developer.apple.com/design/human-interface-guidelines/app-icons/) and [high-resolution ICNS production](https://developer.apple.com/library/archive/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/Optimizing/Optimizing.html)
- [Microsoft design grid](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-design) and [Win32 icon sizes](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-construction#icon-sizes-win32)
- [GNOME app icon geometry](https://developer.gnome.org/hig/guidelines/app-icons.html) and [Freedesktop icon installation](https://specifications.freedesktop.org/icon-theme/latest/#install_icons)

Run `python3 tools/package/desktop-icons.py` from the repository root on macOS. The generator uses ImageMagick 7, Apple `iconutil`, GNU windres and LLVM cvtres. Desktop builds use the checked-in assets.

## Tray artwork

The tray supports the default glyph and the classic cat. The glyph copies the front and back paths from UniClipboard v1 (`apps/gui-go/tray_icon_design.go`) on a 24-unit grid. The classic cat (`tray-cat@2x.png`, 64×64 RGBA) uses the white pixel-cat with dark outline and eyes, rendered as full-color non-template artwork so it remains legible on both light and dark panels without tinting.

macOS scales the cat to 38 pixels inside a 44-pixel transparent canvas; the glyph uses a centered 36-pixel template grid. Windows uses the system small-icon metric (`SM_CXSMICON`), delegating glyph template contrast to mygo and rendering the cat in full color. Linux uses 22 pixels with explicit KDE and gsettings palette rules for the glyph and full color for the cat.
