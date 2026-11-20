# Desktop image memory implementation

## Scope and identity

The user approved production image-request admission and streaming thumbnail input.
The starting product source is origin/v2
173d362036ae4a746878306915c85bcdf7468751. The completed
[Windows research](windows-desktop-memory.md) is preserved in local commit
e8d764af5. The implementation branch is fix/desktop-image-memory.
The independent lightweight-process branch is not included. WM_CLOSE hides this
version's GUI; it does not exit it. No push, PR, merge or release is authorized.

The [protocol](desktop-image-memory/protocol.md) records failure modes, test-first
acceptance and performance investigation thresholds before product edits.

## Behavior

One shared image request queue admits content before the service can materialize
a response. Selected originals precede queued neighbors. Cancellation removes a
queued waiter; a race with slot transfer returns that slot. Response close occurs
before releasing admission on every content/error path. A view with four pending
neighbor jobs cancels a neighbor so the new selection can take the next job.
Text and directory reads keep their separate existing bounds. Single-file previews
share image admission because their MIME/name arrives with response headers.

Thumbnail decoding streams bounded encoded input and replays only inspected image
headers. The existing32 MiB encoded and16-million-pixel bounds remain. The header
uses that same byte bound; the extra overflow byte is enabled only after header
inspection. Draining the bounded remainder catches codecs that stop before an
oversized tail. Cancellation checks cover reads and phase boundaries. The codec
and resize functions remain bounded calls rather than preemptible operations.
Original selection, EXIF orientation, reconstruction bytes, shown-pixel release,
preview completeness and the32 MiB thumbnail reservation remain unchanged.

## Test-first evidence

Canonical `make desktop-e2e CASE=image-previews` reproduced both defects twice
before product changes. Runs20261120T055308Z-798a9f23 and
20261120T055405Z-7a3b94f9 observed four simultaneously open image responses and
a largest thumbnail input read of1,277,952 bytes while serial body consumption
still passed. Selected priority and queued cancellation also failed.

Admission-only run20261120T055647Z-5d129a98 passed request admission, selected
priority, queued/active cancellation, text independence and actual service
stop/restart recovery. Its remaining failure was the intentionally red streaming
assertion. The combined run20261120T055807Z-b00292a7 passed with the race detector:
one open response, zero remaining responses, five requests opened after excluding
the canceled waiter, and largest streamed read30,047 bytes. PNG, JPEG EXIF, selected
original bytes/dimensions, Quick Panel and later-frame CPU release passed.
Malformed/truncated/oversized-header/huge-raster/trailing-over-limit/read-error
inputs and cancellation were rejected; the next valid request succeeded.

A further test-first refinement covered a selected item whose thumbnail was
already cached while four neighbor jobs occupied the view. Run
20261120T061011Z-45c10e90 failed because detailBusy was false during an original
upgrade. The scheduler now tests the required resolution tier independently of
that display flag. Run20261120T061154Z-345d068e passed the full race-instrumented
case, including the selected request taking the next response slot. Prepared
binaries from before this refinement were discarded without running in Windows.

No unit test was added after implementation. The acceptance additions drive the
production view/content path and a real separate service through the canonical
runner. Native Windows cost, native pixels and software-view correctness are
separate evidence.

## Bounded-input CPU refinement

The first native cohort at e540ad3aa was stopped after its first combined ARM64
run exceeded the prespecified CPU investigation threshold:5.016 group CPU seconds
by45s versus3.609 in the matched baseline. Streaming-only used7.062 seconds.
All five completed exploratory runs, native pixel hashes and interruption/cleanup
recovery are preserved in [unbuffered-native](desktop-image-memory/unbuffered-native/measurements.json).
They are not a completed3-repetition final performance matrix.

A test-first real-service PNG resource check observed482 small body-read requests.
The first total-call assertion was too strict because IPC can return short reads;
its failed result is retained. A64 KiB standard bufio.Reader over the same shared
limiter separates codec request sizes from socket reads. Header replay, decode and
tail drain all consume that same buffer, preserving byte accounting. The corrected
resource assertion counts requests smaller than64 KiB rather than assuming every
read fills its buffer. The full canonical race case passed in
20261120T063709Z-39a67bd9, retaining total reads and largest request separately.
No whole-input thumbnail buffer or new configuration is introduced.

## Native measurements

PENDING explicit VM handoff and matched non-race measurements. Previous research
deltas are not formal implementation savings and must not be added together.

## Validation and cleanup

PENDING final receipts, documentation verification and resource cleanup.
