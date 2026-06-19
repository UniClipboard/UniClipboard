# 移动核心（uc-mobile）构建与发包 Runbook

本文是共享 Rust 移动同步核心 `uc-mobile` 的构建与发布操作手册。它把 `uc-mobile` 编译成
`UniClipboardCore.xcframework`（含 UniFFI 绑定 `uc_mobile.swift`），发布为 GitHub Release，
供 **独立的 iOS app 仓库** 经 SwiftPM `binaryTarget(url:checksum:)` 消费。

> 本流程 **只覆盖移动核心**。桌面 / CLI / server 的发包走各自既有的 tag-driven 流水线
> （见 `docs/release-workflow.md`），与本文互不影响。

## 1. 角色与产物

| 角色 | 仓库 | 关键文件 |
|---|---|---|
| 生产侧（Producer） | 本仓库 `UniClipboard/UniClipboard` | `crates/uc-mobile/scripts/build-ios-xcframework.sh`、`.github/workflows/build-mobile-core.yml` |
| 消费侧（Consumer） | iOS app 仓库 | `Package.swift`、`Scripts/update-rust-core.sh`、`RustCore/pinned.json`、`RustCore/uc_mobile.swift` |

每个 release 产出三件套（GitHub Release assets）：

- `UniClipboardCore.xcframework.zip` —— device(arm64) + universal simulator(arm64+x86_64) + macOS(arm64) 三 slice 的静态库框架。
- `uc_mobile.swift` —— UniFFI 生成的 Swift 绑定源。**与 `.xcframework` 同源、版本锁定**，必须配对采纳。
- `UniClipboardCore.checksum.txt` —— zip 的 `sha256`。该值即 SwiftPM `binaryTarget(checksum:)`
  所需值（已实测 `shasum -a 256` == `swift package compute-checksum`）。

## 2. 版本语义（决策 D1）

移动核心走 **独立版本线**，与桌面 `v*` 解耦：

- 版本单一真相源 = `crates/uc-mobile/Cargo.toml` 的 `version`（当前 `0.1.0`）。
  其它 workspace crate 仍用 `version.workspace`；**`uc-mobile-proto` 不脱钩**（它经 `uc-application`
  与桌面 daemon 共享，随桌面版本发布）。
- 发布 tag 命名 `uc-mobile-v<version>`，例如 `uc-mobile-v0.1.0`。前缀与桌面 `v*` glob 不重叠，
  不会触发 `release.yml` / `build-server-image.yml`。
- 何时 bump：移动核心的 FFI 接口、协议编解码或行为发生变化、需要让 iOS 采纳时。bump 是手动、低频的。

## 3. 切一个 release（生产侧）

1. 在本仓库 bump `crates/uc-mobile/Cargo.toml` 的 `version`（如 `0.1.0` → `0.1.1`），合并入主干。
2. 打开 GitHub Actions → **Build Mobile Core**（`build-mobile-core.yml`）→ Run workflow：
   - `dry_run = true`：**先干跑**。构建并把三件套作为 workflow artifact 上传，**不打 tag、不建 Release**。
     用于在正式发布前验证整条流水线（编译、aws-lc-rs 缺席断言、zip、checksum、体积）。
   - `dry_run = false` + `prerelease`（默认 true）：正式发布。workflow 会：
     a. 从 `cargo metadata` 解析版本 → `TAG=uc-mobile-v<version>`；
     b. 若该 tag 的 Release 已存在则 **直接失败**（提示先 bump 版本）；
     c. 跑 `build-ios-xcframework.sh`（`UC_MOBILE_BUILD_LOCKED=1 UC_MOBILE_BUILD_ZIP=1`）；
     d. 把体积报告写进 job summary（D5，目前 warn-only，未设硬阈值）；
     e. `gh release create` 建 tag + Release，附三件套 + release notes（含 commit、checksum）。
3. workflow 只在 `macos-latest` 跑（`xcodebuild` / `lipo` / Apple target 必需）。移动核心发版低频，
   macOS runner 成本可接受。

## 4. iOS 采纳一个 release（消费侧）

在 iOS app 仓库执行（**无需 Rust 工具链**，只要 `gh` 已登录 + `shasum`）：

