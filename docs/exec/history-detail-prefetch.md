# History detail prefetch

## Scope

Prefetch the complete visible history/search result range plus ten entries above and below it. Reuse the existing UI Kit viewport for the main list and grid. Use visible row bounds or the displayed image cells for the quick panel. Retain only the window and the selected detail. See the [architecture](../architecture/overview.md#history-detail-prefetch).

## Failure inventory

Written before implementation: incomplete visible coverage; incorrect grouped/grid indices; cached clicks rereading content; retained payload references after eviction; stale query/revision completions; duplicate overlapping reads; unbounded concurrency; reconciliation flicker; repeated error retries; loss of the active detail when scrolling.

## Acceptance

The existing `search-contract` process scenario uses an isolated real `unicd`, SQLite, production local API and Desktop binding. It injects inclusive viewport indices without opening a native window. Its receipt includes source hashes, service binary hash and named assertions. The canonical runner also records the frozen source manifest, checksum file and cleanup result.

Reproduce with:

```sh
make desktop-e2e CASE=search-contract
make desktop-e2e CASE=data-binding
```

The prefetch scenario covers 20 visible entries plus ten on each side, ready selection without rereading, concurrency bounds, bottom eviction, reverse scrolling, active-detail retention, unchanged reconciliation, query replacement and quick-panel cached previews. The existing data-binding scenario covers image, file and directory previews and real service reconnect behavior.

Native scrolling, physical pointer-to-detail latency and other-platform rendering remain NOT_RUN. The initial process scenario does not claim native visual acceptance. Native switching evidence is recorded below.

## Results (2026-11-13)

| Case | Result | Retained report |
| --- | --- | --- |
| search-contract | PASS with race detector | `.build/desktop-e2e/20261113T075028Z-98067669/index.html` |
| data-binding | PASS with race detector | `.build/desktop-e2e/20261113T075034Z-bf349913/index.html` |

Both runs report an unchanged frozen source snapshot and successful private cleanup. The search run's recorded hashes match the final production renderer, scheduler and acceptance source files. Each report has its own `manifest.json`, `SHA256SUMS`, case result and process receipt. No source, profiles or executable binaries remain in the retained evidence.

`go run ./tools/docscheck` passed with eight architecture freshness warnings. The required `-bump docs/architecture/overview.md` attempt rejected uncommitted source. During PR preparation, the implementation was committed as `6487a039d`, then the overview verified marker was refreshed to that commit. The final documentation check passed with seven remaining architecture freshness warnings. The first sandboxed E2E attempt emitted no case result; the normal-access rerun and final run passed. Native acceptance was NOT_RUN in these initial process runs.

## Native switching recording

Reproduce with `make desktop-e2e CASE=history-detail-prefetch`. This native case seeds eight synthetic text entries that fit completely inside the visible history list. It allows 1.2 seconds for the initial window to prefetch, then clicks forward and backward three times. It repeats the same sequence with an active search query.

The `ui-detail-cycle` command waits only until the clicked row reports selection, then checks the exact body in the same accessibility snapshot inside the History Detail region. It does not wait for an incorrect detail to become ready. It samples that region throughout a 300 ms dwell after each click and fails on a missing/stale body or any loading/fetching label. It verifies visible target bounds and an active search query. `detail-observations.json` retains the timestamp, phase, selected text, immediate-check flag, body-presence result and loading labels for every sample.

On macOS, a private Swift helper uses ScreenCaptureKit and AVAssetWriter to record only the owned native window at a requested 30 frames per second. It initializes the content filter on the main actor and preserves presentation timestamps. Other platforms use the existing window-content sampler with a requested 50 ms interval. Both paths retain `recording.mp4` and `recording-frames.json`; it does not assemble a video from assertion checkpoints. Actual cadence depends on capture latency. The recording and sampled accessibility observations do not establish the absence of an unobserved sub-frame transient, physical mouse delivery, OS focus or cross-application paste. Initial population and query replacement are setup; the no-loading assertion applies to the repeated switching phases after initial prefetch.

### Native result (2026-11-13)

`history-detail-prefetch` PASS: `.build/desktop-e2e/20261113T130635Z-f3214cf8/index.html`.

- The uncut 1100 × 720 H.264 video contains 1147 frames and lasts 39.99 seconds. It records only the test-owned window through ScreenCaptureKit.
- History performs 42 clicks; search performs 42 clicks. All 1008 detail-region observations contain the correct body and zero loading labels. All 84 first-selected-frame assertions pass.
- The history switching phase occupies approximately 4.79–18.88 seconds in the video. The search switching phase occupies approximately 24.32–38.58 seconds. Startup and search setup remain visible in the uncut recording.
- The source snapshot is unchanged, the final E2E source hashes match the accepted snapshot, and private source/profile/binary cleanup passes. The report retains the video, recording timestamps, detail observations, source/binary manifest and SHA256SUMS.
- The first attempt rejected a pre-selection snapshot; the driver now checks the same first snapshot that acknowledges the clicked row, without waiting for its body. The second attempt passed all 84 switching assertions but failed the new recorder's WindowServer initialization. Initializing AppKit and the content filter on the main actor fixed recording. Neither failed attempt is reported as accepted video evidence.

Reproduction requires the macOS Swift compiler and ScreenCaptureKit window-recording permission. The recorder helper is private test infrastructure and is absent from release binaries. These assertions cover text details in the main history list and its search results. Native scrolling, image/grid selection, physical input and other platforms remain outside this run.
