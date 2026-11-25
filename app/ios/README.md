# unic iOS

The mobile UI is shared with the other mobile platform in
[`app/expo`](../expo/README.md). This directory contains platform guidance only.
Do not create a separate native application here. Expo generates its native
project under `app/expo/ios`.

In-process Go Engine integration (`UnicCore.xcframework`) lives in the Expo
Local Module `app/expo/modules/unic-engine/ios`, following the
[Engine contract](../../docs/contracts/engine-contract.md) and reporting iOS
clipboard/background/Keychain capabilities honestly. The production app,
Share Extension (`.Share`), and Keyboard Extension (`.Keyboard`, UIKit-only,
no Go Engine linkage) share the `group.<bundleID>.incoming-share` App Group
(using separate `incoming-share/` and `keyboard-bridge/` subdirectories). The
[iOS candidate workflow](../../docs/architecture/releases.md#mobile-candidates)
requires distribution profiles for the app and both extensions. Provisioning
and Store acceptance require production credentials.
