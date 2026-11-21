# Sidebar navigation feedback and scroll clearance

Status: overall layout implemented; focused E2E, composition regression and local native observation passed. User visual approval is pending.

The user supplied a cropped macOS sidebar with two identical gray row fills and
a visible scrollbar at the counts edge. Current generated colors map
background-tertiary-default and background-secondary-hover to the same RGB in
both schemes. NavState remains single-valued; identical paint alone is not
proof of two selections. The scroll viewport currently reserves no gutter.

Failure inventory: hover mutates the filter; selected and hovered rows are
indistinguishable; multiple AX selected states; row counts intersect the
scrollbar; overflow displaces the fixed footer; tag/all navigation stops filtering.

First extend canonical E2E with the complete history fixture, overflowing device
rows, native title-bar safe-area input, pointer hover/click/scroll, state and pixel
checks, fixed footer geometry and light/dark at 1x/2x. Retain the first red and
one diagnosed replay. Native manual observation remains separate.

The official MIT NavItem recipe at BoardUI export
`ec5c5eb3e29c2441be63b9422cd54f411ef23466`,
`components/application/dashboard/dashboard-sidebar.tsx` (SHA256
`741b45d739386999efc8825d4ce02aa6ec6d560bdde8d390f03abc2e3b073581`),
uses accent-500 to accent-600 with white selected label/icon, and
background-secondary-hover for unselected hover. This application navigation
composition must preserve that distinction without editing generated tokens.

## Results

The first canonical red run reproduced both defects at all four scheme/scale
combinations: identical selection/hover pixels and rows ending at the scrollbar
viewport edge. Single-selection state, filtering and fixed footer checks already
passed. The corrected `green` run passes every assertion after adding source
accent/white selected paint and a stable 12-DIP padding lane.

Receipts and exercised PNG checkpoints are under `.build/boardui-sidebar/red`
and `green`, with frozen source manifests, SHA256SUMS and cleanup results. The
public token generator and shared scrollbar visibility policy are unchanged.
Only the application SidebarNav row and scroll-content geometry changed; compact
rail and management source-list styling are outside this correction.

## Overall-layout clarification

The user clarified that the concern is the overall sidebar layout. Preserve the
verified local state/gutter fixes and extend the complete-view E2E before further
layout changes. The old composition stacks two 8-DIP padding layers, uses 38-DIP
rows and large accent action/link buttons, and gives adjacent groups only a
2-DIP gap. The navigation hierarchy is therefore weak even when selection is
correct.

Use one effective 12-DIP outer inset, the fixed BoardUI NavItem's 36-DIP row
(20-DIP composite type line plus 8-DIP vertical padding), 20-DIP icons and 8-DIP
icon/label spacing. Keep section actions quiet at 24 DIP, group separation at
12 DIP and More aligned with row labels at 24-DIP height. Keep only navigation
content scrolling and separate the fixed footer. These are application-layout
choices over native primitives and BoardUI resources; do not restore the v1 skin.

Additional pre-implementation failure modes: stacked outer padding, oversized
section action, groups running together, More centering instead of aligning to
labels, clipped footer and text/count columns colliding with the scrollbar.

The first overall-layout replay passed geometry/state except the More-text
assertion. The 480-DIP fixture placed More outside the scroll viewport, and a
native button folds visible text into its accessible name rather than exposing
a duplicate AX Text child. The corrected protocol first captures the complete
layout at 900 DIP and measures rendered More ink against the tag-label column,
then resizes to 480 DIP for overflow/footer assertions. Retain `layout-green`
as FAIL and run one diagnosed replay.

Comparing `layout-green` with `layout-corrected` exposed a real remaining layout
defect: after correcting AX/viewport sampling, rendered More ink began at x=97
while tag text began at x=48. ButtonBase centers its children by default; padding
alone did not make the link left-aligned. Explicit Start justification fixes the
actual layout. The final replay retains a tall original PNG plus geometry/ink
JSON before resizing for overflow. There were no unchanged-source retries.
SourceList geometry is isolated from SidebarNav so this navigation correction
does not silently resize management rows; the compact rail retains its old radius.

## Final overall-layout receipt

`layout-fixed` passes light/dark at 1x/2x. It retains tall layout originals and
ink/geometry JSON, then verifies a 480-DIP overflow window, single selection,
tag/all filtering and fixed footer actions. `composition-regression` passes the
canonical full component-composition case, including navigation, section actions,
More, compact rail and management source lists. `scroll-regression` passed the
unchanged shared scroll visibility policy after the gutter correction. The final
pinned scoped lint reports zero issues. The macOS production build passed.

The owned native preview was rebuilt with the final layout. It keeps native
traffic lights and the unified top band. Its actual full-window capture is
`.build/boardui-sidebar/native-layout.png`, with PID/binary identity in
`launch-layout.json`. The prior fixture's system close command failed and its
window remained observable. Cleanup terminated only that exact verified synthetic
preview PID before retaining the new process. This is a tool/preview cleanup
observation; native close acceptance is not claimed. No close/lifecycle API was
changed by the sidebar work.

Runtime scope: 36-DIP rows, 20-DIP icons, complete source typography, effective
12-DIP outer padding, 12-DIP section gaps, quiet 24-DIP section actions, a
24-DIP neutral More row aligned with labels, a separated fixed footer and a
stable scroll-indicator lane. No Engine/service/config contract changed. Native
Windows/Linux runs, native keyboard/IME and user visual approval remain NOT_RUN.
