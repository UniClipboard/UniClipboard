# unic Android

The mobile UI is shared with the other mobile platform in
[`app/expo`](../expo/README.md). This directory contains platform guidance only.
Do not create a separate native application here. Expo generates its native
project under `app/expo/android`.

The Expo application currently uses demo state. Engine integration, clipboard
access, permissions, background execution and native extensions are outside the
UI-shell migration. Future integration must follow the
[Engine contract](../../docs/contracts/engine-contract.md) and report platform
capabilities accurately.
