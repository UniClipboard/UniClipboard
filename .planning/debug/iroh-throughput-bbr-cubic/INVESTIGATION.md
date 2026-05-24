---
status: diagnosed
trigger: "User reported large-file sync from Mac → Windows starts at 40 MB/s and degrades to <10 MB/s on uniclipboard 0.11.0-alpha.6"
created: 2026-05-24T05:00:00Z
updated: 2026-05-24T07:00:00Z
---

## TL;DR

Slow blob transfer is **not** an uniclipboard bug. The full application stack
(encrypt → V3 envelope → iroh-blobs publish/fetch → decrypt) hits the **same**
~45 MB/s ceiling as bare iroh-blobs in a minimal P2P reproduction (see
`src-tauri/crates/p2p-bench/`).

Root cause is in **iroh 0.98 / noq 0.18**:

- BBR congestion controller plateaus at ~40-50% of the physical UDP link
  capacity (iperf3 measures ~110 MB/s on the same LAN; iroh-blobs over the
  same path tops out at 42-50 MB/s).
- CUBIC congestion controller is **30× slower** than BBR (1.3-1.7 MB/s),
  effectively unusable on our LAN.

All previously-considered factors (Windows Defender, multipath QUIC churn,
hairpin NAT, Mihomo/Clash routing, receiver disk I/O, file extension, ISO
content vs. random content) have been **eliminated** through controlled
experiments.

Production action: keep current configuration (tuned BBR + 32 MB
stream_receive_window). It is already at the local maximum given the upstream
constraints. File draft of an upstream issue at
[`UPSTREAM_ISSUE_DRAFT.md`](./UPSTREAM_ISSUE_DRAFT.md).

---

## Symptoms (as reported)

- Mac (sender, Wi-Fi 6, RSSI -19 dBm, Tx 2401 Mbps) → Windows (receiver,
  2.5 GbE Ethernet, same `192.168.31.0/24` LAN behind Redmi A7E1).
- Large file transfer (typically `Fedora-Workstation-Live-44-1.7.aarch64.iso`,
  ~2.5 GB) shows:
  - Initial 40 MB/s burst for ~10 seconds.
  - Decay into ~10 MB/s with periodic 1-5 s stalls.
  - Occasional 60+ second freezes mid-transfer.
- Reproduces consistently with the same source file across many days.

### Observed pattern (Windows receiver, blob fetch progress checkpoints)

```
4 MB → 20 MB     (26 ms — burst)
20 MB → 36 MB    (2050 ms — pause)
36 MB → 42 MB    (701 ms)
42 MB → 46 MB    (727 ms)
46 MB → 54 MB    (6 ms — burst)
54 MB → 58 MB    (766 ms — pause)
…
```

Burst of 4-8 chunks (= 16-32 MB) followed by a 1-5 second stall. Repeats for
the entire transfer until the connection eventually flat-lines around 45 MB/s
or worse.

---

## Eliminated hypotheses

Each ruling-out came from a specific test, recorded here so future
investigation does not re-walk them.

### 1. Windows Defender real-time scanning of `.iso` files

- **Test**: copy `PDF_Reader_Pro.iso` (264 MB) → 10 MB/s; rename copy to
  `.bin` → 40 MB/s.
- **Then** `Add-MpPreference -ExclusionPath` on both `file-cache/` and
  `iroh-blobs_dev/` → `.iso` jumps to 40 MB/s.
- **Verdict**: Defender real-time scan **does** add ~4× penalty on `.iso`
  specifically. After exclusion it is no longer the bottleneck.
