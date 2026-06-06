# uc-app-paths

The **directory-layout authority** for UniClipboard: the single source of truth
for *where* the app's data and cache directories live.

It owns the path-resolution policy — the app directory name
(`app.uniclipboard.desktop`), the `UC_PROFILE` suffix, the portable ("green")
redirect, and the per-platform base directories — and exposes them as pure
functions that depend on **only** `dirs` + `std`.

## Why this crate exists

Two crates need this exact policy but live on opposite ends of the dependency
weight spectrum:

- `uc-platform` — the heavyweight platform layer (keyring / clipboard / objc2 /
  wayland / tokio-full) that owns the `AppDirsPort` implementation.
- `uc-daemon-process` — a deliberately thin, dependency-light crate that
  resolves the daemon PID/token paths without dragging the app stack into the
  CLI client (ADR-008 P5).

Before this crate existed (ADR-008 P5-0), `uc-daemon-process` carried a
byte-identical *copy* of the resolution because it could not depend on the heavy
`uc-platform`. Two copies = drift risk. ADR-008 P5-0c extracts the policy here so
both consumers share one implementation, and a future "split cache / log /
user-data dirs" change happens in exactly one place.

## What stays out

This crate owns the *raw computation*, not the abstraction. The
`AppDirs` / `AppDirsPort` / `AppDirsError` types stay in `uc-core` /
`uc-platform`. The `dev-profile` compile-time feature stays in `uc-platform`
and is threaded in here via the `compile_default` parameter. This crate has no
features and makes no error-mapping decisions: each consumer maps `None` to its
own error type.
