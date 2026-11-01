# TesterArmy E2E migration

Date: 2026-10-31
Status: Implementation in progress

## Recommendation

Use the published `e2e@0.22.0` runner with `@e2e-dev/web@0.15.4` for browser UI checks. Keep all native acceptance scripts until an equivalent native replacement has passed. Start with the two named relay entry points. Use deterministic SDK locators and assertions; no model, subscription, or remote browser is required.

## Verified source and constraints

- Official repository: https://github.com/tester-army/e2e/tree/caf51eb9f8de765b1ecac5ce3cd511a925b2fc2f
- Apache-2.0: `LICENSE`, `NOTICE`, and both package manifests.
- `packages/web/src/browser-connection.ts`: launches Chromium/Firefox/WebKit through Playwright 1.64.1; CDP attachment uses Chromium only.
- `packages/mobile/src/options.ts`: mobile platforms are iOS and Android. There is no supplied desktop accessibility engine.
- `docs/reference/web.mdx`: remote connection normally creates a new context. Persistent connection requires a fresh browser per attempt, default context identity and page target identity. A WebView2 debugging endpoint alone does not prove compatibility.
- macOS WKWebView and Linux WebKitGTK are not the Playwright WebKit browser. Do not substitute browser screenshots for native acceptance.
- Node must satisfy `^22.22.3 || >=24.8.0`. Use Node 24 in CI.
- Disable telemetry with `E2E_TELEMETRY_DISABLED=1`. Do not use model steps or cloud providers with application data.
- Browser fixtures use synthetic data and isolated contexts. No system clipboard, production profile, desktop input, or native process is needed.
- Native runs must use the existing unique `gui-go-*` profile, isolated HOME/portable root and disabled system clipboard. Do not run real-keyboard scenarios on this shared host.

## Alternatives

Reject a new custom desktop engine for this first migration. It would duplicate the existing native automation and require a separate cross-platform driver and validation contract. Reject wrapping Python/native scripts in a passing runner test: that does not establish engine support.

## Coverage and removal

- Inventory: task library `existing-e2e-inventory.json` records paths, platform/driver/artifact signals and file hashes. These signals are discovery aids, not acceptance results.
- Exact initial Desktop HEAD and remote main: `7ae0a059c9e2c3cf2dd753240da9ef65f4343d40`.
- Parallel changes: task library `parallel-worktree-changes.json`. Do not edit other worktrees.
- Preserve old relay tests until replacements pass all existing assertions. Preserve remaining browser and native tests with explicit migration status.

## Checklist

- [x] Read official source, docs and license; record source identity.
- [x] Inventory existing scripts, workflows and recent parallel worktrees.
- [x] Pass minimal browser UI chain with runner-managed browser/server.
- [x] Port named relay scenarios without dropping assertions.
- [x] Save reports, screenshots, commands and checksums.
- [x] Add CI and document remaining native/platform gaps.

## Verified first batch

The two legacy relay scripts and both TesterArmy replacements passed on macOS. The real-daemon browser test passed including UI pinning, API read-back, reload and process cleanup. Remaining browser scripts are retained in `migration.json`; the overall migration is not complete.

## Expanded migration

The public `surfaceOf()` API was validated with runner-owned pages and contexts. It preserves raw Playwright assertion semantics where the portable SDK has no direct equivalent. Raw operations are explicitly out of band: keep scenario JSON and screenshots rather than claiming SDK step tracing. The catalog now contains 21 original assertion groups; 15 groups have 29 verified suite/skin combinations. Six groups and three partial platform gaps retain their original failure paths. Seeded real-daemon suites remain explicitly unported, because the minimal fixture is not equivalent. Native runtime probes and an identity-checked WebView2 characterization run separately from browser acceptance.

## Remaining backend migration: failure model and implementation order

Before changing the fixture, enumerate failures: wrong profile/HOME or system keychain access;
clipboard writes; session/token disclosure through URLs, traces or files; leaked daemon/listener;
wrong seed count or content classification; missing phone source identity; stale search index;
expired or rate-limited sessions; recreated browser contexts losing authentication; component-only
results confused with full-page results; synthetic lifecycle routes confused with real key changes;
WebDriver keyboard translations changing modifier semantics; cleanup replacing the primary failure.

Use the existing CLI isolation owner and real HTTP/mobile/file ingestion. Transfer bootstrap
credentials only over pipes and install sessions before navigation. The framework owns the browser
and server; each backend attempt owns a fresh daemon and removes its HOME. First migrate tag layout,
then the Playwright history/devices/panel bodies, then translate the two WebDriver bodies. Retain
original entry points until their replacement assertions actually pass. Run empty and seeded History
separately. Record each source snapshot and repeat CI after commits. Existing native paths stay intact.
