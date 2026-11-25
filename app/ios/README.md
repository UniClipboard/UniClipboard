# unic iOS

The mobile UI is shared with the other mobile platform in
[`app/expo`](../expo/README.md). This directory contains platform guidance only.
Do not create a separate native application here. Expo generates its native
project under `app/expo/ios`.

In-process Go Engine integration (`UnicCore.xcframework`) lives in the Expo
Local Module `app/expo/modules/unic-engine/ios`, following the
[Engine contract](../../docs/contracts/engine-contract.md) and reporting iOS
clipboard/background/Keychain capabilities honestly. The production app and
Share Extension use a separate incoming-share App Group. The
[iOS candidate workflow](../../docs/architecture/releases.md#mobile-candidates)
uses both distribution profiles. Provisioning and Store acceptance require
production credentials.