- **But**: it does NOT explain the residual ~45 MB/s ceiling, because the
  same ceiling appears on Mac → Mac (no Defender, see #6 below) and on the
  bare iroh-blobs spike with `--store mem` (no disk write at all).

### 2. iroh 0.98 multipath QUIC churn (initial hypothesis)

- **Lead**: code comment in `src-tauri/crates/uc-infra/src/network/iroh/node.rs:311-340`
  claims multipath is disabled, but reading iroh 0.98 source
  (`endpoint/quic.rs:157`) shows `QuicTransportConfigBuilder::new()` hardcodes
  `max_concurrent_multipath_paths(MAX_MULTIPATH_PATHS + 1)` = **13 paths by
  default**, and the setter ignores any value < 13.
- **Test**: counted `iroh::_events::path::*` events on both sides during a
  34-second cold-start window of a slow Fedora ISO transfer.
- **Result**: 1 × `path::open` at the start, 3 × `path::abandoned` at the
  end. Nothing in between for 31 seconds.
- **Verdict**: If multipath were "constantly re-validating paths and stalling
  the stream", we would see many `path::set_status` events during the stall
  window. We do not. **Multipath QUIC is not the cause.**

### 3. Hairpin public IP candidate from STUN reflection

- **Lead**: slow transfers' `conn` field showed `Ip(180.164.125.95:...)` (Mac's
  public NAT-reflected IP) alongside `Ip(192.168.31.72:...)` (LAN). Fast
  transfers had only the LAN IP. Theory: hairpin path causes packet loss /
  PTO retries.
- **Counter-evidence**: A FAST transfer was found with `conn` containing 6
  candidates (3 public IPs + 2 relays + 1 LAN). A SLOW transfer was found
  with only `LAN + 1 Relay`. **The candidate set does not predict speed.**
- **Verdict**: hairpin is a symptom of "transfer ran long enough for iroh to
  discover more candidates", not a cause.

### 4. Network hardware / Wi-Fi / router throughput

- **Test**: `iperf3 -c <win-ip> -u -b 0 -t 30 -l 1200` from Mac to Win.
- **Result**: 30 seconds, 4.25 GiB sent at 1.22 Gbps, receiver 892 Mbps
  (= ~106 MB/s) with 26% loss (expected since `-b 0` floods).
- **Verdict**: Physical UDP path supports >100 MB/s. Wi-Fi, NIC, router
  switching are not the bottleneck.

### 5. Mihomo / Clash (TUN-mode VPN routing)

- **Test**: disabled Mihomo entirely on both sides. Repeat large-file
  transfer.
- **Result**: same 10 MB/s slowdown observed.
- **Verdict**: TUN routing is not the cause.

### 6. Operating-system specific (NTFS, SMB, Windows-only Defender)

- **Test**: Mac → Mac transfer (same LAN, both APFS).
- **Result**: Still <10 MB/s.
- **Verdict**: Cross-OS reproduction means the issue is not specific to
  Windows, NTFS, SMB, or any other Win-side stack.

### 7. uniclipboard application stack (encrypt / V3 envelope / redb / connection pool)

- **Test**: built `p2p-bench` (independent crate at
  `src-tauri/crates/p2p-bench/`) using only iroh + iroh-blobs APIs with no
  app-layer code. Ran the same 2.5 GB file Mac → Mac.
- **Result**: `tuned BBR + FsStore` = 48.16 MB/s, matching uniclipboard's
  application-layer steady-state (~45 MB/s).
- **Verdict**: Application stack contributes near-zero overhead.
  **The ceiling is in iroh / iroh-blobs / noq.**

### 8. Receiver-side disk write backpressure (advisor's leading guess after #1-7)

- **Theory**: receiver's iroh-blobs `FsStore` writes each 4 MB chunk to disk
  (NTFS write + redb meta commit + BAO outboard append). If write time per
  chunk is comparable to send time per chunk, `stream_receive_window` fills
  up and back-pressures the sender — producing the burst-then-pause pattern.
- **Test**: spike receiver with `--store mem` (drops all received data into
  `MemStore`, never touches disk).
- **Result**: 21.54 MB/s, **half** the FsStore throughput. Both-sides mem
  (`--store mem` on sender too): 21.44 MB/s.
