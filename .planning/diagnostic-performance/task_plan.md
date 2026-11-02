# Diagnostic performance implementation

Owner: Engineer. Desktop base: 81a11286d281a54625516a7d9dbf8c48b90fed09.
Engine base: d532978d8bc9740a64460ee08b6e97a85fe4b590.

## Phases
1. [complete] Verify current source and define export acceptance before product changes.
2. [complete] Engine-owned bounded process metrics and local OTLP history, encrypted through existing key lifecycle.
3. [complete] Desktop online and offline ZIP contract, pinned Collector import.
4. [complete] Native process coverage after daemon vertical slice.
5. [complete] Run real export/import/query acceptance and save reproducible evidence.

## Acceptance and failure inventory
User exports diagnostic ZIP; import it through pinned Collector; query original metrics/logs/complete trace.
Verify CPU warming-up, suspension gap, PID reuse, memory bytes, bounded history, encryption/lock,
missing signals, partial export, Collector rejection/duplicate outcomes, no sensitive fields.
Run macOS locally; report Windows/Linux native NOT_RUN unless actually executed.
Never label daemon metrics whole-application metrics. Do not add post-implementation unit tests.

## Remaining acceptance boundaries
Windows/Linux native, attributable system WebView, forced same-PID reuse, real sleep/clock changes, full retention saturation and A/B overhead remain NOT_RUN. See EXECUTION_REPORT.md.
