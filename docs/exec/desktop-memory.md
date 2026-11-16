# Desktop memory implementation and acceptance

Status: event-driven rendering and application image optimizations accepted for the exercised macOS scenarios. Full Desktop acceptance has one reproduced baseline translation failure. The optional framework patch remains separate. Baseline workspace: `e6f03d5174ecc55ec73981d67ce93ba2ddfa746d`. The installed-process investigation used `bd89ff5a12885079e4259c3a72c90ab5e8b420ca`; implementation uses the current workspace.

## Ordered scope

1. Replace the main-history and quick-panel 200 ms frame polling with background-result/event wakeups. Retain independent bounded recovery, retry, debounce, preview and invitation-expiry deadlines. Preserve main-thread model ownership, result-generation checks and cancellation.
2. Bound decoded preview memory and avoid unnecessary CPU/GPU duplicates. Keep selected-detail semantics, viewport prefetch, dimensions, original content, copy/apply and Quick Look correct. Reuse unchanged source icons. Use maintained existing image scaling and mygo APIs.

Application optimizations use the existing immutable mygo dependency. The optional idle-trim patch is diagnostic evidence only. It is not required or adopted. The implementation does not replace the installed application or restart the user's service.

## Failure inventory written before implementation

- Removing frame polling can strand completions, catalogue/count results, send progress, mutations or service events. Every successful publication must wake the appropriate window.
- Events and results can arrive before the first view or while a window is hidden. Binding the window service and draining queued results must not lose notifications.
- Wake callbacks can race view changes or window destruction. Save only thread-safe weak window services; cancel timers and reject late wakeups after close.
- Completion queues can fill. Keep bounded backpressure and cancellation; do not add unbounded goroutines or queues.
- Recovery and failed-page retry can stop without frames. Schedule the next due deadline independently, without rendering a periodic heartbeat.
- Pairing-code expiry must still refresh only on the visible invitation page. Search, preview and blur deadlines must remain exact.
- The quick-panel satellite must not consume the main panel's wake destination. Bind the panel window and invalidate the satellite through the existing update path.
- A mygo trim fix can display stale pixels after content/size/format changes or recreate buffers for unchanged frames. Native acceptance must cover both unchanged requests and changed/resize/hide/reopen frames.
- Releasing CPU pixels too early can race uploads, software rendering or preview resize. Release on the view thread on a later frame after drawing and retain valid reconstruction ownership.
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
- Before image optimization, `go test -race ./...`: 569 passed, 213 failed. An independent original-HEAD archive reproduced the same 569 passing and 213 failing tests, including 210 default-file-cache error messages. These broad failures are baseline failures, not a passing full suite.
- A release-mode UI-only candidate with read-only access to the existing history used 97.31 MB visible, 111.88-111.99 MB hidden, and 100.87-102.13 MB after restoration. This candidate used the original mygo dependency plus aggregate instrumentation. It does not prove that every production state is below 100 MB.
- The isolated mygo patch retained idle trim and reduced 35 unchanged timed requests to one presentation and one trim. Actual native-screen pixel checks passed before and after hidden content changes, restoration and resize from 1100x720 to 800x600. The patch remains optional diagnostic evidence and is not adopted by this repository.
- The full canonical Desktop run reports an `i18n` failure: `live_devices.go` uses an uncatalogued approval label. A separate canonical replay from the exact pre-image commit `f203085722809d8f00f545f67708ed958f252a0e` reproduces the identical failure. The replay freezes `git archive` source and records per-file hashes; its outer recorder HEAD remains the current checkout. This unrelated translation issue is not changed here.
- `image-previews`: real PNG history, selected original dimensions, former-selection downgrade, unchanged stored bytes, quick-panel selection, JPEG EXIF orientation and shown-bitmap lifecycle passed. Original production sources fail on a 1024-pixel neighbor raster. The thumbnail-only stage passed before CPU release. Before decode serialization, the same transport scenario fails with four overlapping image-body readers; serialization passes with one.
- Thumbnail-only native measurements: 91.75 MB visible, 102.24-105.17 MB hidden, 83.05-90.60 MB restored. Six images retain 694,272 bytes of CPU base/mips, compared with 14,735,712 bytes in the earlier attribution snapshot. The history changed between those snapshots; this is attribution evidence, not a fixed-corpus allocation benchmark.
- After shown-pixel release: 87.24-87.35 MB visible, 100.35-100.45 MB hidden, 86.61-88.92 MB restored. Retained image CPU pixels fall to 364,544 bytes, with no retained mip pixels. These measurements use ordinary Go collection and scavenging. They do not force GC or set GOGC/GOMEMLIMIT.

The installed Desktop and its daemon were not restarted or replaced. A hard 100 MB ceiling for arbitrary original images is not established. Decode scratch space, selected large originals and native compositor state remain distinct from idle thumbnail retention.

## Final same-history native comparison

The pre-image source is `f203085722809d8f00f545f67708ed958f252a0e`. The final application source is `4af0019e8fac9380c383169f1652e677a46aba7f`. The final instrumented snapshot was frozen before that commit; all 208 Desktop Go files match the committed files. Both probes use the same original pinned mygo version, with aggregate diagnostic counters only. No product fork patch is applied.

The probes run sequentially against the same existing read-only service, using a fresh production history view at 1100 x 720. Both retain 50 metadata entries, 23 wanted details, six image objects and 2,500 cached text bytes. The selected entry is text. Each 34-second run samples visible state at 5/9 seconds, hidden state at 17/21 seconds and restored state at 28/32 seconds. Units are decimal MB of macOS physical footprint. Collection and page scavenging follow ordinary Go behavior; no forced GC or GC environment tuning is used.