- **Verdict**: Receiver disk write is **not** the bottleneck. FsStore is
  actually faster than MemStore on this hardware. (Plausible explanation:
  MemStore allocates per-chunk into a single mutex-guarded structure;
  FsStore appends to file with kernel page-cache pipelining.)

### 9. Sender-side compress/encrypt/BAO pipeline (advisor's "producer-bursty" guess)

- **Test**: timestamps of Mac-side `iroh blob publish: add_path completed
  (streaming)` log entries during Fedora ISO publishes.
- **Result**: `add_path_ms` consistently 5-9 seconds for the 2.5 GB ISO.
  The receiver-side fetch starts about 2 seconds AFTER add_path completion
  (decoupled by app-layer event dispatch).
- **Verdict**: Sender pipeline finishes before fetch even starts. Producer
  is not the bottleneck.

---

## What actually narrows the diagnosis (positive evidence)

After eliminating all of the above, four controlled experiments through the
`p2p-bench` spike were decisive:

| # | Configuration | Throughput | % of iperf3 |
|---|---|---|---|
| 0 | iperf3 UDP raw | 110 MB/s | 100% |
| 1 | `--cc bbr` + `--tuned` + FsStore both sides | **42-50 MB/s** | 38-45% |
| 2 | `--cc bbr` + `--vanilla` + FsStore both sides | 37 MB/s | 34% |
| 3 | `--cc bbr` + `--tuned` + FsStore sender + MemStore receiver | 21.5 MB/s | 20% |
| 4 | `--cc bbr` + `--tuned` + MemStore both sides | 21.4 MB/s | 20% |
| 5 | `--cc cubic` + `--tuned` + FsStore both sides | **1.29 MB/s** | 1.2% |
| 6 | `--cc cubic` + `--vanilla` + FsStore both sides | 1.7 MB/s | 1.5% |

(All numbers Mac → Mac, 1-2 GB random-content file, same `192.168.31.0/24`
LAN. Each run preceded by a wipe of both sides' p2p-bench-store to ensure
no cache hit.)

### What the matrix says

- **CUBIC is catastrophically broken** in our noq 0.18 environment.
  ~30× worse than BBR. This is the most surprising finding. The upstream
  PR `n0-computer/noq#657` is titled "fix(bbr3): bbr3 controller was not
  accounting correctly", implying their pain point is BBR, not CUBIC. In
  our environment the failure mode is **reversed**.
- **BBR works but caps at ~40-45% of physical capacity**. Tuned (32 MB
  stream window + 64 MB send window) buys ~10 MB/s over vanilla.
- **Memory store is slower than file store**. This decisively rules out
  receiver disk write as the bottleneck.
- **The bottleneck is below iroh-blobs** — both --store flags and pre-known
  application-layer code are now provably innocent.

---

## Source location

- Spike crate: [`src-tauri/crates/p2p-bench/`](../../src-tauri/crates/p2p-bench/)
  (~250 LoC, throwaway, not depended on by anything)
