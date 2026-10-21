# Tray cat image: source

`tray-cat@2x.png` is the tray image used when `general.trayIcon` is `cat`.

- Source: `apps/gui/src-tauri/icons/tray-icon@2x.png` at tag `v1.1.2`, copied byte for byte.
- sha256: `21e459b6f6fd8fa4de3335370f8198a400ae7d555550388cf48d239d4e34a93b`.
- Format: 44 x 44 RGBA PNG. Every visible pixel is black; the shape is in the alpha channel (a macOS template image).
- v1.1.2 used this image on macOS only (`include_bytes!` in `crates/uc-tauri/src/tray.rs`). Windows and Linux showed the application icon. The Go host before #1914 embedded `assets/tray-icon@2x.png` with the same hash.
- It is not the solid-cat vector of `c7209b188..a16a466e7`. That design never reached a release tag.
- Display per platform:
  - macOS: the embedded bytes go to `SetTemplateIcon` unchanged. The menu bar colours the template.
  - Windows and Linux: the host takes the alpha channel, scales it to the platform tray size and fills it with the platform foreground colour (`trayIconSpec()`). This is a new rendering derived from the macOS image. v1.1.x never shipped a cat on these platforms.
- Check: run `apps/gui-go/icons/check-tray-cat.sh` inside the repository. It exits 0 when the three hashes match.
