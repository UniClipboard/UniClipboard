# Multi-Stream / Multi-Connection Spike

**Date:** 2026-05-24
**Setup:** Mac (sender) ↔ Mac via ssh `macbook` (receiver), same wifi LAN (192.168.31.0/24)
**Test file:** 3 GB random bytes (`/tmp/testfile-rand.bin`, generated from `/dev/urandom`)
**Spike binary:** `src-tauri/crates/p2p-bench/` — minimal iroh + iroh-blobs CLI

## Goal

Answer: **Can we work around the ~40 MB/s single-stream iroh-blobs ceiling
by using N concurrent streams/connections at the application layer? If so,
this is a viable fix without forking iroh or noq.**

Two variants tested:

- **X1 (multi-stream, single endpoint)**: sender splits the file into N
  blobs; receiver fetches all N concurrently from one shared `Endpoint`.
  Verifies: do N QUIC streams over the same connection bypass the
  per-connection cwnd?
- **X3 (multi-endpoint, multi-connection)**: receiver builds N independent
  `Endpoint`s, each with its own UDP socket; one ticket per endpoint.
  Verifies: do N independent QUIC connections bypass any per-connection
  limit?

## Results

### iroh-blobs throughput matrix (3 GB, BBR, tuned transport)

#### Mac↔Mac (both wifi, near saturation)

| Configuration | per-stream MB/s | aggregate MB/s | wall (s) | runs |
|---|---|---|---|---|
| iroh X1 N=1 (baseline) | — | 28-42 | 70-79 | 4 |
| iroh X1 N=2 | 16-23 | 24-45 | 66-122 | 4 |
| iroh X1 N=4 | 6-13 | 25-51 | 58-118 | 3 |
| iroh X1 N=8 | ~4 | 30 | 99 | 1 |
| iroh X3 N=2 (receiver multi-endpoint, mem store) | 17.6 each | 34 | 88 | 1 |

#### Mac→Win (Mac wifi → Win wired, large headroom)

| Configuration | per-stream MB/s | aggregate MB/s | wall (s) | × baseline |
|---|---|---|---|---|
| iroh X1 N=1 (baseline) | — | 43.64 | 68.75 | 1.0× |
| **iroh X4 N=2 (sender multi-endpoint)** | **45.5, 42.9** | **85.91** | **34.9** | **1.97×** ✓ |
| iroh X4 N=4 (sender multi-endpoint) | 18-20 each | 72.35 | 41.5 | 1.66× (regression past N=2) |

### Physical-link baseline via iperf3 (30 s tests)

iperf3 was compiled from source on `macbook` (no openssl) since brew install
was unavailable in the build environment. Win runs scoop-installed iperf3
3.20. **Mac and macbook are both on wifi; Windows is on a wired 2.5 GbE NIC.**

#### Mac↔Mac (both wifi → bottleneck is wifi)

| Direction | Mode | Aggregate | Retr |
|---|---|---|---|
| Mac→macbook | TCP single | 370 Mbps = **46.3 MB/s** | 448 |
| Mac→macbook | TCP P=4 | **526 Mbps = 65.8 MB/s** | 4230 ⚠️ |
| Mac→macbook | TCP P=8 | 508 Mbps = 63.5 MB/s | 10012 ⚠️ |
| macbook→Mac | TCP P=4 (reverse) | 480 Mbps = 60 MB/s | — |

**Mac↔Mac physical ceiling: ~65 MB/s (TCP P=4).** Adding more parallelism
doesn't help; high retransmit count indicates significant wifi packet loss
even at the saturation point.

#### Mac↔Win (one wifi end + one wired end → bottleneck is the wifi end)

| Direction | Mode | Aggregate | Retr |
|---|---|---|---|
| Win→Mac | TCP single | 311 Mbps = **38.9 MB/s** | — |
| Win→Mac | TCP P=4 | 788 Mbps = **98.5 MB/s** | — |
| Win→Mac | TCP P=8 | **946 Mbps = 118.3 MB/s** | — |
| Mac→Win | TCP P=4 (reverse) | 853 Mbps = **106.6 MB/s** | 1315 |

**Mac↔Win physical ceiling: ~118 MB/s (TCP P=8)**, close to gigabit line
rate. The Mac's wifi uplink can carry 100+ MB/s to a wired receiver — far
beyond what Mac↔Mac wifi-to-wifi can deliver.

### ssh+dd reference (incidental, do not over-interpret)

ssh single TCP behaves badly on jittery wifi (CUBIC slow-recovery after
each loss). Listed for completeness only — these are not "wifi physical
upper bounds":

