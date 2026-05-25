# Plan: Windows-Side Instrumentation for the 40→10 MB/s Decay

## Symptom under investigation

User report: when receiving a large file (multi-GB ISO) from Mac, throughput
**starts at ~40 MB/s and decays to ~10 MB/s over the course of the transfer**.
The decay is not reproducible on Mac↔Mac (throughput stays steady), so the
mechanism is Windows-specific — somewhere in Win's storage / Defender /
SMB / iroh-blobs receive path.

Today's spike data (`SPIKE-MULTI-STREAM.md`) shows Mac→Win N=1 starts at
**43.64 MB/s and holds it for ~70 s on a 3 GB file**. The decay must be
something that gets worse over longer transfers (15+ GB, 10+ minutes), so
the short spike doesn't reproduce it — instrumentation needs a *long-form*
test scenario.

## Hypothesis tree

The decay is one (or several) of:

### H1: Windows Defender deferred scan as the file grows
- On-access scanning runs as the file is being written
- Defender may **back off scan during high write bursts**, then catch up
  (scanning the entire growing file) once write rate dips, costing CPU +
  disk IO, throttling subsequent writes
- Most likely culprit if Defender exclusion isn't in place for the entire
  iroh-blobs store dir (`%LOCALAPPDATA%\...\iroh-blobs_dev\`) AND the
  uniclipboard file-cache (`%LOCALAPPDATA%\...\file-cache\`)

### H2: NTFS fragmentation under sustained sequential writes
- iroh-blobs FsStore writes the blob in chunks as ranges arrive
- BAO outboard is appended to as data verifies, the data file may grow
  in non-contiguous pre-allocations
- NTFS fragmentation grows during the transfer → write latency rises →
  iroh-blobs receive task gets backpressured → BBR shrinks cwnd

### H3: Filesystem cache / standby memory pressure
- Win caches recent writes in standby memory; when standby fills,
  flushing-to-disk becomes synchronous and slow
- Particularly bad on SSDs without SLC cache headroom or HDDs (latter
  unlikely on user's machine)

### H4: Win iroh / quinn receive-side regression
- Quinn / noq on Windows may have different `RecvBuf` / scheduling behavior
  than on macOS
- High-bandwidth steady state may trigger a path-validation or
  congestion-control edge case unique to Windows
- Less likely (we'd expect Mac↔Mac to flag similar issues), but not ruled
  out

### H5: iroh-blobs FsStore BAO outboard computation cost growing with file size
- BAO tree has O(N) leaf nodes; computing the outboard for a partial blob
  may grow more than linearly if the store re-validates accumulated ranges
- Would show up as receiver CPU climbing and write throughput dropping

## Instrumentation plan

Each of the five hypotheses needs its own data channel. Run them
**concurrently during one long-form test** so we can timecorrelate them.

### Test scenario

- File: **15 GB random bytes** (`/tmp/testfile-large.bin`, regenerated each run)
- Path: Mac (production uniclipboard) → Win (production uniclipboard)
- Wall-clock target: long enough to reproduce decay if user reports it
  (`>10 minutes`); 15 GB at 40 MB/s ≈ 6 minutes start, plus decay time
- One run with **Defender exclusion in place**, one run **without** —
  isolating H1 by comparison

### Data channels

#### Channel 1: receiver throughput curve

The spike binary already prints checkpointed `FETCH ts=... bytes=...` lines
every 4 MB. For production uniclipboard the equivalent is the existing
`blob fetch: progress checkpoint` log entries (already in JSON Lines
format under `%LOCALAPPDATA%\...\desktop\logs\`).

**Action**: parse the log post-run via `transfer-speed.sh` (already
written), plot per-second instantaneous MB/s.

#### Channel 2: Windows Defender activity

```powershell
# Background loop, 1 Hz sampling, log to CSV
while ($true) {
  $s = Get-MpComputerStatus
  $r = Get-MpPreference
  $obj = [PSCustomObject]@{
    ts = (Get-Date).ToString("o")
    realtime_enabled = $s.RealTimeProtectionEnabled
    ioav_enabled = $s.IoavProtectionEnabled
    on_access = $s.OnAccessProtectionEnabled
    cpu_load = (Get-Counter "\Process(MsMpEng)\% Processor Time" -ErrorAction SilentlyContinue).CounterSamples.CookedValue
    excluded_paths = ($r.ExclusionPath -join ";")
  }
  $obj | Export-Csv -Append -NoTypeInformation defender-trace.csv
  Start-Sleep 1
}
```

Also enable Defender's operational event log to capture scan events:
- Event Viewer → Applications and Services Logs → Microsoft → Windows →
  Windows Defender → Operational
- Look for event IDs 1000 (scan started), 1001 (scan completed), 5007 (config changed)

#### Channel 3: Disk write performance counters

```powershell
Get-Counter -Counter @(
  "\PhysicalDisk(_Total)\Disk Write Bytes/sec",
  "\PhysicalDisk(_Total)\Avg. Disk sec/Write",
  "\PhysicalDisk(_Total)\Current Disk Queue Length",
  "\LogicalDisk(C:)\% Free Space",
  "\Memory\Standby Cache Reserve Bytes",
  "\Memory\Modified Page List Bytes"
) -SampleInterval 1 -Continuous | Export-Counter disk-trace.blg
```

If write latency (`Avg. Disk sec/Write`) climbs over time while throughput
drops → H2/H3 confirmed.

#### Channel 4: NTFS fragmentation snapshot

Before and after the transfer:

```cmd
defrag C: /A /U /V > frag-before.txt
... run transfer ...
defrag C: /A /U /V > frag-after.txt
```

Compare "Total fragmented files" + "Average fragments per file".

#### Channel 5: iroh-blobs FsStore tracing

Add structured tracing to vendor fork's BAO write hot path. Specifically
`vendor/iroh-blobs/src/store/fs/bao_file.rs::persist` and
`src/store/fs.rs::HashContext::persist`. Emit a per-chunk event:

```rust
tracing::trace!(
  target: "bench",
  bytes = chunk.len(),
  outboard_ns = outboard_dur.as_nanos(),
  data_write_ns = data_dur.as_nanos(),
  "bao chunk persisted",
);
```

Filter the existing log subscriber to retain `bench=trace` events to the
JSONL output. Post-process the log to plot per-chunk write latency over
time — if growing, H5 confirmed.

## Order of operations

1. **First, the cheap experiment**: re-run user's failing transfer with
   Defender exclusion *audited* (`Get-MpPreference` to confirm). If
   throughput holds steady, H1 confirmed without need for further
   instrumentation. Likely outcome but worth checking before bigger work.

2. If H1 didn't catch it: enable Channels 1-4 (no code changes required,
   pure PowerShell), run the 15 GB test, post-analyze.

3. If still inconclusive: add Channel 5 tracing to the fork, rebuild,
   re-run.

## Out of scope

- This plan does NOT address the "40 MB/s start" being too slow (that's
  the `X4 sender-multi-endpoint` work from `SPIKE-MULTI-STREAM.md`). The
  two problems are independent.
- Not investigating SMB / network drive scenarios — the user reported
  direct iroh-blobs transfer, not SMB.
- Not adding any new dependencies; everything is PowerShell + existing
  log facilities + one optional vendor fork patch.

## Estimated effort

- Step 1 (Defender exclusion audit): **10 minutes**
- Step 2 (Channel 1-4 instrument + one long-form run): **45 minutes**
- Step 3 (Channel 5 tracing patch + rebuild + run): **2 hours**