| State | Before image optimization | Final candidate |
| --- | ---: | ---: |
| Visible | 93.85-95.11 MB | 84.72-84.83 MB |
| Hidden | 106.85 MB | 94.48-94.69 MB |
| Restored | 96.05-97.31 MB | 75.71-82.00 MB |
| Retained image CPU base/mips | 14,735,712 bytes | 364,544 bytes |

The final candidate's hidden-state footprint exceeds its visible-state footprint in this sequential run. Between the 9-second visible and 21-second hidden samples, HeapAlloc falls from 21.31 MB to 9.42 MB and image CPU storage stays at 364,544 bytes. These counters exclude cache growth as the explanation; they do not identify the native allocation or accounting responsible for the physical delta. The retained selected vmmap lines cannot account for the full delta. Full native-category attribution and age-matched repeated hide/show controls remain follow-up work. Window.Hide is distinct from application hide, minimization and production close-to-tray behavior.

The final candidate is below 100 MB at all six sample points in this run. It does not establish a long-duration or all-content ceiling. Decode temporaries, a selected large original, allocator/scavenger timing and native compositor state can exceed this idle scenario. The earlier per-stage numbers are independent process runs; do not attribute their entire physical-footprint difference to a single code change.

The complete canonical Desktop run finishes with 44 PASS and one FAIL (`i18n`), with private cleanup PASS. The exact pre-image source reproduces that translation failure. Image previews, event wakeups, search/prefetch, incremental browsing, data binding, native history copy, startup and all native scenarios pass. Pinned golangci-lint v2.15.0 reports zero issues for desktopui. Formatting, three entrypoint builds and docscheck pass. Docscheck retains seven pre-existing stale-page warnings outside the updated overview.

## Retained evidence

The canonical runner emits per-run source snapshots, binary hashes and self-contained reports under `.build/desktop-e2e`. Accepted targeted runs in this session were:

- `20261116T084547Z-b9d944a8`: full canonical Desktop run, 44 PASS / one reproduced baseline FAIL.
- `20261116T055148Z-c59abd99`: native `idle-event-wakeup`.
- `20261116T055529Z-2f9fc76e`: process `event-wakeup`.
- `20261116T055615Z-45270e4e`: process `incremental-browsing`.
- `20261116T055616Z-a17be556`: process `search-contract`.
- `desktop-memory-i18n-baseline`: identical original-source translation failure.
- `20261116T084521Z-26095673`: process `image-previews`, including serial image reads.
- `20261116T084434Z-4254140c`: expected red control before serialization (four overlapping readers).
- `20261116T083943Z-26ea8fd8`: thumbnail-only process acceptance before CPU release.

The separate original-source replay reports the expected `idle renderer rebuilt 10 times without changes` failure. Private diagnostics retain only aggregate receipts, native synthetic-window pixel hashes and the portable mygo patch. Full temporary source and build copies are removed after the receipt is written.

## Current application optimization sequence

The user requested independent application-side optimization after exact attribution. Framework-fork identity is not a prerequisite. Implement and measure neighbor thumbnails, then CPU-pixel release, then bounded decode admission. Do not force GC or change GC tuning as an optimization.

Additional failure inventory before implementation:

- A thumbnail must not change original dimensions, file kind, EXIF orientation, copy/apply bytes or actual-size selected preview.
- Selecting a thumbnail must show it immediately while upgrading to the original; selecting another item must downgrade the old original and reject stale read tiers.
- A changing viewport can outgrow a per-item bound. Quantize thumbnail edge from the entire wanted window so estimated retained CPU/encoded thumbnail storage stays within 32 MiB.
- Tiny invalid or oversized payloads retain existing fallback/error semantics; image formats with EXIF must keep their oriented dimensions.
- Images shared between cache, entry projections and retained selection need one lifecycle record; eviction must clear every payload reference.
- Pixel release must run on the view thread only after a previous image view frame; reconstruction bytes must stay valid. Ask for one lifecycle frame, never a heartbeat.
- Waiting for a decoder slot must honor context cancellation; every admitted exit must release its slot. Text/content jobs retain their existing four-reader concurrency.

Use the existing x/image scaling capability where sufficient. JPEG auto-orientation is not provided by the standard decoder; use an established pure-Go image library for the bounded thumbnail path rather than copying EXIF parsing. The full-resolution selected path keeps mygo's existing decoder.

Final native aggregate receipts, exact source-file hashes and reproduction scripts remain under `.cache/memory-image-current-baseline` and `.cache/memory-image-step3`. The baseline used a `git archive` snapshot. The final snapshot records its dirty-source parent plus per-file hashes. Diagnostic source copies, mygo copies, executable probes, temporary profiles and raw logs are removed after measurement. The installed Desktop PID 94960, daemon PID 707, start identities and installed executable SHA256 remain unchanged.

## PR integration verification

The branch integrates `v2` at `25605176cc05d4f1fc4c6a40ea4764fb01632e3f` in merge `058a6334b`. Only the overview verification marker required conflict resolution. Both upstream migration/tag behavior and icon/tray behavior remain present. After integration, canonical `image-previews` (`20261116T112549Z-b61d9ce8`), `event-wakeup` (`20261116T112550Z-0c279d71`) and `production-boundary` (`20261116T112551Z-99e5560e`) pass. Pinned golangci-lint v2.15.0 reports zero desktopui issues. Canonical `i18n` (`20261116T112618Z-8bc34bde`) reproduces the same uncatalogued approval label. The full 45-case run and native memory measurements above precede this base integration; no new native footprint claim is made for the merged icon/tray changes.
