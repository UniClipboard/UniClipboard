# unic iOS

The mobile UI is shared with the other mobile platform in
[`app/expo`](../expo/README.md). This directory contains platform guidance only.
Do not create a separate native application here. Expo generates its native
project under `app/expo/ios`.

In-process Go Engine integration (`UnicCore.xcframework`) lives in the Expo
Local Module `app/expo/modules/unic-engine/ios`, following the
[Engine contract](../../docs/contracts/engine-contract.md) and reporting iOS
clipboard/background/Keychain capabilities honestly. Production App Group,
Share Extension and Store signing remain separate scope.
