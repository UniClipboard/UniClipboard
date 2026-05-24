# Upstream issue draft

**Target repo (suggested):** [`n0-computer/iroh`](https://github.com/n0-computer/iroh/issues/new)
(`iroh` is the discoverable entry point; maintainers can re-route to `noq` if appropriate)

**Existing related issue to cross-reference:**
- [`iroh#1943` Slow network throughput](https://github.com/n0-computer/iroh/issues/1943) (umbrella)
- [`noq#657` fix(bbr3): bbr3 controller was not accounting correctly](https://github.com/n0-computer/noq/pull/657) (BBR direction, may be reversed in our env)
- [`noq#474` Make congestion controller aware of all paths](https://github.com/n0-computer/noq/issues/474) (multipath CC coordination)

The body below is plain GitHub markdown — copy from the `---` separator to the bottom of this file and paste it into a new issue.

---

## Title

> iroh-blobs single-stream LAN throughput caps at ~40% of link capacity with BBR; CUBIC is ~30× slower than BBR on the same path

---

## Body

### TL;DR

On a `192.168.31.0/24` LAN where `iperf3` (raw UDP) measures **~110 MB/s** between two Apple-silicon Macs (one Wi-Fi 6, one 2.5 GbE through the same router), a single-stream `iroh-blobs` blob fetch tops out at:

- **42-50 MB/s** with `BbrConfig` (~40% of link capacity)
- **1.29 MB/s** with `CubicConfig` (~1% of link capacity)
- **37 MB/s** with vanilla `iroh::QuicTransportConfig::builder().build()`

The BBR result is consistent with [`#1943`](https://github.com/n0-computer/iroh/issues/1943). The **CUBIC result is the surprise**: 30× slower than BBR on the same hardware, same RTT, same file. This is the inverse of the failure mode in [`noq#657`](https://github.com/n0-computer/noq/pull/657) (which reports broken BBR3 vs. healthy CUBIC). On our setup, BBR is the only usable controller.

A self-contained repro is included.

### Versions

- `iroh = "0.98"` (features = `["address-lookup-mdns"]`)
- `iroh-blobs = "0.100"` (we vendor it for an unrelated `HashContext::persist` patch — does not touch transport code)
- `noq-proto = "0.17"` (which pulls `noq = 0.18.0`)
- macOS 15.6.1, both peers aarch64

### Reproduction

Minimal CLI, two binaries (sender + receiver) sharing one source file. ~250 LoC, no application-level wrapper:

```rust
// Cargo.toml deps:
//   iroh = { version = "0.98", features = ["address-lookup-mdns"] }
//   iroh-blobs = "0.100"
//   noq-proto = "0.17"
//   ... (clap, tokio, anyhow)

// Sender:
let transport = QuicTransportConfig::builder()
    .congestion_controller_factory(Arc::new(BbrConfig::default())) // or CubicConfig
    .stream_receive_window(VarInt::from_u32(32 * 1024 * 1024))
    .send_window(64 * 1024 * 1024)
    .keep_alive_interval(Duration::from_secs(15))
    .build();
let endpoint = Endpoint::builder(presets::N0)
    .transport_config(transport)
    .bind().await?;
endpoint.online().await;  // wait for relay home + addrs
let store = FsStore::load("./store").await?;
let tag = store.blobs().add_path("/tmp/big.bin").await?;
let ticket = BlobTicket::new(endpoint.addr(), tag.hash, tag.format);
println!("{ticket}");
let _router = Router::builder(endpoint)
    .accept(iroh_blobs::ALPN, BlobsProtocol::new(&store, None))
    .spawn();

// Receiver:
let parsed: BlobTicket = ticket_string.parse()?;
let lookup = MemoryLookup::new();
lookup.add_endpoint_info(parsed.addr().clone());  // seed addrs to avoid discovery race
let endpoint = Endpoint::builder(presets::N0)
    .transport_config(same_transport_config_as_sender)
    .address_lookup(lookup)
    .bind().await?;
let store = FsStore::load("./store").await?;
let downloader = store.downloader(&endpoint);
let mut stream = downloader
    .download(parsed.hash_and_format(), [parsed.addr().id])
    .stream().await?;
// drain stream, time it
```

(Test file: 1 GB or 2 GB from `/dev/urandom` so the BAO hash is unique each run and the receiver cannot short-circuit from a cached blob.)

### Measurements

Reproduction matrix, Mac → Mac LAN, 1 GB random-content file, both sides
running the spike above:

| Sender CC | Receiver CC | Sender store | Receiver store | Throughput |
|---|---|---|---|---|
| BBR | BBR | Fs | Fs | **42.65 MB/s** |
| BBR | BBR | Fs | Fs (tuned 32M/64M windows) | **48.16 MB/s** (2.7 GB run) |
| BBR | BBR | Fs | Fs (vanilla `builder().build()`) | 37 MB/s |
| BBR | BBR | Fs | **Mem** | 21.54 MB/s |
| BBR | BBR | **Mem** | **Mem** | 21.44 MB/s |
| **CUBIC** | **CUBIC** | Fs | Fs (tuned) | **1.29 MB/s** |
| **CUBIC** | **CUBIC** | Fs | Fs (vanilla) | 1.70 MB/s |

Comparison baseline on the same LAN:

| Tool | Throughput |
|---|---|
| `iperf3 -u -b 0 -t 30 -l 1200` | ~110 MB/s |
| `iroh-blobs` + BBR (best) | ~50 MB/s (= ~45% of iperf3) |
| `iroh-blobs` + CUBIC | ~1.3-1.7 MB/s (= ~1.5%) |

### Receiver-side per-chunk pattern (slow case)

`iroh-blobs` progress checkpoints emit every 4 MB. Inter-arrival intervals
during a typical slow window:

```
4 MB → 20 MB     (+16 MB / 26 ms = 615 MB/s burst)
20 MB → 36 MB    (+16 MB / 2050 ms = 8 MB/s pause)
36 MB → 50 MB    (~700 ms per 4-MB chunk)
50 MB → 54 MB    (6 ms — another burst)
54 MB → 58 MB    (~766 ms — pause again)
```

8-chunk bursts (= 32 MB = our `stream_receive_window`) followed by
multi-second silence. We initially read this as receiver-side disk-write
backpressure, but `--store mem` on the receiver runs *slower* than
`--store fs` (21.5 vs. 42 MB/s), so the disk is not what's filling the
window. The behavior persists across MemStore + FsStore + both directions.

### What we have already ruled out

To save maintainer time, the following hypotheses have been explicitly
tested and eliminated in our environment:

- **multipath QUIC path-validation churn** — only 4 `iroh::_events::path::*`
  events in a 34-second slow window (1 × `path::open` at start, 3 ×
  `path::abandoned` at end). No `path::set_status` between. So even if
  `MAX_MULTIPATH_PATHS = 12` is on by default (we confirmed it is in 0.98 —
  see [`iroh#3635`](https://github.com/n0-computer/iroh/issues/3635) for
  the setter ignoring values < 13), it does not appear to be churning here.
- **hairpin / public NAT-reflected IP candidate dragging multipath down** —
  ruled out by finding fast transfers with 6-candidate sets and slow
  transfers with 2-candidate sets. Candidate count does not predict speed.
- **Windows Defender / NTFS / SMB / OS-specific path** — reproduces Mac → Mac.
- **Wi-Fi physical layer** — sender RSSI -19 dBm, Tx 2.4 Gbps; iperf3 110 MB/s
  baseline.
- **Mihomo / Clash TUN routing** — disabled, no effect.
- **Application-level encryption / encoding / store layer overhead** — bare
  `iroh-blobs` matches the application-stack ceiling within noise.
- **Receiver disk write** — `--store mem` is slower than `--store fs`.
- **Sender-side compress/encrypt pipeline** — `add_path` completes 7-9 seconds
  in, well before the fetch's cold-start window ends.

### Hypothesis

The 45 MB/s BBR ceiling **and** the 30× CUBIC vs. BBR ratio are both consistent
with a **`noq` 0.18 congestion-controller defect that manifests on real LAN
links with mild loss / RTT jitter** (Wi-Fi-class environment), while `noq`'s
own netsim CI (which reports `~782 Mbps` LAN-condition throughput in
[`noq#657`](https://github.com/n0-computer/noq/pull/657)) does not surface it.

In `noq#657` the reporter sees broken BBR, healthy CUBIC; we see the inverse.
Both could be the same underlying "on_packet_sent feedback path is wrong",
expressed differently on different links.

### Open questions for maintainers

1. Are there known good numbers for `iroh-blobs` single-stream LAN
   throughput on real (non-netsim) hardware as of 0.98 / noq 0.18, against
   which our 45 MB/s should be considered "low" vs. "expected"?
2. Is there a recommended tuning knob path beyond `stream_receive_window` /
   `send_window` / CC factory to recover the gap to physical capacity for
   single-stream LAN transfers? `initial_window` on the CC came up in
   [`#1943`](https://github.com/n0-computer/iroh/issues/1943) but is not
   exposed on `QuicTransportConfigBuilder`.
3. Is the CUBIC 30× regression in this environment something
   [`noq#657`](https://github.com/n0-computer/noq/pull/657)'s fix would
   address as a side effect, or does it warrant a separate CUBIC-specific
   investigation?
4. Has `1.0-rc.0` (with `MAX_MULTIPATH_PATHS` lowered to 8 and any
   post-0.18 `noq` fixes pulled in) been benchmarked against the same
   spike? If maintainers have such numbers we'd love to compare; otherwise
   we'll repeat the spike after our own upgrade and append results.

Happy to run additional configurations on this hardware if it would help —
the spike crate is ~250 LoC and the matrix above takes 10 minutes to
re-collect.
