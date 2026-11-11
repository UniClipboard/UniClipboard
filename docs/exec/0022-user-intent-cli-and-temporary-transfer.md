# 0022 execution record: user-intent CLI and temporary transfer

- Current behavior: [headless architecture](../architecture/headless.md#command-surface-and-temporary-transfer), [local service contract](../contracts/local-service-contract.md#temporary-transfer-routes) and [Engine contract](../contracts/engine-contract.md#63-device-selection-and-temporary-transfer).
- Date: 2026-11-10
- Scope: CLI command surface, `send --to`, temporary transfer (host and CLI), interoperability with the browser transfer page.
- Not in scope: a GUI entry, the page's reply (`push`), production verification of the `uc-web-try` code service.

## Steps

- [x] 1. Rename and regroup commands. Behavior unchanged; catalog, suites and current docs use the new names.
- [x] 2. `send --to all|DEVICE` (`CommitRequest.DeviceIDs`).
- [x] 3. Temporary transfer: `internal/webtransfer`, `internal/tcnet/ephemeral.go`, `Engine.AdmitTransfer`, host routes `/v1/transfers` and `/v1/receive`, CLI `send` and `recv`.
- [x] 4. Interoperability with the real page code, both directions.
- [x] 5. Ticket-kind dispatch: `recv` refuses a pairing code, `device add` refuses a transfer code.

## Deviations from the first draft of 0022

1. `send --to ADDRESS` was dropped. The page's protocol only lets the receiver pull, and a bare address carries no token. A transfer always starts on the side that holds the content.
2. Bare `tc…` addresses are refused by `recv`. A link or a code carries the token.
3. `recv list|show|dismiss` stay under `recv`; any other first word is a source.
4. `device list` also prints devices waiting for approval, which replaces `requests`. Scripts that asserted "no pending request" now assert the absence of `Waiting for your approval`.
5. `device add` takes no argument (shows a pairing code), a 6-digit code, or an address.

## Receipt

| Check | Command | Result |
|---|---|---|
| Transfer e2e (unic to unic, link and code, `--to`, wrong-kind codes) | `make transfer-e2e` | PASS, 3 of 3 final runs (`send-to-device`, `transfer-code`, `temporary-transfer`). The first run failed on connection rebuild after pairing; two `retry-copy` warm-up lines fixed it. |
| Page interoperability, both directions | `make web-interop-e2e UC_WEBSITE=…/uc-website` | PASS. `page-to-unic` 11821 ms, `unic-to-page` 12519 ms, 65536-byte payload; receipt `web-interop-receipt.json`. |
| CLI testscript suite after the rename | `UNIC_E2E=1 go test ./e2e/cli -run TestCLI` | PASS (full TestCLI, plus PostHog, headless, logs-export and desktop-startup suites). |
| Lifecycle suite (race, tagged binaries) | `make service-lifecycle-e2e` | PASS (29 scenarios) after the rename |
| Public engine dependency boundary | `go test ./engine -run TestPublicEngineHasNoDesktopDependencies` | PASS. |

## Known limits

- `go test ./engine` has a pre-existing hang in `TestPipelineReceiveRechecksAdmissionAfterProcessing/trust` (reproduced on d96cfaa169). It is not caused by 0022.
- `TestSyncObservabilityResources` needs `UNIC_SYNC_BASELINE_DAEMON` and fails without it (pre-existing).
- The production `uc-web-try` code service is not verified; links work without it.
- The page `push` reply is not supported (the page gets BUSY). No GUI entry.
