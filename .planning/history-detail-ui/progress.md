# Progress

- Read frontend rules, systematic-debugging and React Doctor workflows.
- Original video reviewed in earlier turn; now preparing reproducible automated acceptance.

- Baseline browser run completed for linux/macos/windows × light/dark: 18 failures (all three regressions in all six combinations), 12 ancillary checks passed.
- Evidence: browser runner output in external acceptance artifact directory; screenshots and per-check JSON captured.
- Fixed the acceptance harness to use Chinese locale and only scroll user-scrollable ancestors (programmatically scrolling overflow:hidden would falsely pass clipping).

- Initial fix: all 30 original browser checks passed across all skins/themes; 72 targeted unit tests passed.
- Additional warm-image frame sampling reproduced a remaining asynchronous object-URL cache flash on Linux light/dark; added synchronous cache reads and included this check plus selection scroll reset in the final matrix.
- Typecheck initially caught a removed type import; restored it before final validation.

- Final acceptance: 42/42 browser checks passed on linux/macos/windows × light/dark, using production components and deterministic intercepted HTTP delays in Chromium. This does not claim native WKWebView/WebView2/WebKitGTK execution.
- Revisited text never paints loading/missing/stale delivery; cached images always mount their existing URL without replacing the image with a placeholder, and finish decoded. Frame callbacks occur before painting, so an img.complete=false inside an early callback alone is not treated as a painted loading placeholder.
- All 72 relevant unit tests passed; TypeScript, oxlint, formatting and git diff checks passed. React Doctor: 93/100, no findings.
- Before/after evidence and screenshots: target/e2e/history-detail-regressions/before and target/e2e/history-detail-regressions/after (ignored build artifacts).

## Re-run

```bash
node apps/gui-go/frontend/e2e/history-detail-regressions.mjs
```

Requires Playwright and Chromium; external installations can be selected with PLAYWRIGHT_MODULE and PLAYWRIGHT_EXECUTABLE_PATH. UI_KIT_PLATFORMS can restrict the skin matrix.
