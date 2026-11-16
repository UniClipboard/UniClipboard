# Desktop icons

Copied byte-for-byte from UniClipboard v1 commit `c6483d73c7a0234be68571fdb5fa3d38a17c4871`.
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

The tray uses the v1 cat alpha mask. macOS scales it to 34 pixels inside a 44-pixel transparent canvas, matching v1 padding.
Windows delegates template contrast to mygo. Linux uses a colored cat to retain contrast on light and dark panels.
Native panel sizing, contrast and installed icon cache behavior require separate platform acceptance.