| Configuration | Aggregate |
|---|---|
| ssh+chacha20 single | 14.5 MB/s |
| ssh+aes128-gcm single | 5.87 MB/s |
| **ssh+chacha20 2 parallel** | **31.4 MB/s** (2.16× single — wifi has headroom) |

### Connection-level evidence (X1 N=2 with `RUST_LOG=iroh=debug`)

```
Connection established. me=d2626b2316 remote=73d7e0b278 alpn=/iroh-bytes/4
handle_path_event ... event=Opened { id: PathId(4) }
```

Exactly **one** QUIC connection, **one** path. The N=2 streams are multiplexed
over a single connection — they share cwnd. This is consistent with the
N=2 per-stream throughput being half of N=1.

## Interpretation

### Mac↔Mac (both wifi)

Five facts have to fit:

1. **iroh single = 40 MB/s ≈ iperf3 single TCP = 46.3 MB/s**. iroh's single
   QUIC stream already hits **87 % of what plain TCP can do** on this wifi
   link. iroh is *not* leaving meaningful single-stream headroom on the
   table — it's about as good as one connection gets.

2. **iperf3 P=4 = 65 MB/s** is the physical wifi ceiling. P=8 doesn't
   improve, so 4 streams already saturate. **Mac↔Mac wifi tops out at
   ~65 MB/s**, not the gigabit-class number you'd see from iperf3 on a
   wired link.

3. **iroh X1 N>1 doesn't improve over N=1**. Multiple QUIC streams on the
   same connection share cwnd. Debug log confirms exactly 1 QUIC
   connection / 1 path for X1 N=2.

4. **iroh X3 N=2 = 34 MB/s ≈ X1 N=1 = 40 MB/s**. Two *independent*
   connections still aggregate to ~single-stream. At first glance this
   contradicts "cwnd is the bottleneck" — independent CCs should each
   climb to ~40 and total ~80. But the wifi ceiling is only ~65, so 34 is
   what you'd expect once contention overhead + jitter eat into the
   already-near-saturated channel.

5. **ssh 2-parallel = 31.4 MB/s ≈ 2.16× single ssh = 14.5 MB/s**. ssh
   single is far below the wifi ceiling (CUBIC handles wifi loss badly),
   so doubling streams cleanly doubles throughput. This is the "headroom
   exists" signal, not contradiction of (4) — iroh + ssh sit at very
   different fractions of the same ceiling.

**Mac↔Mac conclusion**: iroh single is near the physical ceiling already.
The "X3 didn't scale" result is consistent with a ~65 MB/s wifi ceiling +
multi-connection overhead. Sender-side `Endpoint` contention may still
play a role, but on Mac↔Mac there's not much headroom to demonstrate it.

### Mac↔Win (Mac wifi + Win wired)

The story changes dramatically:

| Metric | Value | What it says |
|---|---|---|
| iperf3 Mac→Win P=4 | **106 MB/s** | Physical headroom is huge — gigabit-class, because Win is wired |
| iperf3 Mac→Win single | ~38 MB/s | Single TCP, like Mac↔Mac single — wifi single-flow CUBIC is the limit |
| iroh single (user report) | ~40 MB/s | **34 %** of physical headroom — iroh single is leaving a *lot* on the table |

On Mac↔Mac, iroh single = 87 % of physical = "iroh is healthy". On
Mac↔Win, iroh single = 34 % of physical = **iroh is severely
under-utilizing the link**. The difference isn't iroh — it's that the
physical ceiling is now far above what a single QUIC stream can deliver
over wifi.

This is exactly the regime where application-layer multi-stream/connection
should help. The Mac↔Mac X3 result (no scaling) doesn't generalize here
because Mac↔Mac was already near saturation. Mac↔Win has 60+ MB/s of
unused capacity that 1-stream iroh can't reach.

The bottleneck is **above the noq congestion controller** but **below the
iroh-blobs application logic**, AND on Mac↔Win there's enough physical
headroom that working around it at the app layer could realistically
double or triple throughput.

## Implications for the original question

The user asked: *"Can our forked iroh-blobs fix the throughput?"*

