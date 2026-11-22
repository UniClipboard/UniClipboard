# Tailcat connected-publication race

Date: 2026-11-22. Draft PR: [#2285](https://github.com/UniClipboard/UniClipboard/pull/2285).
Investigation started at `93a570df7b44172918be46d9de7a7d237743dfc3`.

## Scope and evidence boundary

The historical `TestSubmitDeliversEverything` failure completed one
`builtin.tailcat` target with `unavailable: the transfer failed`. The operation
completed in 15 ms; the whole test took 0.41 seconds. Its retained log does not
include the underlying backend response or transport error.

This test uses two actual Engines and the production built-in session backend
over a loopback TCP Transport fixture. It does not exercise Tailcat routing,
DERP, Internet connectivity or system clipboard access.

The unchanged test passed 20 isolated iterations and one complete Engine race
suite before instrumentation. These passes did not explain the failure. The
investigation then added an E2E scheduling barrier before changing production
code. It paused the sender immediately after Engine accepted its `connected`
event, before the authority callback returned to the backend.

The first controlled run failed. A bounded-release version failed three of
three iterations. Each observed backend response was `not_found`, and the
public operation had the same unavailable/transfer-failed result as the
historical failure. The original and bounded first-failure artifacts are
retained separately under `.cache/tailcat-transfer-investigation/`.

## Confirmed cause of the controlled failure

1. The backend elects a session.
2. Its `connected` callback publishes that session to Engine.
3. Engine can now select that session for a submission.
4. The backend has not yet returned from the callback or added the session to
   its local map.
5. A concurrent `Send` finds no local session and returns `not_found`.
6. Engine sanitizes that backend response into the public generic transfer
   failure.

No source-file failure or network failure is needed to reproduce this path.
The production file `internal/session/engine.go` had the same Git blob
`ecfbf13b6c40addc77d751055d18bdd1f7d32b9d` in original base
`894790a3a4dc512ab622b0cf9120895251955173` and the pre-fix PR head. The race
therefore predates the external plugin-instance implementation.

This is a confirmed cause of the controlled reproduction, not unique
retrospective attribution of the historical invocation. That invocation lacks
the diagnostic information needed to distinguish all paths to the same public
error. Keep the existing merge hold and draft state.

## Fix

One backend publication RWMutex serializes session election, the connected
callback and session registration with `Send` session lookup. Engine can still
publish its event during the callback, but a send waits for that publication
boundary to finish before looking up the session. Competing establishments
cannot interleave the election and registration steps.

The backend state mutex remains released during authority callbacks. Send
releases the publication lock before authorization, file opens and wire I/O.
Existing callback timeouts remain in effect. No public API, protocol, trust
rule, automatic retry, delay or reconnect policy changes.

## Regression and reproducible artifact

The regression uses the original mixed-content assertions: exact text, HTML,
PNG, inline file and referenced file arrive at the other Engine. It holds the
connected callback while a send enters, then releases it when the send either
finishes prematurely or waits 100 ms. The 100 ms scheduling barrier exists
only in the test. Cleanup always releases it. The receipt contains only the
test name, verdict, stable backend response code and synthetic payload class.

```sh
TMPDIR=/private/tmp UNIC_E2E_ARTIFACTS="$PWD/.cache/tailcat-readiness-replay" \
  go test -race ./engine \
  -run '^(TestTailcatConnectedPublicationDeliversEverything|TestSubmitDeliversEverything)$' \
  -count=20 -timeout=2m -v
```

Inspect `.cache/tailcat-readiness-replay/connected-publication.json` for the
controlled case. The console reports every iteration. The committed
[receipt](evidence/tailcat-connected-publication/receipt.json) binds the tested
source files and retained local evidence by SHA-256. It preserves the
historical attribution limit.

## Verification

- Before fix: original isolated test 20/20 PASS; complete Engine race suite
  PASS. These were baseline observations.
- Before fix: initial controlled E2E FAIL; bounded controlled E2E 3/3 FAIL,
  all with backend `not_found`.
- After fix: original E2E 20/20 PASS and bounded controlled E2E 20/20 PASS
  under the race detector. All controlled responses were `success`.
- `TMPDIR=/private/tmp go test -race ./... -count=1 -timeout=5m`: PASS.
- Production headless E2E with real binaries and two service processes:
  PASS. Its manifest is
  `.cache/tailcat-transfer-investigation/production-after-fix/manifest.json`.
  Separately gated migration suites were not run.
- Pinned golangci-lint v2.15.0 with Go 1.27.3: PASS, zero issues.
- Documentation checks: PASS with four stale-page warnings. Pipeline and
  identity verification markers reference implementation commit `8bbc04226`.

No native application, browser, VM, real clipboard or cross-application paste
was used. No release, merge or deployment is authorized by this result.
