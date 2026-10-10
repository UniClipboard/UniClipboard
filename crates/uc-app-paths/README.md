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

本 crate 负责路径计算，`AppDirs` / `AppDirsPort` / `AppDirsError` 类型留在
`uc-platform`。开发宿主通过运行时 `UC_PROFILE` 选择配置；可选的调用者默认值
通过 `compile_default` 参数传入。本 crate 没有特性开关，不决定错误映射，各调用者
自行把 `None` 转换为自己的错误类型。
