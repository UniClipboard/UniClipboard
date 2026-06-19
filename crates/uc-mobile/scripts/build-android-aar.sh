#!/usr/bin/env bash
# build-android-aar.sh — DESIGN PLACEHOLDER (goal-B build/release, decision D-Android).
#
# Android delivery is intentionally NOT implemented yet (iOS-first). This script
# is the agreed *shape* of the future Android pipeline so the structure is in
# place: it sits beside build-ios-xcframework.sh, the cdylib crate-type is
# already declared, and uc-mobile-proto/uc-mobile are target-agnostic. When
# Android is picked up, fill in the steps below and wire a parallel job into
# build-mobile-core.yml.
#
# Refuses to run so nobody mistakes it for a working build. Remove the guard
# (and the `exit`) when implementing.
#
# Intended pipeline (mirrors the iOS xcframework flow):
#   1. uniffi-bindgen Kotlin bindings:
#        cargo run -p uc-mobile --features bindgen-cli --bin uniffi-bindgen -- \
#          generate --library <host cdylib> --language kotlin --out-dir <dir>
#      -> uc_mobile.kt + the uniffi runtime shim (JNA-backed).
#   2. cargo-ndk cross-compile libuc_mobile.so for the Android ABIs:
#        arm64-v8a (aarch64-linux-android), armeabi-v7a (armv7-linux-androideabi),
#        x86_64 (x86_64-linux-android). Requires `cargo install cargo-ndk` +
#        the Android NDK + `rustup target add` for each. Keep the aws-lc-rs
#        absence assertion (ring-only), same as the iOS script (seam 1).
#   3. Assemble an Android library module: jniLibs/<abi>/libuc_mobile.so + the
#      generated Kotlin under src/main/kotlin, then `gradle :uc-mobile:assembleRelease`
#      -> UniClipboardCore-<version>.aar.
#   4. Runtime dependency the consumer app must add: net.java.dev.jna:jna
#      (uniffi Kotlin bindings call into the .so via JNA).
#
# Publish target (to decide when implementing): GitHub Packages (Maven) or
# Maven Central, versioned identically to the iOS line (`uc-mobile-v<version>`),
# from the SAME release/run so the iOS xcframework and the Android AAR are built
# from one commit. The Android consumer pins the Maven coordinate + version.

set -euo pipefail

echo "build-android-aar.sh is a DESIGN PLACEHOLDER — Android delivery is not implemented yet." >&2
echo "See the header comment for the intended pipeline, and docs/packaging/mobile-core-build-release.md." >&2
exit 64  # EX_USAGE: not runnable by design
