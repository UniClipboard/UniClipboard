# Native BoardUI component contract

Status: first-slice implementation. Acceptance evidence and unresolved platform limits are recorded in the [implementation receipt](../exec/boardui-mygo-implementation.md). This contract does not change Engine, plugin, sync or local-service contracts.

## Scope and source identity

The public Go package is `github.com/uniclipboard/unic/app/desktop/boardui`. It depends on mygo and Go standard-library resources. Core Engine and UI-free CLI/service programs must not depend on it. The existing three-skin UI Kit and production pages retain their existing interfaces.

The sole BoardUI source reference is official MIT export `ec5c5eb3e29c2441be63b9422cd54f411ef23466`. Source-derived code/resources retain the BoardUI MIT notice. Embedded Inter 4.1 retains OFL 1.1. Bundled Remix assets retain their independent license. [Source lock](../../app/desktop/boardui/assets/source-lock.json) binds source files, compiler dependency lock, font and glyph hashes. The repository root license remains AGPL-3.0. Upstream MIT, OFL and Remix notices retain their independent scope; this change does not relicense existing repository code. Public-source access is not authorization for Pro source. No Pro implementation is included.

## State and lifecycle

`Kit` is a value containing a scheme selection, not mutable widget or domain state. `LoadFonts` registers one embedded font family once per process. `Apply` installs the resolved scheme on a view context. Text and paint resources have no runtime web dependency.

Button returns `ui.Element`; callers read its native interaction result. Input accepts a nonnil `*string` and returns the real native editor element. Select accepts a nonnil selected-value pointer, stable item IDs and an optional open pointer; nil open state uses mygo's stable widget state. Dialog accepts a nonnil open pointer and composes a caller-owned body. Stable identities are required for repeated/reordered controls.

The caller owns validation, persistence, async work, selected data and cancellation. A theme switch must not reset these values. Native editor/focus/overlay machinery remains owned by mygo. There is no generic callback bus, renderer adapter or duplicated text-editing engine.

## Supported behavior

- Button preserves four named variants and three named sizes, leading/trailing glyphs, icon-only accessible names, disabled activation and source 0.98 press motion. Busy is a host extension: it exposes busy and blocks activation; it does not claim an additional official source visual variant.
- Input preserves label/description/error association, placeholder, password, read-only and disabled behavior in two sizes. Error replaces the visible description. Required paints the source marker and adds an accessible description; the fixed mygo bridge does not expose a separate required state.
- Select uses a real in-window listbox, stable choices, disabled choices, keyboard arrows/Home/End/Enter/Escape, outside dismissal, trigger focus and anchor/collision behavior. It composes mygo PopoverBase, FocusGroup and ButtonBase to retain nonmodal outside-action behavior. Typeahead is not implemented in this first slice. Selection is read through the bound value; no trigger Changed() notification is promised. This standalone package intentionally uses BoardUI surfaces on macOS too. It does not change production Kit native-menu policy.
- Dialog uses the fixed Settings Modal shell's surface/motion in a bounded synthetic form composition. It restricts Tab/Shift+Tab, closes the top overlay on Escape, supports outside dismissal and restores its opener. It is not a port of the newer source-unavailable generic Modal API. Initial programmatic panel focus occurs after mygo records the opener.

Unknown variants/resource names and nil mandatory pointers are programmer errors. The implementation fails visibly instead of silently selecting an approximate skin or missing token. Accessible names must be nonempty for icon-only actions. Glyphs are decorative; the action supplies its name.

## Geometry, rendering and acceptance

Shared fixture data, font bytes, viewport in DIP/CSS px, device scale, locale and theme identify a comparison. Reference images must be actual official React output at device-pixel dimensions. Native software screenshots are view evidence, not native-window or OS-compositor evidence.

The first-slice comparison checks control geometry within 0.5 DIP and empty flat control interiors within 2/255 per RGB channel. Text-only regions retain full unmasked raster metrics and source typography values; native/Chromium antialias differences are reported separately. No image translation, rescaling, blank-space similarity score or hidden text mask is allowed. Original images, side-by-side images, overlays, absolute differences, geometry and per-scene statuses remain bound by SHA256SUMS.

E2E must operate the real controls, not static images. Failure artifacts are immutable. Diagnose a repeated or relevant failure before delivery; passing retries do not erase it. Native OS keyboard/IME, accessibility bridges, high-contrast system policy, GPU material composition and Windows memory require explicit platform evidence. Unicode value editing in a software tester does not establish those native behaviors.

## Coverage boundary

The first slice contains Button, Input, Select and a derived Dialog, plus Text/icon/theme helpers and the standalone settings composition. It does not claim all 69 pinned install items. The research catalogue distinguishes foundations, base controls, app compositions, unknown-source free entries and Pro. Current implementation coverage must be recorded separately from that immutable research inventory.
