# Desktop icons

`unic-logo.svg` is the supplied artwork. The platform assets preserve its black field, white cat path, and aspect ratio.

| Platform | Artwork and delivery |
|---|---|
| macOS | `icon.icns` contains 16, 32, 128, 256 and 512 point images at 1× and 2×. The 1024 px master uses an inset 824 px rounded square for the existing macOS 13+ bundle. Launch Services supplies the packaged Dock icon. The unbundled binary uses `icon-macos.png`. |
| Windows | `icon.ico` contains 14 sizes from 16 to 256 px. The icon uses a 48-unit grid with a 44-unit silhouette and a 2-unit exterior corner radius. Its neutral edge separates the black field from dark surfaces. Both executable architectures and the NSIS installer use the ICO. The main window uses `icon-windows.png` through mygo. |
| Linux | `icon-linux.svg` uses a 128-unit canvas, a 96-unit field and a 4-unit lower face. Installed packages provide the scalable SVG and 48 and 512 px fallbacks in the hicolor theme. AppImage uses the 512 px PNG at its root. Linux windows and the About view use the same 512 px PNG. |

Platform references:

- [Apple app icon guidance](https://developer.apple.com/design/human-interface-guidelines/app-icons/) and [high-resolution ICNS production](https://developer.apple.com/library/archive/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/Optimizing/Optimizing.html)
- [Microsoft design grid](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-design) and [Win32 icon sizes](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-construction#icon-sizes-win32)
- [GNOME app icon geometry](https://developer.gnome.org/hig/guidelines/app-icons.html) and [Freedesktop icon installation](https://specifications.freedesktop.org/icon-theme/latest/#install_icons)

Run `python3 tools/package/desktop-icons.py` from the repository root on macOS. The generator uses ImageMagick 7, Apple `iconutil`, GNU windres and LLVM cvtres. Desktop builds use the checked-in assets.

## Tray artwork

The tray supports the default glyph and the classic cat. Both come from UniClipboard v1 commit `c6483d73c7a0234be68571fdb5fa3d38a17c4871`. The glyph copies the front and back paths from `apps/gui-go/tray_icon_design.go` on a 24-unit grid. The cat uses the original alpha mask from `apps/gui-go/icons/`.

macOS scales the cat to 34 pixels inside a 44-pixel transparent canvas. The glyph uses a centered 36-pixel grid. Windows delegates template contrast to mygo. Linux uses the explicit KDE and gsettings palette rules for both styles.
