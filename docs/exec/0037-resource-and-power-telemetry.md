# Execution Plan: 0037 Resource and Mobile Power Telemetry

- Status: Implemented (engineering acceptance `PASS` across Go, Desktop, CLI, Expo, isolated Android host cutoff suites, and offline Linux ARM64 container `unicd` first ~60s export; Linux native Desktop GUI, Windows runtime acceptance, and physical device power benchmarks `RV6`/`RV7` remain `NOT_RUN`).
- Decision: [0037 · Resource and Mobile Power Telemetry](../decisions/proposed/0037-resource-and-power-telemetry.md)
- Contracts: [Observability Contract §14](../contracts/observability-contract.md#14-host-process-resource-and-mobile-power-telemetry), [Local Service Contract](../contracts/local-service-contract.md)

## 1. Local OTLP Verification & Replay

### 1.1 CLI & Service Resource Telemetry (`.cache/0037/cli-acceptance/`)

Run the isolated real-process suite against a local TLS OTLP receiver (never production PostHog):

```sh
mkdir -p .cache/0037/bin
go build -race -tags unic_e2e -o .cache/0037/bin/unicd ./app/service/cmd/unicd
go build -race -o .cache/0037/bin/unic ./app/service/cmd/unic
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN="$(pwd)/.cache/0037/bin" UNIC_E2E_ARTIFACTS="$(pwd)/.cache/0037/cli-acceptance" \
  go test -race ./e2e/cli -run '^(TestResourceObservability.*|TestObservability.*|TestPostHog.*)$' -skip '^TestPostHogTemporaryRestarts$' -count=1 -v
```

> CLI replay scope: `TestResourceObservability` (including `service-self-sampling`, `desktop-route-validation-and-bounds`, `disable-barrier-and-cli-temporary-exclusion`, and `privacy-and-summary-artifact`), `TestObservability.*`, and the remaining `TestPostHog*` cases (`TestPostHog`, `TestPostHogPairingAndSend`, `TestPostHogResources`, `TestPostHogTransport`, `TestPostHogReleaseAssembly`, `TestPostHogEnableFromDisabled`, `TestPostHogDesktopReconnect`) passed; `TestPostHogTemporaryRestarts` has a documented timeout reproduced on unchanged baseline `a3cb68a49`.

### 1.2 Desktop Role Telemetry (`resource-telemetry` & `production-boundary`)

```sh
python3 app/desktop/e2e/run.py catalog --check
TMPDIR=/private/tmp make desktop-e2e CASE=resource-telemetry
TMPDIR=/private/tmp make desktop-e2e CASE=production-boundary
```

### 1.3 Mobile Binding & Expo Verification

```sh
(cd bindings && GOWORK=off go test -race ./mobile/...)
(cd bindings && GOWORK=off UNIC_MOBILE_DIAG_ARTIFACTS="$(pwd)/../.cache/0037/mobile-diagnostics" go run ./e2e/mobile-diagnostics)
(cd app/expo && npm run type-check && npm run verify)
```

## 2. Physical Device Power Benchmark Protocol

> Physical device tests (V7 Android, V8 iOS) must run on physical hardware with an isolated profile pointing to a local LAN TLS OTLP collector (or with uploads disabled when isolating sampling overhead). Never report results from simulators or emulators as physical power acceptance.

### 2.1 Android Physical Device Protocol (`.cache/0037/android-power-bench/`)

Prerequisites: physical Android device, wireless ADB (`adb tcpip` / Wi-Fi debugging), physically unplugged from all AC/USB/wireless power (`dumpsys battery` confirms `AC/USB/Wireless powered: false`) with charge level `< 95%`, granted required monitor permissions (`overlay`, notification, `READ_LOGS`, Shizuku), and isolated profile pointing to a LAN TLS OTLP collector. Any failure to meet unplugged preconditions keeps this benchmark strictly `NOT_RUN`.

1. Run 4 standardized 15–30 minute unplugged scenarios:
   - Scenario A: Telemetry disabled vs. Telemetry enabled (60s OTLP export) to measure net telemetry wakeup/radio overhead.
   - Scenario B: `fgs_idle` (Foreground Service active, no overlay/shizuku) — screen-on and screen-off windows.
   - Scenario C: `overlay_polling` (1 Hz focus probe) — screen-on and screen-off windows.
   - Scenario D: `overlay_event` / `shizuku` — screen-on and screen-off windows.
2. Before each run (authorized dedicated lab/test device only, since global battery stats are not isolated per profile; when resetting global `batterystats` is outside device authorization, record before/after counter deltas instead): `adb shell dumpsys batterystats --reset`.
3. After each run: capture `adb shell dumpsys batterystats --checkin`, `adb shell dumpsys meminfo <pkg>`, `adb shell dumpsys cpuinfo`, and the decoded local OTLP `otlp.decoded.jsonl`.
4. Correlate computed mAh drain, wakeup counts, Wi-Fi MulticastLock residency, and PSS against `unic.resource.cpu.time`, `unic.power.bg_mode.time`, `unic.power.network.bytes`, `unic.power.context_switches`, and `unic.power.system_battery.drain_rate`.

### 2.2 iOS Physical Device Protocol (`.cache/0037/ios-power-bench/`)

Prerequisites: physical iPhone connected over wireless/tunnel debugging, Developer Mode enabled, **an unused non-production bundle identifier configured with a dedicated isolated App Group + Keyboard Extension target and matching Apple Development signing** (never reuse or overwrite an already-installed profile/app or rely on an App-Group-less wildcard profile), unplugged during measurement (device battery state verified on hardware rather than assumed), host LAN TLS OTLP collector reachable and trusted by the phone, and Xcode Instruments (`Power Profiler` on this host's Xcode toolchain, or `Energy Log` where available) — any missing signing isolation, battery, or connectivity precondition keeps `RV7` strictly `HOLD` / `NOT_RUN`.

1. Main App foreground + 25s background drain release: record `xcrun xctrace record --template 'Power Profiler'` (or `'Energy Log'` if provided by the active Xcode version) and Memory Gauge comparing telemetry disabled vs. enabled (verifying CPU, `phys_footprint`, thermal residency, and `battery_probe_status="unsupported"` per Decision A1).
2. Keyboard Extension (`ios_keyboard`): 10 consecutive activate/sync/dismiss pulses; record peak `phys_footprint` vs. Jetsam headroom.
3. MetricKit verification: trigger Xcode `Debug > Simulate MetricKit Payloads` and verify decoded `unic.power.ios_metrickit.*` metrics reach the local TLS OTLP receiver across the B1 durable admission boundary.
4. Retain device category metadata, `.trace` summary, `otlp.decoded.jsonl`, and `SHA256SUMS` inside the repo-local bench directory (without writing personal device identifiers or temporary machine paths into tracked docs).

## 3. Final Engineering Acceptance & Scope Separation

Independent Gate 3 verification passed for engineering layers (`L1`–`L3`), while physical power measurement (`L4`: `RV6`/`RV7`) and non-macOS Desktop runtimes remain `NOT_RUN`.

### 3.1 Scope Distinction

| Category | Measured scope & contract semantics | Acceptance status |
|---|---|---|
| Application process resources | Per-process CPU (`unic.resource.cpu.time`, `unic.resource.cpu.utilization`), active window time, OS memory footprint (`phys_footprint` / `totalPss` / `smaps_rollup`), Go heap, macOS wakeups, Linux/Android voluntary context switches, Desktop socket sampling (`C1` discard), and Android aggregate UID `TrafficStats` / `bg_mode.time` | Engineering `PASS` (process E2E, Go `-race`, isolated Android Kotlin suites, and offline Linux ARM64 container `unicd` first ~60s CPU/memory/context-switches export); Linux native Desktop GUI and Windows runtime remain `NOT_RUN` |
| Android system-wide discharge | System-wide `unic.power.system_battery.drain_rate` (`{percent}/h`, strictly >=10 min continuous unplugged monitoring; cutoff-proven completion times across battery/LPM/state/thermal getters; crossed `T_cut` thermal read omitted without synthetic probe failure) | Engineering `PASS` on controlled host fixtures (`.cache/0037/android-rv3-replan-r7/replay.sh`); physical unplugged `RV6` `NOT_RUN` |
| iOS MetricKit & A1 battery boundary | Main app (`mobile_app`) retains CPU, memory (`phys_footprint`), `power_source`, thermal residency (`unic.power.thermal.time`), and 24h `unic.power.ios_metrickit.*` summaries (`B1` generation + `pending`/`seen` atomic admission); keyboard extension (`ios_keyboard`) collects CPU and memory footprint (`phys_footprint`) with `power_source="unknown"` and `wakeups_probe_status="unsupported"` (no thermal or MetricKit fields). Neither iOS role emits `system_battery.drain_rate` (`battery_probe_status="unsupported"` per `A1`) | Engineering `PASS` (temp iOS simulator/native build + Go binding & MetricKit loopback TLS assertions); physical `RV7` `HOLD` / `NOT_RUN` |
| External physical benchmarks | Unplugged real-device battery correlation, Xcode `Power Profiler` traces, and live phone-to-LAN TLS OTLP delivery | `NOT_RUN` (`RV6`/`RV7` tracked as open gaps alongside Windows/Linux Desktop GUI runtime gaps; `0037` stays in `docs/decisions/proposed/` until closed or split) |

### 3.2 Android Cutoff & APK Provenance (`.cache/0037/android-rv3-replan-r7/`)

- **Local replay & API-level separation**:
  - **API 35 (`Build.VERSION.SDK_INT = 35`, 22 suites, exit `0`)**: `bash .cache/0037/android-rv3-replan-r7/replay.sh` and `(cd .cache/0037/android-rv3-replan-r7 && shasum -a 256 -c SHA256SUMS)` verify PV1–PV3 paired reorderings, battery/thermal cutoff completion boundaries (`T_cut=60000ms`), missing-`PowerManager` thermal `unsupported` (`rv3-edges`), and MV1–MV3 regressions against `AndroidResourcePowerCollector.kt` (SHA-256 `86838b7965895f63b61ea8669f53777e1888000cad54a01afbe63d0f94ea928c`, 48-file working-tree source manifest SHA-256 `5b8971c474f8f5e82caa53a1d39995e847476d9426b8af39e3d09c61e1b986ba`).
  - **API 28 (`SDK_INT < Q`, independent control)**: verified separately outside the 22-entry API 35 script (`0` thermal getter calls returning `thermal_probe_status="unsupported"` in the independent Gate 3 `run-thermal-unavailable.sh` control).
- **Verified `arm64-v8a` Release APK identity**: `app-release.apk` SHA-256 `59cb92a4e6585c232032884c10f9d3f6241b7d5bc71775f78c28ce4ee240da16`; `classes3.dex` SHA-256 `15aa9bdaa68bd9ffc383aa19c80846bb02ffb75f9eb705863bda42081895227c`; APK Signature Scheme v2 `Verifies` using the local debug signing certificate (`SHA-256: fac61745dc0903786fb9ede62a962b399f7348f0bb6f899b8332667591033b9c`), with provenance logs retained under `.cache/0037/android-rv3-replan-r7/after/`.
- **Other current host & container checks**: offline Linux ARM64 container production `unicd` first ~60s export (`59.998s` window emitting CPU utilization, OS memory footprint, Go heap, and voluntary context switches) alongside macOS production `unicd` (`59.985s`), `(cd bindings && go test -race -count=1 ./mobile/... ./e2e/mobile-diagnostics/...)` (`ok`), `(cd app/expo && npm run type-check && npm run verify)` (`passed`), `go run ./tools/docscheck` (`ok`, retaining baseline warnings + uncommitted `observability.md` `verified` lag), and `git diff --check` (`0`).

### 3.3 Remaining `NOT_RUN` Hardware & Runtime Gaps

- **RV6 (Android physical power benchmark)**: `NOT_RUN`. Connected physical Android device (over USB ADB transport) reported `AC powered: true`, `USB powered: false`, and `100%` battery charge (remaining target is an emulator). Must run later unplugged from all external power (`AC/USB/Wireless powered: false`) with `< 95%` charge over wireless ADB.
- **RV7 (iOS physical power benchmark)**: `HOLD` / `NOT_RUN`. Xcode host provides `Power Profiler` (no `Energy Log` template), on-device unplugged battery state was unconfirmed in the latest wireless session, and both existing non-production App-Group bundle IDs were already occupied on device while available wildcard profiles lacked App Group entitlements. Requires an unused isolated test bundle identity + App Group + matching developer profile and confirmed phone-to-LAN TLS trust.
- **Windows runtime & Linux native Desktop GUI**: `NOT_RUN` on this macOS host (`CGO_ENABLED=0` cross-compilation verified only; offline Linux ARM64 container production `unicd` runtime passed as noted above, while Linux native Desktop GUI and all Windows runtime probes/sockets/sleep-jump behavior remain unexecuted).
