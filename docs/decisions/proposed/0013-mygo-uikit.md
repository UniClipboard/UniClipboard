# 0013 · One native BoardUI presentation for Desktop

- Status: accepted; production migration and its acceptance are tracked separately.
- Original decision date: 2026-11-09.
- Updated decision date: 2026-11-20.
- Current facts: [repository architecture](../../architecture/overview.md#native-boardui-presentation), [Desktop runtime](../../architecture/desktop-runtime.md), [component contract](../../contracts/boardui-native-contract.md).
- Execution: [production migration](../../exec/boardui-production-migration.md); the [0013 implementation receipt](../../exec/0013-mygo-uikit.md) is historical evidence for the superseded implementation.

The filename and decision number remain stable so existing references resolve. The former Adwaita/macOS glass/Fluent three-skin proposal is superseded. It is not an alternative current production option.

## Context

The first implementation ported a separate React Kit contract into mygo. It used platform skins, platform token snapshots and accepted visual approximations. The historical reference declared 59 public components. That number described the old source contract, not native BoardUI coverage.

The user now requires BoardUI's own visual language across Windows, macOS and Linux, production adoption and removal of the old shared UI solution. The accepted first slice already provides real native controls and exact-source comparison tooling. The design must preserve existing product behavior and native integration while replacing the visual system.

## Decision

1. Use official MIT BoardUI source export `ec5c5eb3e29c2441be63b9422cd54f411ef23466` as the fixed source baseline. Actual source takes precedence over stale comments or newer skill recipes. No Pro or source-unverified component is implied by the public catalogue.
2. `app/desktop/boardui` is the single reusable public native control/resource package. It uses mygo behavior primitives and source-generated semantic colors, composite typography, dimensions, gradients, shadows and motion. It has no React, WebView, Tailwind or Node runtime dependency.
3. `app/desktop/internal/desktopkit` contains application-only compositions and adapters for existing Desktop models. It consumes BoardUI resources and controls. It is not a platform-skin engine, public component library or independent token source. `desktopui`, setup, Quick Panel and both demos share this presentation.
4. Remove `internal/uikit`, its old token snapshots/generator, platform typography and icon overrides, and the skin selector. There is no production Adwaita/macOS glass/Fluent fallback or selectable legacy visual scheme. `system`, `light` and `dark` remain color-scheme choices.
5. Preserve native OS ownership: title bars, traffic lights/caption reservations, drag and resize regions, caption actions, modifiers, tray/system menus, file/credential prompts, updater windows, notifications, shortcut registration, clipboard and focus/paste dispatch. In-content controls and menus use the unified BoardUI presentation.
6. Keep caller-owned values, stable IDs, validation, persistence, async cancellation and application domain models. Reuse mygo input, selection, focus, modal, menu, scrolling and virtualization behavior. Do not rewrite text editing or move UI concerns into Engine/service packages.
7. Retain the pinned Inter font/OFL, functional Remix glyphs and their independent notice/source hashes. Generate resources from the inspected official source and exact reference compiler. Do not hand-edit generated values or redraw platform icon variants.
8. Migrate runnable slices, then remove every old consumer. A renamed legacy skin engine is not completion. The final product has one visual system; application-specific compositions remain explicit and do not claim every BoardUI catalogue item has been ported.

## Geometry and interaction

BoardUI control dimensions replace the old per-platform field/button sizes. Composite type records preserve font, size, line height, weight and tracking together. Layout must accommodate the real control and hint/error/focus extents rather than compressing them into old macOS dimensions.

Keep the established safety invariants: visible keyboard focus; unclipped focus rings; shrinkable fields and command hit targets; viewport-aware empty states; stable list selection; explicit fingerprint approval; destructive dialogs initially focused on cancellation; and overlay focus restoration. The old macOS glass, press/sheet motion and system-menu presentation recipes are not mandatory content styling after this decision.

The fixed source scales a pressed Button to 0.98. Its settings-modal surface uses a black-at-70% backdrop. These actual source facts override the newer no-scale guidance and the stale 20% backdrop comment. Source-unavailable generic Modal APIs are not invented; the bounded Dialog is explicitly derived from the inspected Settings Modal shell.

## Acceptance and limits

Use E2E scenarios written before the corresponding change and retain reproducible artifacts. Compare actual official React components against native views with fixed source/dependency/font hashes, locale, data, scheme, viewport and DPI. Keep raw device-pixel originals, geometry, color/type values, semantic assertions, differences and SHA256SUMS. Preserve first failures and investigate relevant regressions before delivery.

Production behavior acceptance must exercise the real isolated service and shared renderer. The first-slice geometry/color gates do not automatically accept new compositions, every component state or the whole production interface. Native OS keyboard/IME, platform accessibility, compositor/material behavior and Windows memory require separate platform evidence. Software views or cross-compilation cannot establish those results. User visual approval remains separate.

The old [0013 receipt](../../exec/0013-mygo-uikit.md) and other earlier execution records remain historical evidence. Their old skin choices, approximations and past passes do not describe the current design or accept the new production presentation.