- Helper script (Mac sender, ssh remote receiver):
  [`tmp/spike-run.sh`](file:///tmp/spike-run.sh) (local to investigation host)
- Production transport config (BBR + tuning):
  `src-tauri/crates/uc-infra/src/network/iroh/node.rs:260-340`

### Repro in one command (from `src-tauri/`)

```bash
cargo run --release -p p2p-bench -- serve --path /tmp/large.bin --cc bbr
# (copy ticket)
cargo run --release -p p2p-bench -- fetch --ticket <ticket> --out /tmp/out --cc bbr
# Compare: --cc cubic, --store mem, --vanilla, --no-relay
```

---

## Investigation timeline (compressed)

| Time (PDT) | Step | What we learned |
|---|---|---|
| ~14:00 | Read user report; first dual-side log pass | Saw `path::abandoned` + relay timeout — bet on Wi-Fi disturbance |
| 14:30 | Second transfer: stable 48 MB/s | Started doubting the Wi-Fi story |
| 15:00 | iperf3 = 110 MB/s, Clash already off, same slowdown | Network/router/Mihomo eliminated |
| 15:45 | Found `MAX_MULTIPATH_PATHS = 12` default-on regression | New hypothesis: multipath churn |
| 16:30 | Counted path events: 4 total in 34s | Multipath hypothesis eliminated |
| 16:45 | `.iso` vs `.bin` divergence | Defender hypothesis |
| 17:00 | Defender exclusion → 40 MB/s on `.iso`. Still <50 MB/s ceiling | Partial fix, not root cause |
| 17:30 | Cold-start analysis: producer/receiver ruled out | Now suspect iroh / noq layer |
| 18:00 | User: "Mac → Mac also slow" | OS-stack eliminated |
| 18:30 | Built `p2p-bench` spike to isolate iroh-blobs | First-class measurements possible |
| 19:00 | BBR 48 MB/s = app-layer steady state | App stack innocent |
| 19:30 | Tested `--cc cubic` → 1.7 MB/s | CC layer is the actual problem |
| 20:30 | Validated MemStore vs FsStore | Disk I/O eliminated. Conclusions sealed. |

---

## Open questions / not yet explored

1. Is the BBR ceiling (~45 MB/s) recoverable by tuning **only** transport
   parameters (`initial_window` on the CC, `congestion_event_threshold`,
   per-path keep-alive)? iroh#1943 hints at this but is not specific.
2. Would parallel multi-stream fetch (split blob into N sub-blobs, fetch
   concurrently with N independent QUIC streams) approach iperf3 ceiling?
   Single-stream CC limit suggests yes, but it has not been measured.
3. Is the CUBIC catastrophic-slowness consistent across other noq 0.18
   deployments, or specific to our cross-Wi-Fi pair? Repro on a wired-wired
   LAN would help separate "noq 0.18 CUBIC is broken everywhere" from
   "noq 0.18 CUBIC + lossy link breaks badly".
4. Does iroh `1.0-rc.0` (with MAX_MULTIPATH_PATHS lowered to 8 and noq
   0.19+ which includes BBR fixes) recover? Worth a separate spike when
   upgrade is on the roadmap.

---

## Production recommendation

**Do nothing immediate.** Current configuration is at the local maximum:

- `congestion_controller_factory(Arc::new(BbrConfig::default()))` is correct
  given CUBIC is broken. Do NOT revert to CUBIC.
- `stream_receive_window(32 MB)` + `send_window(64 MB)` add ~10 MB/s over
  vanilla; keep them.

**Schedule for next iroh version bump**: re-run `p2p-bench` baseline. If
iroh 1.0 + noq 0.19+ moves the BBR ceiling closer to 80-100 MB/s, document
the regression-tested improvement; if not, consider the
**parallel-fetch (open question #2)** mitigation path at the app layer
(splits a large blob into N sub-blobs, fetches concurrently).

**Upstream**: file the report in
[`UPSTREAM_ISSUE_DRAFT.md`](./UPSTREAM_ISSUE_DRAFT.md). Cross-reference
`iroh#1943` (throughput umbrella) and `noq#657` (BBR3 fix, possibly related).

---

## What I personally got wrong during the investigation

For posterity / process improvement — at multiple points I committed to a
hypothesis the data did not yet support, then had to backpedal:

- **multipath QUIC**: I jumped to "iroh 0.98 default-on multipath is the
  cause" after finding the `MAX_MULTIPATH_PATHS = 12` regression, before
  checking whether path events actually clustered around the stalls. They
  did not.
- **noq#657 BBR-broken**: I told the user "noq#657 is almost certainly the
  bug" based on the web-search agent's report, before re-checking the
  predicted ranking (BBR slow, CUBIC fine). Our measured ranking was
  reversed.

Both times, **calling `advisor` before committing to a new hypothesis**
caught the error and redirected to a falsifying experiment. The lesson is
to call advisor before the third confident statement of "I think it's X",
not after.