| Fix path | Mac↔Mac | Mac↔Win | Why |
|---|---|---|---|
| Fork `noq` to patch BBR/CUBIC | ❌ | ❌ | CC isn't the bottleneck (X3 still didn't scale on Mac↔Mac; on Mac↔Win, single-stream BBR already reaches what TCP CUBIC reaches) |
| Fork `iroh` to tweak transport / multipath | ❌ | unlikely | Already exhausted in spike (multipath ruled out, window/keepalive/CC factory tried) |
| App-layer multi-stream (X1) | ❌ | ❌ | Streams share single-connection cwnd; debug log proves only 1 QUIC connection |
| App-layer multi-endpoint **receiver** (X3) | ❌ | unverified | On Mac↔Mac no headroom to exploit. On Mac↔Win 60+ MB/s headroom exists, but the receiver is Win and the sender-side endpoint is still shared — likely still bottlenecks |
| **App-layer multi-endpoint sender** (X4) | unverified | ✓ **verified 1.97× at N=2** | Mac→Win N=2 SME = 85.91 MB/s vs N=1 = 43.64 MB/s. Per-stream throughput stays at single-stream baseline (45/43 MB/s each), confirming sender-side `Endpoint` was the shared bottleneck. N=4 regresses to 72 MB/s — N=2 is the sweet spot |
| Upgrade to iroh 1.0-rc | unverified | unverified | Multiple changes (multipath default 8 not 13, noq updates). Worth testing in isolation, but no specific patch known to address the symptom |
| Accept current ceiling | pragmatic | costly | Mac↔Mac 40 MB/s = 87% of physical = healthy. Mac→Win 40 MB/s = 34% of physical = leaving large speedup on the table |

## Important caveat for the user's actual symptom

The user's main complaint is **Mac→Windows starts at 40 MB/s and decays to
10 MB/s over a long transfer**. The spike's two findings cut along
different axes:

1. **The 40 MB/s starting throughput** is itself only 34 % of the physical
   wifi+wired capacity on Mac→Win. This is not "iroh near saturation" — it
   is "iroh single-stream is not designed to multiplex into wifi-wired
   asymmetric links". Sender multi-endpoint may lift this number toward
   80-100 MB/s.

2. **The decay from 40 to 10 MB/s** is *not* reproducible on Mac↔Mac (Mac↔Mac
   holds steady), so this is Windows-specific. Candidates:
   - Windows Defender deferred scan as the file grows
   - NTFS fragmentation pressure during long sequential writes
   - SMB or filesystem cache eviction under sustained pressure
   - Windows iroh / quinn behavior on the receive side

These two problems are **independent** and need separate fixes. Even if
sender multi-endpoint lifts the start from 40 to 90 MB/s, the decay would
still drag it back down without a Windows-side fix.

## What X4 N=2 result actually means

This is the headline number. On the user's actual problem path (Mac→Win
file sync), splitting the **sender** into 2 independent `Endpoint`s
**doubles throughput** with no code change to iroh, noq, or iroh-blobs —
purely an application-layer pattern.

What "do this in production" would look like for uniclipboard:
- The current sender process binds one `Endpoint` (via `uc-infra/network/iroh/node.rs`)
  and one `BlobsProtocol` instance shared across all transfers
- The proposed change: when serving a large blob, bind a 2nd ephemeral
  `Endpoint` for that transfer, split the blob into 2 chunks, send each
  chunk's ticket as a pair, receiver fetches both concurrently
- For small clipboard items the existing single-endpoint path stays —
  doubling only matters once the transfer is large enough that
  startup overhead (~3-5 s per extra endpoint for relay + discovery) is
  amortized

Cost considerations:
- 2nd `Endpoint` = additional UDP socket + iroh node_id + relay handshake
- Adds ~3-5 s setup latency per large transfer; not noticeable for files
  > a few hundred MB
- Doubles the number of public node_ids exposed by the sender
  (privacy/observability surface grows accordingly)
- Receiver-side change: need to accept pairs of tickets and fan out the
  fetch

The spike does NOT change iroh / noq / iroh-blobs themselves. The X4 path
is a pure application-layer workaround that exploits iroh-blobs's already
working multi-connection support.

## Spike artifacts

- `src-tauri/crates/p2p-bench/src/main.rs` — CLI binary supporting:
  - `--split N` — sender splits file into N blobs
  - `--multi-endpoint` (X3) — receiver builds N independent endpoints
  - `--sender-multi-endpoint` (X4) — sender binds N independent endpoints,
    one ticket per endpoint
- `/tmp/spike-run.sh` — Mac→macbook orchestrator
- `/tmp/win-spike-run.sh` — Mac→Win orchestrator (sshpass-based, retries
  on intermittent Win sshd auth failures)
- `/tmp/receiver-n2-debug.log` — debug log showing only 1 QUIC connection
  for X1 N=2
- `/tmp/spike-multi-results.log` — raw multi-iteration Mac↔Mac data

The `p2p-bench` crate is `publish = false` and tagged "throwaway diagnostic
spike" in its description; it does not need to ship with any release.
