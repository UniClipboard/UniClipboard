# Wenshushu Action plugin

The independent implementation is
[UniClipboard/unic-plugin-wenshushu](https://github.com/UniClipboard/unic-plugin-wenshushu).
Implemented revision:
[`8e0c92fcb0a033cc0a934ef6ec193b391ad104e1`](https://github.com/UniClipboard/unic-plugin-wenshushu/tree/8e0c92fcb0a033cc0a934ef6ec193b391ad104e1)
(release [`v0.1.1`](https://github.com/UniClipboard/unic-plugin-wenshushu/releases/tag/v0.1.1) with prebuilt platform assets).
This directory references that repository; it does not duplicate its code.

The plugin exposes `unic.wenshushu/share`, titled “通过文叔叔临时分享”.
It accepts text, images and files, creates one temporary provider share, and
returns a public download URL as a new text history item. It uploads only when
the user explicitly invokes the Action. Directories are excluded.

Use a host that implements the current
[Action source contract](../../../docs/contracts/clipboard-action-contract.md#go-action-source-content).
The plugin pins host SDK revision `549ef96c8ab5dae7f5791d03a05a9726087de286`.
The generic host changes are on `feat/wenshushu-action-source` and are not yet
merged into the host default branch.

```sh
unic plugin install https://github.com/UniClipboard/unic-plugin-wenshushu --ref 8e0c92fcb0a033cc0a934ef6ec193b391ad104e1
unic plugin enable unic.wenshushu
```

Restart the service after enabling the plugin. See the independent
[acceptance receipt](https://github.com/UniClipboard/unic-plugin-wenshushu/blob/fc968e8efd8e4ac204194c21c2c08e34e9c26e7e/ACCEPTANCE.md)
for process/live-provider evidence and remaining native/release boundaries.
