# BoardUI replay harness

Status: final integrated software-view/reference replay PASS. A successful script build or catalogue check is not a visual or native-window acceptance result. See the [implementation receipt](boardui-mygo-implementation.md) for actual run results.

## Canonical view scenarios

The independent BoardUI package registers two scenarios with the existing Desktop E2E runner:

```sh
make desktop-e2e CASE=boardui-settings
make desktop-e2e CASE=boardui-states
```

`boardui-settings` drives the complete settings operation chain. `boardui-states` drives the source-derived component/state matrix and narrow reduced-motion view. Both are `view` cases. The runner freezes source, executes the Go scenario registry with race instrumentation where supported, requires exercised PNG checkpoints and records individual assertions. Source and private test data cleanup follow the existing [Desktop E2E contract](../contracts/desktop-e2e-contract.md).

These scenarios create no native window. They do not establish physical keyboard delivery, native input-method integration, OS accessibility bridges, GPU composition, Windows resident memory or system clipboard behavior.

## Combined reference replay

Run from the repository root:

```sh
python3 app/desktop/boardui/e2e/replay.py --node /absolute/path/to/node
```

The script requires Node >=22.12 only for the real official React reference tooling. It calls the reference preparation script, runs Vite on the loopback address with a strict port, delegates browser capture to the reference capture script, runs both canonical software-view cases and calls the visual comparison script. No JS runtime is linked into the Go component package.

The default output is a unique directory under ignored `.build/boardui-e2e/`. `--out` accepts an explicit new artifact directory. An existing directory is rejected before any scenario runs. The script performs no retries. First failure evidence stays in its original run directory. Use the repository failure policy before launching any retry: preserve and diagnose the first failure, bound additional attempts and do not claim PASS from a later successful attempt alone.

A reference-only or software-only run is not a complete visual comparison. `--native-only` permits bounded software-view development while explicitly recording reference and visual comparison as NOT_RUN. The full replay fails if required visual comparison does not run.

The replay owns only its temporary Vite process group. It terminates and waits for that group on success, failure and interruption. It never opens a product profile, personal web account or system clipboard.

## Evidence and metrics

`replay.json` records source HEAD, dirty state, per-source hashes, Go/OS/Python/Node identities, font file hashes, executed commands, per-step duration, process resource scope, visual comparison status and source stability. Each canonical child run retains its standard manifest, report, screenshots and checksums. A top-level `SHA256SUMS` binds the combined artifacts.

The optional POSIX `ps` sampler retains only descendant BoardUI `TestDesktopAcceptance` test-process metrics. It excludes compiler processes, the outer testscript process, reference browser and production roles. It records sampled RSS, cumulative CPU time, sample count and peak sampled RSS. A process that ends before sampling is recorded NOT_RUN, not zero. Sampling misses peaks between samples. Race instrumentation adds overhead. These numbers are a software-E2E resource baseline; they are not production GUI or Windows memory claims.

The Go view scenario separately records first-frame wall time, open/close event/render-chain duration, process RSS and Go heap state where implemented. Compare only runs with the same source, font, viewport, scale, renderer and instrumentation. Do not compare software-view RSS with an incomplete browser process tree. No explicit GC or working-set trimming is used to manufacture a lower figure.

## Source changes and offline replay

The replay hashes the BoardUI package and relevant harness/module inputs before execution and checks them afterward. Concurrent edits fail the combined run. Wait for implementation work to stop before measuring.

The reference preparation script owns the pinned official source and exact npm lock. The capture script owns the isolated browser task space and real React interaction protocol. Offline reference replay requires the approved source/font/icon/dependency cache to exist; missing fixtures must fail rather than silently using a lookalike. Cache/download contents stay ignored. Public reports retain only legal synthetic evidence and minimal receipts.

## Verification performed by the harness task

- Python syntax compilation: PASS.
- Canonical catalogue generation and exact catalogue check: PASS.
- `boardui-settings` catalogue discovery: PASS.
- Full reference/native software-view replay: PASS in replay-final-02; 13 paired scenes. Native-window acceptance remains NOT_RUN.
