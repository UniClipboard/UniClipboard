# Desktop memory implementation and acceptance

Status: stage 1 implemented and accepted; stage 2 patch accepted in isolation, fork delivery pending; stage 3 not implemented. Baseline workspace: `e6f03d5174ecc55ec73981d67ce93ba2ddfa746d`. The installed-process investigation used `bd89ff5a12885079e4259c3a72c90ab5e8b420ca`; implementation uses the current workspace.

## Ordered scope

1. Replace the main-history and quick-panel 200 ms frame polling with background-result/event wakeups. Retain independent bounded recovery, retry, debounce, preview and invitation-expiry deadlines. Preserve main-thread model ownership, result-generation checks and cancellation.
2. Fix idle trimming in the existing mygo fork. Preserve unchanged-scene detection while trimmed and restore graphics only for actual content, geometry, format or compositor changes. Keep idle resource release enabled. Pin the immutable fork revision; do not modify the module cache or commit an absolute local replacement.
3. Bound decoded preview memory and avoid unnecessary CPU/GPU duplicates. Keep selected-detail semantics, viewport prefetch, dimensions, original content, copy/apply and Quick Look correct. Reuse unchanged source icons. Use maintained existing image scaling and mygo APIs.

The user selected an existing mygo fork. Its address is being resolved. The implementation does not replace the installed application or restart the user's service.

## Failure inventory written before implementation

- Removing frame polling can strand completions, catalogue/count results, send progress, mutations or service events. Every successful publication must wake the appropriate window.
- Events and results can arrive before the first view or while a window is hidden. Binding the window service and draining queued results must not lose notifications.
- Wake callbacks can race view changes or window destruction. Save only thread-safe weak window services; cancel timers and reject late wakeups after close.
- Completion queues can fill. Keep bounded backpressure and cancellation; do not add unbounded goroutines or queues.
- Recovery and failed-page retry can stop without frames. Schedule the next due deadline independently, without rendering a periodic heartbeat.
- Pairing-code expiry must still refresh only on the visible invitation page. Search, preview and blur deadlines must remain exact.
- The quick-panel satellite must not consume the main panel's wake destination. Bind the panel window and invalidate the satellite through the existing update path.
- A mygo trim fix can display stale pixels after content/size/format changes or recreate buffers for unchanged frames. Native acceptance must cover both unchanged requests and changed/resize/hide/reopen frames.
- Releasing CPU pixels too early can race uploads, software rendering or preview resize. Release on the view thread only after presentation and retain valid reconstruction ownership.
- A count-bounded preview cache can exceed byte limits. Account shared images once, separate thumbnail and full-detail ownership, and bound in-flight decoding.
- Large, corrupt or oversized images must keep existing error/file fallbacks. Do not change stored content or clipboard bytes to save preview memory.
- Selected off-viewport details and quick-panel previews must remain available within the declared budget. Evicted results cannot repopulate obsolete generations.
- A dependency checkout, tag or local override can point at the wrong source. Record fork identity, exact parent/fix commit, module version and accepted binary hashes.
- A 100 MB measurement can hide a correctness regression. Keep memory, async behavior, native frame/pixel correctness and process identity as distinct assertions.

## Acceptance

The canonical entry remains `make desktop-e2e`. Add the `idle-event-wakeup` native case before changing production code. It checks native settling and displays a new service-side history record without manual input or refresh. The native idle endpoint excludes future timers, so the process `event-wakeup` case also advances the actual renderer clock and asserts no heartbeat builds. Reuse process search/prefetch and incremental-browsing cases for cancellation, paging and recovery. Add image-budget/selection scenarios before image implementation. Emit reproducible source/binary identities, case receipts and native artifacts.

Run native memory controls against each candidate and keep aggregate counters rather than private heap dumps. Compare visible text, visible images, hidden background and restored windows separately. Do not claim a production 100 MB ceiling from synthetic controls.

## Image ownership chosen before implementation

Neighbor prefetch uses at most 256-pixel thumbnails. A 32 MiB conservative CPU/encoded thumbnail budget determines a power-of-two edge from the complete wanted window; larger viewports use smaller thumbnails. The selected image retains the existing original-resolution preview limit (16 million pixels / 32 MiB encoded) separately. Only one image decode runs at a time per process. A former selected full image returns to thumbnail ownership. Original dimensions, actual-size view and copied/stored bytes remain unchanged.

Each image preview owns its first-shown frame and releases CPU pixels on a later view frame through the existing Bitmap.ReleasePixels API. The reconstruction bytes remain valid. This one-shot lifecycle request is not a heartbeat. Budgeted prefetch still obeys the existing four-content-read limit. Peak original-image decoding and explicitly selected large-image use are separate from the idle 100 MB target.

## Results

- Production build and focused race checks passed.
- `idle-event-wakeup`: native async-delivery smoke passed. Its idle API deliberately excludes future timers.
- `event-wakeup`: stopped-renderer production-service acceptance passed. Two seconds of future renderer time produced no idle heartbeat frames. A baseline control using original production sources failed with ten idle rebuilds in two seconds. Both main-history and quick-panel async delivery and independent recovery deadlines passed. A focused input caret is legitimate animation; the background-idle phase removes keyboard focus.
- `search-contract` and `incremental-browsing`: passed with actual isolated services, bounded prefetch, cancellation and recovery.
- `go test -race ./...`: 569 passed, 213 failed. An independent original-HEAD archive reproduced the same 569 passing and 213 failing tests, including 210 default-file-cache error messages. These broad failures are baseline failures, not a passing full suite.
- A release-mode UI-only candidate with read-only access to the existing history used 97.31 MB visible, 111.88-111.99 MB hidden, and 100.87-102.13 MB after restoration. This candidate used the original mygo dependency plus aggregate instrumentation. It does not prove that every production state is below 100 MB.
- The isolated mygo patch retained idle trim and reduced 35 unchanged timed requests to one presentation and one trim. Actual native-screen pixel checks passed before and after hidden content changes, restoration and resize from 1100x720 to 800x600. The patch is not adopted by this repository until the user-specified existing fork is identified and pinned.
- Image-budget acceptance has been drafted but is not registered as implemented. It remains after fork delivery in the requested order.

The user-selected fork could not be identified in the enabled project catalogue, current account repositories, existing project module files or expected repository locations. Its GitHub address or local checkout path is required to publish the patch to the correct repository and pin its immutable commit. No new fork was created. The installed Desktop and its daemon were not restarted or replaced.

## Retained evidence

The canonical runner emits per-run source snapshots, binary hashes and self-contained reports under `.build/desktop-e2e`. Accepted targeted runs in this session were:

- `20261116T055148Z-c59abd99`: native `idle-event-wakeup`.
- `20261116T055529Z-2f9fc76e`: process `event-wakeup`.
- `20261116T055615Z-45270e4e`: process `incremental-browsing`.
- `20261116T055616Z-a17be556`: process `search-contract`.

The separate original-source replay reports the expected `idle renderer rebuilt 10 times without changes` failure. Private diagnostics retain only aggregate receipts, native synthetic-window pixel hashes and the portable mygo patch. Full temporary source and build copies are removed after the receipt is written.
