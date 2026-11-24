# unic Android

The mobile UI is shared with the other mobile platform in
[`app/expo`](../expo/README.md). This directory contains platform guidance only.
Do not create a separate native application here. Expo generates its native
project under `app/expo/android`.

In-process Go Engine integration (`unic-engine.aar`) lives in the Expo
Local Module `app/expo/modules/unic-engine/android`, following the
[Engine contract](../../docs/contracts/engine-contract.md) and reporting Android
10+ clipboard, SAF and network capabilities honestly. Production keystore signing
and Play Store distribution remain separate scope.
