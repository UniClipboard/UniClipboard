# Inbound clipboard loop regression

Status: implemented; final committed-source validation is recorded in generated run manifests. Research session: c99afa5a-e4df-4f99-a4c9-a3cbef22a9ca.
Base: ab72df12f4c8018562b42abe8c3da3b8a3767314; Engine: 3d3ecb22166f0edab545ce8b4d0b6c71397ea471.

## User operation and failures (before code)

Copy synthetic text, an image, one file, and multiple files on A. Real daemon A must capture and send through the production pairing/transport path. Real daemon B must decrypt/materialize history and write its native pasteboard. Its actual polling watcher must observe the ownership version and finish processing without a local capture or outbound dispatch. A must not gain a return entry. A subsequent distinct local copy on B must still synchronize to A.

Failures to detect: disabled/unstarted watcher; shared system clipboard bypassing transport; snapshot/identity mismatch; premature negative assertions; watcher dedup masking attribution; local capture with history dedup; failed outbound dispatch with accepted=0; partial/missing file transfer; image byte conversion; lost evidence; leaked credentials; orphaned test processes. Same-content deliberate recopy requires its own assertion. Adversarial concurrency remains a separate retained case.

## Smallest implementation

Reuse the existing private NSPasteboard injection library and production HTTP pairing endpoints. Extend its native tool with checked image/multiple-file writes and read-only digest inspection. Use only synthetic fixtures, independent portable development profiles, and owned child processes. No access to the user's general clipboard or system key store.

Engine currently awaits one clipboard process_change before polling HostClipboardChangeStream::next again. Desktop can therefore emit a per-cycle delivery and subsequent poll completion receipt, including whether the queued snapshot was consumed. Correlate native ownership version, watcher read, host cycle and actual Engine capture/dispatch events. This establishes completion of the synchronous capture/attribution path without duplicating Engine origin logic. It is not a general fence for arbitrary detached work; scan outbound initiation through clean process shutdown as an additional assertion. A watcher-dedup event cannot count as identity attribution success.

Reject a new Engine control API or general task-drain abstraction for the sequential base slice. Engine remains pinned. Do not require total B-to-A network silence: active-state re-broadcast is legitimate. Reject two profiles sharing generalPasteboard as an isolation strategy.

## Delivery

- [x] Native helper and black-box dual-daemon runner written before product instrumentation.
- [x] Real failure evidence before adding observation receipts.
- [x] Privacy-safe native/watcher/host cycle diagnostics.
- [x] Sequential text/image/single-file/multiple-file receive and local-copy control.
- [x] Old unmodified receiver reproduces actual local recapture/dispatch.
- [x] Path-routed macOS CI, commands and artifacts.
- [x] Committed source identity, binary/tool hashes, sanitized events, assertions and cleanup receipts.

## Acceptance boundaries

The named-pasteboard run is isolated native process integration with real AppKit, watcher, transport, Engine and persistence. General clipboard product E2E requires two independent desktop environments and remains NOT_RUN until actually executed. Do not reinterpret a missing capability, failed pairing or missing completion receipt as a pass. No newly written unit tests. Documents and comments in this task are English.

## Local results

The original unmodified receiver reproduced local recapture and outbound dispatch in `baseline/run-09ee9d2dd4134630a2f899ad0770b0bc`. Missing diagnostics in the first red run were not counted as original-defect reproduction.

The same-content recopy scenario failed before the watcher change in `recopy-red/run-09240ba681c14a31a5b532f7b014b474`. Content dedup suppressed an actual new ownership version. The watcher now forwards a changed reliable ownership token to Engine attribution before content heuristics can suppress it. Platforms without reliable identities retain their existing heuristics.

The candidate run `candidate/run-1d9d7f10ff094367a534b2b5b1bd7ba7` passed all twelve scenarios. Existing watcher and host identity regression checks, the 56 existing CI-routing checks, Cargo check, Cargo formatting and actionlint passed. These are supporting checks; the native runner provides the scenario acceptance. Final clean-source reruns and binary hashes are recorded in the generated manifests rather than embedded in this document. Artifacts live under the ignored `target/test-artifacts/inbound-clipboard-loop/` directory.

An early harness assertion incorrectly treated the UI `isEncrypted` projection as a persistence contract. Source inspection showed that projection is not suitable for that assertion. The runner instead scans database/WAL bytes for exact synthetic text canaries. Complete AEAD validation remains NOT_RUN.

Two-desktop general pasteboard acceptance, real screenshot-application behavior, Windows/Linux runtime, and adversarial concurrent writes remain NOT_RUN. No general task-drain fence or Engine API change was introduced.
