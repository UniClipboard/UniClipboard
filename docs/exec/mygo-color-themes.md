# Code-defined mygo color themes

## Delivered behavior

The shared native presentation exposes 160 resolved color tokens per scheme. The
[catalogue](../../app/desktop/boardui/COLOR-TOKENS.md) records current sRGB values.
The [theme guide](../../app/desktop/boardui/README.md#code-defined-color-themes)
documents immutable construction, scheme defaults, overrides and host injection.
Theme authors use at most twelve base colors per scheme through
`NewPaletteTheme`, including one-color brand definitions. `WithOverrides` applies
optional final token adjustments. Two additional foreground roles separate text
on accent/danger from literal white. Separate dialog/popover surface tokens make
`Elevated` control floating surfaces without changing the page or control fill. Generated BoardUI source resources,
typography, geometry and licenses are unchanged.

Public controls, application compositions, gradients, shadow layers and native
primitive defaults resolve through the selected theme. The main Desktop, setup
and QuickPreview expose code-defined theme injection. Independent role processes
receive their theme from their own host code. No theme file, editor or persisted
preference is introduced. Custom menu swatches bypass the shared default cache;
default cache keys include the scheme and remain bounded.

## Token-layer acceptance (first phase)

| Check | Result / artifact directory under `.build/desktop-e2e/` |
|---|---|
| Shared custom theme E2E | PASS: `20261122T234207Z-ebea4709` |
| Public settings composition | PASS: `20261122T234305Z-222737e4` |
| Public control state matrix | PASS: `20261122T234305Z-381c1427` |
| Main/settings/Quick Panel composition | PASS: `20261122T234315Z-b1551f5f` |
| In-content menus | PASS: `20261122T234425Z-872992cf` |
| Desktop build and all Desktop test-package compilation | PASS |
| Targeted `go vet` | PASS |
| Pinned golangci-lint v2.15.0 on affected packages | PASS: 0 issues |
| Pinned formatter diff and catalogue freshness | PASS |
| `go run ./tools/docscheck` | PASS with seven stale-page warnings |
| `git diff --check` | PASS |

Each canonical report retains source identities, assertions and software-view
checkpoints. The theme case also exports `theme-tokens.json`. These reports do
not establish native-window, compositor, OS integration or user visual acceptance.
No native app or browser was opened for this work.

## First failure and bounded correction

The first theme run, `20261122T234020Z-d4401187`, failed when returning from dark
to light. Its artifact remains intact. The test changed an external Kit value
and called `Tester.Advance`, which does not request a frame for idle content.
The correction explicitly calls `Tester.Frame` after changing the theme.
Two bounded reruns passed: `20261122T234058Z-5bdd28ff` and
`20261122T234207Z-ebea4709`. The stale-frame cause is resolved.

The system linter could not read the repository's v2 configuration. The pinned
linter initially selected an unavailable Go download. Running the pinned linter
with the already installed Go 1.27.3 completed successfully.

The overview `verified` bump was attempted and refused because the covered code
is uncommitted. The existing SHA remains unchanged. The component contract has
no architecture front matter and does not support `-bump`. Re-run the overview
bump after committing the reviewed code; do not name the old HEAD as verification
of an uncommitted implementation.

## Twelve-color authoring acceptance

The expanded scenario defines brand-only light/dark palettes and complete
12-color palettes. It edits a real Input, toggles a Switch, activates Buttons,
opens and dismisses Dialogs, switches schemes without replacing state, checks
rendered background pixels, and retains unique PNG/accessibility checkpoints.
`palette-tokens.json` records the resolved tokens for all five scenes. Empty
palettes retain every source token. Exact overrides retain transparent values
without mutating their parent. Source-blue primary states and accidental literal
white changes are checked explicitly.

| Check | Result / artifact directory under `.build/desktop-e2e/` |
|---|---|
| Brand-only/full palettes and exact overrides | PASS: `20261123T010708Z-2b2c0912` |
| Expanded palette scene with floating surfaces and dialogs | PASS: `20261123T011008Z-88dedd1b` |
| Existing raw token theme API | PASS: `20261123T010827Z-862c0851` |
| Application composition regression | PASS: `20261123T010827Z-869958bd` |

These are software-view results. No native app or browser was launched.
The composition regression includes the drawer path. The earlier drawer shadow
code multiplied the stored alpha twice. It now passes only the animation factor
to mygo Color.Alpha, whose documented operation multiplies the stored alpha.

Default settings acceptance passed: `20261123T011054Z-8cf27455`.
The final Desktop build, all Desktop test-package compilation, targeted go vet,
pinned lint (0 issues), formatter diff, catalogue freshness and whitespace checks
passed. docscheck passed with the same seven stale-page warnings. The architecture
verified bump still requires committed code, as recorded above.

The final palette artifact contains five resolved palettes, 160 tokens per
palette and ten PNG checkpoints (control scene plus dialog per palette). The
larger internal token count is not the authoring API: a theme author supplies up
to twelve base colors per scheme or one Accent for a brand change.