```bash
Scripts/update-rust-core.sh 0.1.1
```

它会：下载该 `uc-mobile-v0.1.1` Release 的三件套 → 校验 `sha256` → 写
`RustCore/pinned.json`（`{version, url, checksum}`）→ 落 `RustCore/uc_mobile.swift` → 删掉本地可能
残留的 override xcframework。随后：

```bash
git add RustCore/pinned.json RustCore/uc_mobile.swift
swift build   # SwiftPM 按 pinned.json 的 url+checksum 下载并缓存 xcframework
```

`Package.swift` 的取用逻辑（三态）：

- **pinned 模式**：存在 `RustCore/pinned.json` 且无本地 override → `binaryTarget(url:checksum:)`。
- **local-dev override 模式**：存在 `RustCore/UniClipboardCore.xcframework`（本地 stage）→ 它优先，
  走 `binaryTarget(path:)`。
- **native-only**：两者都没有 → 不链接 Rust 核心，`UniClipboardModels/Network/Cache` 测试套照常构建
  （全新 checkout、未跑任何脚本时即此态，保证零 Rust 依赖也能 `swift test`）。

`RustCore/` 整体 gitignored，**只有** `pinned.json` 与 `uc_mobile.swift` 被追踪（`.gitignore` 用
`RustCore/*` + `!` 例外），所以 iOS 仓库不需要 Rust 工具链即可消费发布版。

## 5. 本地开发 override（对 worktree 直连调试）

核心开发者改了 Rust 后，想立刻在 iOS 上验证、不必先切 release：

```bash
# 在 iOS 仓库，指向本仓库（或某个 worktree）
UC_RUST_REPO=/path/to/uniclipboard Scripts/build-rust-core.sh
```

它会用本仓库的 `build-ios-xcframework.sh` 现场构建，并把 `UniClipboardCore.xcframework` +
`uc_mobile.swift` stage 进 `RustCore/`。此时 `Package.swift` 命中 local override 分支。注意：它会覆盖
被追踪的 `uc_mobile.swift`（出现本地 git diff），调试完 `git checkout RustCore/uc_mobile.swift` 或重跑
`update-rust-core.sh` 回到 pin 即可。

## 6. 不变量与排错

- **ABI 配对（D7）**：`uc_mobile.swift` 绑定与 `.a` 静态库由同一次构建的 uniffi 元数据生成，**必须配对**。
  绝不可只换其一。`update-rust-core.sh` 总是同时换两者；release 也总是同源发两者。
- **aws-lc-rs 必须缺席（seam 1）**：`build-ios-xcframework.sh` 内置 `cargo tree -i aws-lc-rs` 断言，
  命中即 fail（aws-lc-rs 会拖 cmake/clang 进 iOS 交叉编译并翻倍 crypto 体积）。
- **checksum 不匹配**：`update-rust-core.sh` 校验下载 zip 的 `sha256` 是否等于 `checksum.txt`；不等即
  报错退出（下载损坏或 Release 被改）。
- **`swift build` 报 checksum mismatch**：`pinned.json` 的 checksum 与远端 zip 不一致——重跑
  `update-rust-core.sh <version>` 重新 pin。
- **重复发布**：同版本的 Release 已存在时 workflow 直接失败，需先 bump `crates/uc-mobile/Cargo.toml`。

## 7. Android（占位，未实现）

Android 交付 **暂未实现**（iOS 优先）。设计骨架见 `crates/uc-mobile/scripts/build-android-aar.sh`
（运行会拒绝，仅记录形态）：uniffi Kotlin 绑定 + `cargo-ndk` 编三 ABI（arm64-v8a / armeabi-v7a /
x86_64）的 `libuc_mobile.so` + gradle 打 `.aar`；runtime 依赖 `net.java.dev.jna:jna`；发布候选
GitHub Packages(Maven) 或 Maven Central，与 iOS 同 `uc-mobile-v*` 版本、同源构建。`uc-mobile` 的
`cdylib` crate-type 已就绪，`uc-mobile-proto` / `uc-mobile` 与目标平台无关，无 Rust 侧阻塞。届时在
`build-mobile-core.yml` 加一个并行 job 即可。
