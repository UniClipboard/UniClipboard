# Desktop image memory implementation protocol

Written before product changes. Source baseline: origin/v2
173d362036ae4a746878306915c85bcdf7468751. Research was preserved in local
commit e8d764af5. The implementation branch is fix/desktop-image-memory.

## Failure inventory

- Requests waiting for decode can already own a fully materialized service response.
- A per-view bound can multiply across the main window and Quick Panel.
- FIFO neighbors or four occupied per-view jobs can starve a new selection.
- Cancelling queued work can accidentally acquire a slot, open a response, leak a
  slot or publish obsolete pixels. Cancelling active work must close its response.
- Request errors and service shutdown must return every slot and retained lease.
- Text and directory metadata keep their existing four-job bound. A single-file
  candidate is admitted conservatively because its name/MIME is returned only
  after response open. Multi-file names keep their existing bounded concurrency; no configuration layer or process-role redesign is introduced.
- Streaming DecodeConfig can retain an arbitrarily long pre-image header. Bound
  inspected headers by the existing 32 MiB input limit; replay only inspected
  header bytes, never deliberately buffer the full ordinary encoded input.
- Decoders can stop before trailing bytes, bypass the encoded limit, ignore a
  truncated tail, or allocate a huge declared raster. Bound input bytes and pixels
  before full decode; consume the bounded remainder before accepting a thumbnail.
- Cancellation can occur during header inspection, decode, resizing or encoding.
  Check it on reads and phase boundaries; do not force GC to mask retained memory.
- Preserve EXIF orientation, selected-original bytes/dimensions, neighbor budget,
  reconstruction, later-frame CPU release and complete visual previews.

## Test-first correctness and resource loop

Extend canonical image-previews before product edits. Use the real separate
service, production loadSearchEntry/Desktop/Quick Panel and existing transport
wrapper. Count responses from request open until Close, not first body Read.
Hold one real response briefly, queue neighbors and selected work, cancel a queued
request, and assert at most one image request is open across views, selected work
wins the next slot, queued cancellation opens no response, text proceeds, all
responses close and a later request still succeeds. Exercise cancellation of an
active response, HTTP error, and actual service shutdown/restart with pending work.

A separate real-service stream subcase replaces only its returned body with
bounded synthetic adversarial bytes. Assert chunk-sized input reads for a large
PNG, unchanged selected-original bytes, valid PNG/JPEG EXIF results, rejection of
truncated/corrupt/oversized/trailing-over-limit/huge-dimension/oversized-header
inputs, cancellation and subsequent successful reads. These are acceptance
scenarios through the production call path, not post-hoc unit tests.

## Prespecified native comparison

After explicit VM handoff, use non-race builds with identical diagnostic sources,
private initialized profiles, synthetic fixtures and fixed 1100x720 logical view,
DPI192, Windows skin and light theme. Do not read the system clipboard.
Compare baseline, admission only, streaming only and combined source variants.
Run three image repetitions per variant, with alternating order where possible;
run empty and text controls three times for baseline and combined. Native ARM64
is primary; matched x64-on-ARM64 controls remain separately identified.

Sample at 1 Hz: initial visible45s, hidden30s, reopened30s. Summarize visible30-45s
and hidden/reopened15-30s, plus sampled first10s peaks. Capture actual role/PID/tree,
private WS, total WS, private commit, handles, Go aggregate heap and cumulative
CPU. The unmerged version hides GUI to tray; it does not exit GUI. Standalone
tray-role costs are NOT_RUN unless that implementation is merged or independently
accepted at a separately identified source; never merge it for this experiment.

After reopened sampling, select the third synthetic image row. Require a new
full-resolution readiness event within8s, then observe10s and capture its native
client pixels. Record app-observed selection-to-original latency, process startup
to first original and command-to-observed-ready elapsed time. None is physical-input
latency. Record both initial and changed native selected-image crop hashes.
Require unchanged pixels and complete previews. Correctness: one open image
request; queued cancellation and controlled blocked-response cancellation finish
within500ms locally; no retained request at scenario exit. Performance acceptance:
combined median group private commit must improve versus matched baseline, with
no missing preview. Report any resident counter increase. Compare CPU and latency;
more than20% CPU or more than100ms selection-latency regression requires investigation
rather than silently accepting it. These are prespecified investigation thresholds,
not a promise of a fixed memory saving. Never sum the previous research deltas.

Retain small source/program hashes, phase aggregates, timing/pixel checks and
failure receipts. Remove task profiles, programs, source exports and raw captures.
Restore the acquired VM state after owned processes exit.

## Cached-selection priority refinement

Before correcting the scheduler condition, add a real Desktop/software-view plus
real-service regression. Cache all image neighbors, hold one outside image request,
occupy all four view jobs with neighbors, then select an already cached thumbnail
that needs an original-resolution upgrade. The selected item must be the next
image response after the held request. This distinguishes an upgrade from an
uncached detail: the existing detailBusy flag is false for cached thumbnails.
The initial implementation must go red on this scenario before changing the
reservation condition. No additional unit test is introduced.

## Native CPU investigation: bounded stream read granularity

The first ARM64 combined image run consumed5.016 group CPU seconds by45s versus
3.609 in its baseline, exceeding the prespecified20% investigation threshold.
The unbuffered streaming-only run consumed7.062 seconds. The initial cohort is
an exploratory failure, not final performance acceptance. Preserve its aggregates
and stop further unbuffered runs. Do not remove its negative result.

Before changing product reads, extend the real-service PNG resource acceptance
with a body-read call count. The first count assertion required at most128 total input reads for a3 MiB PNG.
The unbuffered code made482 calls. The buffered control made387 calls because IPC
legally returned short chunks, so that assertion was too strict and was retained
as a failed harness assumption. Correct the resource assertion to at most64
codec-sized requests smaller than64 KiB; retain total calls and the64 KiB maximum
request separately. This checks coarse bounded requests without assuming full reads. First run the current streaming code and require this assertion
to go red. Then test a single existing-library change: a64 KiB bufio.Reader over
the shared bounded input reader, reused by header replay, decode and tail drain.
This preserves strict consumed-byte accounting and streaming memory bounds.
Repeat the full matched matrix for the final source; never relabel the first
cohort's CPU or memory deltas as final optimized results.

The exploratory cohort contained a startup readiness event after10s. Keep the
prespecified first10s sampled peak, and additionally retain the full original45s
visible-phase maximum from the same raw samples. This supplementary descriptive
peak prevents late startup from appearing as a lower first10s cost. It is not a
changed acceptance threshold. Group medians use paired same-sample counter sums.
Native capture geometry must be2200x1440 at DPI192 before cropping.
