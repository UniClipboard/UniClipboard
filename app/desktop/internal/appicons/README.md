# Desktop icons

The four binary assets below were copied byte-for-byte from UniClipboard v1 commit `c6483d73c7a0234be68571fdb5fa3d38a17c4871`.
The source directory is `apps/gui-go/icons/`.

| Asset | SHA-256 |
|---|---|
| `icon.png` | `4f73034c64869baee5ced45b8b6899c74aa4ba5635f5fe596445a2579e00aa7b` |
| `icon.icns` | `c2e8f8b06afb6321c6ae8da264d6293a0317575e6164bc3c3b43a93eb5bf26cd` |
| `icon.ico` | `dc691b4edab0b4adba8fe01bcdea0409562a986c415ccd163f6ae7605736f6fe` |
| `tray-cat@2x.png` | `21e459b6f6fd8fa4de3335370f8198a400ae7d555550388cf48d239d4e34a93b` |

The PNG is shared by the main window, macOS Dock and About view.
macOS bundles use the original ICNS. Linux packages and AppImage use the original PNG.
Windows executables embed resources generated from the original ICO for amd64 and arm64.
Regenerate these resources with `sh tools/package/windows-icons.sh` (GNU windres and LLVM cvtres required).
The installer also uses the original ICO.

The tray defaults to `glyph` and also supports `cat`, matching v1. `tray-glyph.svg` copies the exact front/back paths and stroke geometry from `apps/gui-go/tray_icon_design.go` at the source commit above. It uses a transparent 24-unit grid and the current foreground color. The application renders it with existing mygo SVG support, rather than maintaining a separate SVG rasterizer.

The cat PNG remains unchanged. macOS scales it to 34 pixels inside a 44-pixel transparent canvas, matching v1 padding; the glyph uses a centered 36-pixel design grid. Windows delegates template contrast to mygo. Linux reuses the v1 explicit KDE/gsettings palette rules for both styles.
Native panel sizing, contrast and installed icon cache behavior require separate platform acceptance.
