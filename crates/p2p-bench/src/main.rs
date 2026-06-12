//! Throwaway diagnostic spike — NOT shipped, NOT depended on by anything.
//!
//! Two-mode CLI to measure raw iroh + iroh-blobs throughput, completely
//! isolated from uc-infra / uc-application. Goal: answer "is the slow blob
//! transfer caused by something in our app stack, or by iroh-blobs / iroh
//! QUIC itself, on this exact LAN, between these two machines?"
//!
//! Subcommands:
//!   serve  --path <file> [--split N]      prints 1 or N tickets joined by ','
//!   fetch  --ticket <t1[,t2,...]>         fetches in parallel, prints aggregate
//!   offline-peer-dispatch-storm           replays the production "user copies
//!                                         N times while peer is offline" path,
//!                                         counting raw `iroh connect` attempts
//!                                         and total wall time. Used as the
//!                                         pre-refactor baseline for #886.
//!
//! Key knobs (apply to both subcommands unless noted):
//!   --tuned           (default) match uniclipboard's production QUIC config
//!   --vanilla         use iroh's stock QuicTransportConfig defaults
//!   --window-mb N     override stream_receive_window (after --tuned/--vanilla)
//!   --store {mem,fs}  receiver backing store (default fs); mem isolates disk I/O
//!   --no-relay        sender disables relay → LAN-only
//!   --split N         serve side splits the file into N blobs; fetch side
//!                     downloads all of them in parallel over the same endpoint.
//!                     N=1 (default) is the original single-blob baseline.
//!
//! Output line shape (fetch, final summary on stdout):
//!   FETCH ts=<ISO8601> n=<N> bytes=<total> wall_elapsed_ms=<n> aggregate_mbps=<f>

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use futures_lite::StreamExt;
use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::{presets, QuicTransportConfig, VarInt};
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, RelayMode, SecretKey, TransportAddr};
use iroh_blobs::api::downloader::DownloadProgressItem;
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::store::mem::MemStore;
use iroh_blobs::ticket::BlobTicket;
use iroh_blobs::BlobsProtocol;
use noq_proto::congestion::{Bbr3Config, CubicConfig};
use tokio::task::JoinSet;

#[derive(Parser)]
#[command(version, about = "iroh-blobs P2P throughput spike")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// Apply uniclipboard production transport config (BBR + 32 MB stream window
    /// + 64 MB send window + 60s idle + 15s keepalive). Default ON.
    #[arg(long, default_value_t = true, global = true)]
    tuned: bool,

    /// Use iroh's stock QuicTransportConfig defaults instead. Overrides --tuned.
    #[arg(long, default_value_t = false, global = true)]
    vanilla: bool,

    /// Override stream_receive_window in MB (after --tuned / --vanilla applied).
    #[arg(long, global = true)]
    window_mb: Option<u32>,

    /// Disable iroh relay (LAN-only). Useful to remove relay as a variable.
    #[arg(long, default_value_t = false, global = true)]
    no_relay: bool,

    /// Congestion controller. `bbr` matches production; `cubic` tests the
    /// noq#657 hypothesis. Defaults to bbr.
    #[arg(long, value_enum, default_value_t = Cc::Bbr, global = true)]
    cc: Cc,

    /// Multi-stream spike. Serve splits the file into N independent blobs;
    /// fetch downloads all N concurrently over the same endpoint (so they
    /// share one QUIC connection, multiple streams). N=1 reproduces the
    /// original single-blob baseline.
    #[arg(long, default_value_t = 1, global = true)]
    split: u32,

    /// Multi-CONNECTION spike (X3). Fetch builds one independent `Endpoint`
    /// per ticket — each gets its own UDP socket, its own QUIC connection,
    /// its own congestion controller. This is the test that says
    /// "is single-connection cwnd the ceiling, or is it the NIC/CPU?".
    /// Requires --split N > 1 on the sender side.
    #[arg(long, default_value_t = false, global = true)]
    multi_endpoint: bool,

    /// Verify mode for hypothesis A (production uses `add_path` lazy-disk-read
    /// while spike uses `add_slice` from-RAM). When true and `--split == 1`,
    /// sender uses `store.blobs().add_path(file)` like production does, so the
    /// hot serve path goes through `DataReader::read_bytes_at` (sync pread).
    /// Receiver behaviour unchanged. Use with `--store fs` on the sender side
    /// to actually exercise the FsStore + pread path.
    #[arg(long, default_value_t = false, global = true)]
    use_add_path: bool,

    /// Sender-side multi-endpoint (X4). Sender binds N independent
    /// `Endpoint`s — each with its own UDP socket and its own iroh node_id
    /// — and serves one chunk per endpoint. Receiver naturally opens N
    /// independent QUIC connections (one per unique provider node_id),
    /// bypassing any sender-side per-endpoint shared resource (UDP socket
    /// send-queue, quinn cross-connection scheduler/locks). Requires
    /// --split N > 1.
    ///
    /// This is the test that says "if the bottleneck is the sender's
    /// single `Endpoint`, does splitting it actually let aggregate
    /// throughput scale beyond a single-stream cap?" Most meaningful on
    /// asymmetric links where physical capacity > single-stream BBR
    /// equilibrium (e.g., Mac wifi → Windows wired, where iperf3 shows
    /// 100+ MB/s headroom that iroh single-stream doesn't reach).
    #[arg(long, default_value_t = false, global = true)]
    sender_multi_endpoint: bool,
}

#[derive(Copy, Clone, ValueEnum, PartialEq, Eq, Debug)]
enum Cc {
    Bbr,
    Cubic,
}

#[derive(Subcommand)]
enum Cmd {
    /// Publish a file (optionally split into N blobs) and print fetch ticket(s).
    Serve {
        #[arg(long)]
        path: PathBuf,

        #[arg(long, value_enum, default_value_t = StoreKind::Fs)]
        store: StoreKind,

        #[arg(long)]
        store_dir: Option<PathBuf>,
    },

    /// Fetch one or more ticketed blobs (','-separated). Prints aggregate
    /// throughput across all parallel downloads.
    Fetch {
        #[arg(long)]
        ticket: String,

        /// Unused: parallel mode doesn't reconstruct a single output file.
        /// Kept for CLI compatibility with the single-blob baseline path.
        #[arg(long)]
        out: PathBuf,

        #[arg(long, value_enum, default_value_t = StoreKind::Fs)]
        store: StoreKind,

        #[arg(long)]
        store_dir: Option<PathBuf>,
    },

    /// Replay the production "user copies N times while peer is offline"
    /// storm against a synthetic unreachable peer. Returns the aggregate
    /// wall-clock time, total `iroh connect` attempts, and how many times
    /// the dispatch path would have called `PresencePort::mark_offline`.
    ///
    /// Used as the pre-refactor baseline measurement for #886 (collapse
    /// the in-process negative cache into PresencePort). After phases 1
    /// and 2 land, the same invocation should show:
    ///   - wall time dropping from > 30s to < 6s (5 dispatches @ 1s)
    ///   - `iroh connect` attempts dropping from >= 15 to <= 4
    ///   - mock `mark_offline` calls dropping from N to 1
    ///
    /// This subcommand is intentionally self-contained: it inlines the
    /// staggered-retry constants from
    /// `uc-infra/src/network/iroh/connect.rs` (ATTEMPT_TIMEOUT = 3s,
    /// STAGGERED_DELAYS = [0, 500ms, 1500ms] after phase 4) so
    /// p2p-bench keeps its "throwaway spike, no uc-* deps" property.
    OfflinePeerDispatchStorm {
        /// Number of back-to-back dispatches to fire (one per simulated
        /// user copy). Default 5 — matches the issue's worked example.
        #[arg(long, default_value_t = 5)]
        dispatches: u32,

        /// Sleep between successive dispatches. Default 1000ms — matches
        /// the issue's "5 次复制 / 1s 间隔" baseline. Set to 3000 /
        /// 10000 / 30000 to sweep the inter-dispatch interval axis.
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,

        /// Synthetic unreachable target. Defaults to TEST-NET-1 (RFC
        /// 5737), which is guaranteed by RFC to be non-routable, so the
        /// QUIC handshake hits ATTEMPT_TIMEOUT instead of getting an
        /// ICMP refusal in <1ms.
        #[arg(long, default_value = "192.0.2.1:1")]
        fake_target: SocketAddr,

        /// ALPN the storm dials on. Defaults to production CLIPBOARD_ALPN
        /// so endpoint behaviour matches the real dispatch path.
        #[arg(long, default_value = "uniclipboard/clipboard/0")]
        alpn: String,
    },
}

#[derive(Copy, Clone, ValueEnum, PartialEq, Eq)]
enum StoreKind {
    Mem,
    Fs,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=warn,iroh_blobs=warn,noq=warn".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Serve {
            ref path,
            store,
            ref store_dir,
        } => {
            if cli.sender_multi_endpoint && cli.split > 1 {
                serve_multi_endpoint(&cli, path.clone(), cli.split).await
            } else {
                // Sender just needs one endpoint, no MemoryLookup seeding required.
                let endpoint = build_endpoint(&cli, None).await?;
                eprintln!("[bench] node_id = {}", endpoint.id());
                serve(
                    endpoint,
                    path.clone(),
                    store,
                    store_dir.clone(),
                    cli.split,
                    cli.use_add_path,
                )
                .await
            }
        }
        Cmd::Fetch {
            ref ticket,
            ref out,
            store,
            ref store_dir,
        } => fetch(&cli, ticket.clone(), out.clone(), store, store_dir.clone()).await,
        Cmd::OfflinePeerDispatchStorm {
            dispatches,
            interval_ms,
            fake_target,
            ref alpn,
        } => dispatch_storm(&cli, dispatches, interval_ms, fake_target, alpn.clone()).await,
    }
}

async fn build_endpoint(cli: &Cli, seeded_lookup: Option<MemoryLookup>) -> Result<Endpoint> {
    let transport = build_transport(cli);

    let relay_mode = if cli.no_relay {
        RelayMode::Disabled
    } else {
        RelayMode::Default
    };

    let mut builder = Endpoint::builder(presets::N0)
        .transport_config(transport)
        .relay_mode(relay_mode);

    if let Some(lookup) = seeded_lookup {
        builder = builder.address_lookup(lookup);
    }

    let endpoint = builder
        .bind()
        .await
        .context("failed to bind iroh endpoint")?;

    Ok(endpoint)
}

fn build_transport(cli: &Cli) -> QuicTransportConfig {
    let mut builder = QuicTransportConfig::builder();

    if cli.vanilla {
        eprintln!("[bench] transport config: VANILLA (iroh defaults)");
    } else {
        eprintln!("[bench] transport config: TUNED (matches production minus CC)");
        builder = builder
            .stream_receive_window(VarInt::from_u32(32 * 1024 * 1024))
            .send_window(64 * 1024 * 1024)
            .persistent_congestion_threshold(5)
            .max_idle_timeout(Some(
                Duration::from_secs(60)
                    .try_into()
                    .expect("60s fits QUIC encoding"),
            ))
            .keep_alive_interval(Duration::from_secs(15));
    }

    eprintln!("[bench] congestion controller: {:?}", cli.cc);
    builder = match cli.cc {
        Cc::Bbr => builder.congestion_controller_factory(Arc::new(Bbr3Config::default())),
        Cc::Cubic => builder.congestion_controller_factory(Arc::new(CubicConfig::default())),
    };

    if let Some(mb) = cli.window_mb {
        eprintln!("[bench] OVERRIDE stream_receive_window = {} MB", mb);
        builder = builder.stream_receive_window(VarInt::from_u32(mb * 1024 * 1024));
    }

    builder.build()
}

async fn serve(
    endpoint: Endpoint,
    path: PathBuf,
    store_kind: StoreKind,
    store_dir: Option<PathBuf>,
    split: u32,
    use_add_path: bool,
) -> Result<()> {
    eprintln!("[serve] waiting for iroh to come online (relay home + addrs)...");
    let online_started = Instant::now();
    endpoint.online().await;
    eprintln!(
        "[serve] online after {} ms; sleeping 3s for direct addr discovery",
        online_started.elapsed().as_millis()
    );
    tokio::time::sleep(Duration::from_secs(3)).await;

    let abs = std::path::absolute(&path).context("resolve path")?;
    let started = Instant::now();
    let n = split.max(1) as usize;

    // Production-A verify path: use add_path (lazy disk read via FsStore +
    // sync pread on serve) instead of add_slice (file pre-loaded into RAM).
    // Branch early so we skip the whole-file mem read entirely.
    if use_add_path {
        if n != 1 {
            anyhow::bail!("--use-add-path requires --split 1 (add_path can't split a file)");
        }
        let StoreKind::Fs = store_kind else {
            anyhow::bail!("--use-add-path requires --store fs (MemStore.add_path would still read all into RAM, defeating the experiment)");
        };
        let dir = store_dir.unwrap_or_else(|| PathBuf::from("./p2p-bench-store"));
        std::fs::create_dir_all(&dir).context("create fs store dir")?;
        let store = FsStore::load(&dir).await.context("load FsStore")?;
        eprintln!(
            "[serve] use_add_path=true: store.blobs().add_path({})",
            abs.display()
        );
        let tag = store
            .blobs()
            .add_path(abs.clone())
            .await
            .context("add_path failed")?;
        let import_ms = started.elapsed().as_millis() as u64;
        eprintln!(
            "[serve] add_path done in {}ms hash={} format={:?}",
            import_ms,
            &tag.hash.to_string()[..16],
            tag.format
        );
        let addr = endpoint.addr();
        let ticket = BlobTicket::new(addr, tag.hash, tag.format);
        let blobs = BlobsProtocol::new(&store, None);
        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();
        let _guard = RouterGuard::new(router, Some(store));
        println!("{}", ticket);
        eprintln!("[serve] ticket printed; serving via add_path / FsStore sync pread");
        tokio::signal::ctrl_c().await?;
        eprintln!("[serve] shutting down");
        return Ok(());
    }

    // Read the whole file once into memory, then split into N owned chunks
    // via slice copies. Each chunk goes to add_slice which import-copies it
    // into the store (BAO outboard). For a 2.7 GB test file at N=4 this
    // costs ~10 GB of transient RSS (file + 4 chunk copies + store import),
    // which fits comfortably in a 16+ GB box and only happens once at serve
    // startup — irrelevant to the throughput measurement.
    let file_bytes = tokio::fs::read(&abs).await.context("read source file")?;
    let file_size = file_bytes.len();
    let chunk_size = file_size.div_ceil(n);
    eprintln!(
        "[serve] file={} size={} bytes; split into {} chunks (~{} bytes each)",
        abs.display(),
        file_size,
        n,
        chunk_size,
    );

    let (tickets, _guard) = match store_kind {
        StoreKind::Mem => {
            let store = MemStore::new();
            let tickets = import_chunks(&endpoint, &file_bytes, chunk_size, n, |slice| {
                let store = store.clone();
                async move { store.blobs().add_slice(slice).await }
            })
            .await?;
            let blobs = BlobsProtocol::new(&store, None);
            let router = Router::builder(endpoint.clone())
                .accept(iroh_blobs::ALPN, blobs)
                .spawn();
            (tickets, RouterGuard::new(router, None::<FsStore>))
        }
        StoreKind::Fs => {
            let dir = store_dir.unwrap_or_else(|| PathBuf::from("./p2p-bench-store"));
            std::fs::create_dir_all(&dir).context("create fs store dir")?;
            let store = FsStore::load(&dir).await.context("load FsStore")?;
            let tickets = import_chunks(&endpoint, &file_bytes, chunk_size, n, |slice| {
                let store = store.clone();
                async move { store.blobs().add_slice(slice).await }
            })
            .await?;
            let blobs = BlobsProtocol::new(&store, None);
            let router = Router::builder(endpoint.clone())
                .accept(iroh_blobs::ALPN, blobs)
                .spawn();
            (tickets, RouterGuard::new(router, Some(store)))
        }
    };

    let add_ms = started.elapsed().as_millis() as u64;
    eprintln!(
        "[serve] store={:?} chunks={} add_total_ms={}",
        match store_kind {
            StoreKind::Mem => "mem",
            StoreKind::Fs => "fs",
        },
        tickets.len(),
        add_ms,
    );
    println!("{}", tickets.join(","));
    eprintln!(
        "[serve] {} ticket(s) printed to stdout; waiting for fetches, Ctrl-C to stop",
        tickets.len()
    );

    tokio::signal::ctrl_c().await?;
    eprintln!("[serve] shutting down");
    Ok(())
}

/// X4: serve N chunks from N independent sender `Endpoint`s — each gets
/// its own UDP socket, its own iroh node_id, its own MemStore. Each chunk
/// gets its own ticket pointing at its own provider. When the receiver
/// fetches all N tickets, iroh's `ConnectionPool` (keyed by remote
/// node_id) opens N independent QUIC connections, fully bypassing any
/// per-`Endpoint` shared resource on the sender side.
///
/// MemStore only (per endpoint): N independent FsStores in the same dir
/// would race; giving each its own subdir doubles disk-write surface and
/// pollutes the measurement.
///
/// Startup overhead: each endpoint does its own relay handshake + addr
/// discovery (~3-4s online + 3s grace), so N=4 spends ~15-25s in setup
/// before printing tickets. That's all amortized before the transfer
/// clock starts on the receiver side.
async fn serve_multi_endpoint(cli: &Cli, path: PathBuf, split: u32) -> Result<()> {
    let abs = std::path::absolute(&path).context("resolve path")?;
    let n = split.max(1) as usize;
    let file_bytes = tokio::fs::read(&abs).await.context("read source file")?;
    let file_size = file_bytes.len();
    let chunk_size = file_size.div_ceil(n);
    eprintln!(
        "[serve-multi] file={} size={} bytes; {} chunks (~{} bytes each); building {} independent endpoints",
        abs.display(),
        file_size,
        n,
        chunk_size,
        n,
    );

    // Bundle of (endpoint, store, router) per chunk. All held until Ctrl-C
    // to keep the senders alive.
    struct Bundle {
        _endpoint: Endpoint,
        _store: MemStore,
        _router: Router,
    }

    let mut bundles: Vec<Bundle> = Vec::with_capacity(n);
    let mut tickets: Vec<String> = Vec::with_capacity(n);

    for i in 0..n {
        let start = i * chunk_size;
        let end = (start + chunk_size).min(file_size);
        if start >= end {
            break;
        }

        let endpoint = build_endpoint(cli, None).await?;
        let online_started = Instant::now();
        endpoint.online().await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        eprintln!(
            "[serve-multi] endpoint[{i}] node_id={} online_after={}ms",
            endpoint.id(),
            online_started.elapsed().as_millis()
        );

        let store = MemStore::new();
        let chunk_slice = &file_bytes[start..end];
        let tag = store
            .blobs()
            .add_slice(chunk_slice)
            .await
            .context("add chunk to store")?;
        let addr = endpoint.addr();
        let ticket = BlobTicket::new(addr, tag.hash, tag.format);
        eprintln!(
            "[serve-multi] endpoint[{i}] chunk {}..{} ({} bytes) hash={}",
            start,
            end,
            end - start,
            &tag.hash.to_string()[..16],
        );

        let blobs = BlobsProtocol::new(&store, None);
        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();

        tickets.push(ticket.to_string());
        bundles.push(Bundle {
            _endpoint: endpoint,
            _store: store,
            _router: router,
        });
    }

    eprintln!(
        "[serve-multi] {} independent endpoints up; printing {} tickets",
        bundles.len(),
        tickets.len()
    );
    println!("{}", tickets.join(","));
    eprintln!("[serve-multi] waiting for fetches, Ctrl-C to stop");

    tokio::signal::ctrl_c().await?;
    eprintln!("[serve-multi] shutting down");
    Ok(())
}

/// Walk through `file_bytes` in N chunks of `chunk_size` (last chunk may be
/// shorter), call `add_one(slice)` for each, and build a `BlobTicket` from
/// the resulting hash + format using the endpoint's current addr.
///
/// The `add_one` closure returns a future yielding the TagInfo from
/// iroh-blobs's `add_slice` (or `add_bytes`) call — left as a closure so the
/// same logic works against `MemStore` and `FsStore` without unifying their
/// types.
async fn import_chunks<'a, F, Fut>(
    endpoint: &Endpoint,
    file_bytes: &'a [u8],
    chunk_size: usize,
    n: usize,
    mut add_one: F,
) -> Result<Vec<String>>
where
    F: FnMut(Vec<u8>) -> Fut,
    Fut: std::future::Future<
        Output = Result<iroh_blobs::api::tags::TagInfo, iroh_blobs::api::RequestError>,
    >,
{
    let mut tickets = Vec::with_capacity(n);
    for i in 0..n {
        let start = i * chunk_size;
        let end = (start + chunk_size).min(file_bytes.len());
        if start >= end {
            break;
        }
        // Copy the slice into an owned Vec so the future can outlive
        // file_bytes's borrow (necessary because the closure returns an
        // owned future).
        let chunk: Vec<u8> = file_bytes[start..end].to_vec();
        let len = chunk.len();
        let tag = add_one(chunk).await.context("add chunk to store")?;
        let addr = endpoint.addr();
        let ticket = BlobTicket::new(addr, tag.hash, tag.format);
        eprintln!(
            "[serve] chunk[{}] {}..{} ({} bytes) hash={}",
            i,
            start,
            end,
            len,
            &tag.hash.to_string()[..16]
        );
        tickets.push(ticket.to_string());
    }
    Ok(tickets)
}

/// Holds the router (so Drop doesn't immediately tear it down) and optionally
/// the FsStore (which must outlive any in-flight transfer).
struct RouterGuard<S> {
    _router: Router,
    _store: Option<S>,
}

impl<S> RouterGuard<S> {
    fn new(router: Router, store: Option<S>) -> Self {
        Self {
            _router: router,
            _store: store,
        }
    }
}

async fn fetch(
    cli: &Cli,
    ticket_str: String,
    _out: PathBuf,
    store_kind: StoreKind,
    store_dir: Option<PathBuf>,
) -> Result<()> {
    let tickets: Vec<BlobTicket> = ticket_str
        .split(',')
        .map(|s| s.parse::<BlobTicket>().context("parse ticket"))
        .collect::<Result<Vec<_>>>()?;
    let n = tickets.len();
    eprintln!(
        "[fetch] parsed {} ticket(s), store={:?}, multi_endpoint={}",
        n,
        match store_kind {
            StoreKind::Mem => "mem",
            StoreKind::Fs => "fs",
        },
        cli.multi_endpoint,
    );
    for (i, t) in tickets.iter().enumerate() {
        eprintln!(
            "[fetch]   [{}] hash={} provider={}",
            i,
            &t.hash().to_string()[..16],
            t.addr().id.fmt_short(),
        );
    }

    if cli.multi_endpoint {
        // X3 path: one independent Endpoint per ticket.
        if n < 2 {
            eprintln!("[fetch] WARNING: --multi-endpoint with n={n} is equivalent to baseline");
        }
        run_multi_endpoint(cli, &tickets).await
    } else {
        // X1 path: single shared endpoint, all tickets multiplexed on one
        // QUIC connection IF all tickets share a provider node_id. With
        // --sender-multi-endpoint on the sender side each ticket has a
        // DIFFERENT provider node_id, so seed every distinct addr — the
        // receiver's `ConnectionPool` will open one connection per unique
        // node_id, giving us N connections "for free" through the X1 fetch
        // path.
        let lookup = MemoryLookup::new();
        for (i, t) in tickets.iter().enumerate() {
            let addr = t.addr().clone();
            eprintln!(
                "[bench] seeded MemoryLookup [{}] provider {} (relays={}, ips={})",
                i,
                addr.id.fmt_short(),
                addr.relay_urls().count(),
                addr.ip_addrs().count(),
            );
            lookup.add_endpoint_info(addr);
        }
        let endpoint = build_endpoint(cli, Some(lookup)).await?;
        eprintln!("[bench] node_id = {} (single endpoint)", endpoint.id());

        match store_kind {
            StoreKind::Mem => {
                let store = MemStore::new();
                let downloader = store.downloader(&endpoint);
                run_parallel(downloader, &tickets).await?;
            }
            StoreKind::Fs => {
                let dir = store_dir.unwrap_or_else(|| PathBuf::from("./p2p-bench-store"));
                std::fs::create_dir_all(&dir).context("create fs store dir")?;
                let store = FsStore::load(&dir).await.context("load FsStore")?;
                let downloader = store.downloader(&endpoint);
                run_parallel(downloader, &tickets).await?;
            }
        }
        endpoint.close().await;
        Ok(())
    }
}

/// X3: one independent `Endpoint` per ticket. Each endpoint binds its own
/// UDP socket, gets its own `MemoryLookup` (seeded with the same provider
/// addr — every ticket shares the same sender), and runs its own download
/// via a per-endpoint `MemStore`. This is the experiment that tells us
/// whether the throughput ceiling is per-connection (cwnd) or per-NIC/CPU.
///
/// Stores are MemStore-only by design: a single FsStore can't be safely
/// shared across N endpoints (writer contention), and giving each endpoint
/// its own FsStore subdir doubles the disk-write surface, polluting the
/// measurement. MemStore strips disk-write entirely so we see pure network.
async fn run_multi_endpoint(cli: &Cli, tickets: &[BlobTicket]) -> Result<()> {
    // Sequentially build N endpoints. Each one needs a few hundred ms for
    // relay handshake + initial discovery; doing them serially keeps the
    // log readable. Total setup overhead is paid before we start the clock.
    let mut endpoints = Vec::with_capacity(tickets.len());
    for (i, t) in tickets.iter().enumerate() {
        let lookup = MemoryLookup::new();
        lookup.add_endpoint_info(t.addr().clone());
        let ep = build_endpoint(cli, Some(lookup)).await?;
        eprintln!(
            "[fetch] endpoint[{i}] node_id={} (independent UDP socket)",
            ep.id()
        );
        endpoints.push(ep);
    }

    // Spawn one task per (endpoint, ticket) pair. Each task owns its
    // endpoint and store, so there is zero sharing — N truly independent
    // QUIC connections to the same sender.
    let mut handles: Vec<tokio::task::JoinHandle<Result<(u64, Duration)>>> =
        Vec::with_capacity(tickets.len());
    for (i, (ep, t)) in endpoints
        .into_iter()
        .zip(tickets.iter().cloned())
        .enumerate()
    {
        handles.push(tokio::spawn(async move {
            let store = MemStore::new();
            let downloader = store.downloader(&ep);
            let task_started = Instant::now();
            let mut stream = downloader
                .download(t.hash_and_format(), [t.addr().id])
                .stream()
                .await
                .with_context(|| format!("downloader.stream open failed ticket[{i}]"))?;
            let mut bytes: u64 = 0;
            while let Some(item) = stream.next().await {
                match item {
                    DownloadProgressItem::Progress(total) => bytes = total,
                    DownloadProgressItem::Error(e) => {
                        anyhow::bail!("ticket[{i}] download error: {e}");
                    }
                    DownloadProgressItem::ProviderFailed { id, .. } => {
                        eprintln!(
                            "[fetch] task[{i}] provider {} failed (bytes_so_far={})",
                            id.fmt_short(),
                            bytes
                        );
                    }
                    _ => {}
                }
            }
            let elapsed = task_started.elapsed();
            eprintln!(
                "[fetch] task[{i}] DONE bytes={} elapsed={:.2}s per_conn_mbps={:.2}",
                bytes,
                elapsed.as_secs_f64(),
                if elapsed.as_secs_f64() > 0.0 {
                    (bytes as f64) / elapsed.as_secs_f64() / 1_048_576.0
                } else {
                    0.0
                },
            );
            ep.close().await;
            Ok((bytes, elapsed))
        }));
    }

    let t0 = Instant::now();
    let mut total: u64 = 0;
    let mut per_task_max = Duration::ZERO;
    for h in handles {
        let (bytes, elapsed) = h.await.context("task join failed")??;
        total += bytes;
        if elapsed > per_task_max {
            per_task_max = elapsed;
        }
    }
    let t1 = Instant::now();
    let wall = t1.duration_since(t0).as_secs_f64();
    let agg = if wall > 0.0 {
        (total as f64) / wall / 1_048_576.0
    } else {
        0.0
    };

    println!(
        "FETCH ts={} mode=multi-endpoint n={} bytes={} wall_elapsed_ms={} aggregate_mbps={:.2}",
        iso_now(),
        tickets.len(),
        total,
        (wall * 1000.0) as u64,
        agg,
    );
    eprintln!(
        "[fetch] DONE (multi-endpoint) n={} total_bytes={} wall={:.2}s aggregate={:.2} MB/s (longest_task={:.2}s)",
        tickets.len(),
        total,
        wall,
        agg,
        per_task_max.as_secs_f64(),
    );
    Ok(())
}

/// Spawn one tokio task per ticket, all sharing the same `Downloader`
/// (which routes through the same `ConnectionPool` and therefore the same
/// QUIC connection to the sender). Aggregate throughput = total bytes
/// across all tasks / wall-clock time from when all tasks have been
/// spawned to when the last one completes.
///
/// Per-task elapsed printed individually so we can see whether the streams
/// finished together (multiplexed evenly) or staggered.
async fn run_parallel(
    downloader: iroh_blobs::api::downloader::Downloader,
    tickets: &[BlobTicket],
) -> Result<()> {
    let mut handles: Vec<tokio::task::JoinHandle<Result<(u64, Duration)>>> =
        Vec::with_capacity(tickets.len());

    for (i, t) in tickets.iter().enumerate() {
        let dl = downloader.clone();
        let t = t.clone();
        handles.push(tokio::spawn(async move {
            let task_started = Instant::now();
            let mut stream = dl
                .download(t.hash_and_format(), [t.addr().id])
                .stream()
                .await
                .with_context(|| format!("downloader.stream open failed for ticket[{i}]"))?;
            let mut bytes: u64 = 0;
            // Emit a progress checkpoint every CHECKPOINT_STEP bytes so we can
            // plot throughput-over-time and spot decay during long transfers.
            const CHECKPOINT_STEP: u64 = 32 * 1024 * 1024;
            let mut last_checkpoint_bytes: u64 = 0;
            let mut last_checkpoint_at = task_started;
            while let Some(item) = stream.next().await {
                match item {
                    DownloadProgressItem::Progress(total) => {
                        bytes = total;
                        if total >= last_checkpoint_bytes + CHECKPOINT_STEP {
                            let now = Instant::now();
                            let elapsed_ms = now.duration_since(task_started).as_millis() as u64;
                            let inst_dt = now.duration_since(last_checkpoint_at).as_secs_f64();
                            let inst_db = total - last_checkpoint_bytes;
                            let inst_mbps = if inst_dt > 0.0 {
                                (inst_db as f64) / inst_dt / 1_048_576.0
                            } else {
                                0.0
                            };
                            let avg_mbps = if elapsed_ms > 0 {
                                (total as f64) / (elapsed_ms as f64 / 1000.0) / 1_048_576.0
                            } else {
                                0.0
                            };
                            println!(
                                "PROGRESS ts={} task={i} bytes={total} elapsed_ms={elapsed_ms} inst_mbps={inst_mbps:.2} avg_mbps={avg_mbps:.2}",
                                iso_now()
                            );
                            last_checkpoint_bytes = total;
                            last_checkpoint_at = now;
                        }
                    }
                    DownloadProgressItem::Error(e) => {
                        anyhow::bail!("ticket[{i}] download error: {e}");
                    }
                    DownloadProgressItem::ProviderFailed { id, .. } => {
                        eprintln!(
                            "[fetch] task[{i}] provider {} failed (bytes_so_far={})",
                            id.fmt_short(),
                            bytes
                        );
                    }
                    _ => {}
                }
            }
            let elapsed = task_started.elapsed();
            eprintln!(
                "[fetch] task[{i}] DONE bytes={} elapsed={:.2}s per_stream_mbps={:.2}",
                bytes,
                elapsed.as_secs_f64(),
                if elapsed.as_secs_f64() > 0.0 {
                    (bytes as f64) / elapsed.as_secs_f64() / 1_048_576.0
                } else {
                    0.0
                },
            );
            Ok((bytes, elapsed))
        }));
    }

    // Mark t0 AFTER all spawns are queued so wall-clock excludes spawn
    // serialization. Tokio doesn't actually run the tasks until we hit an
    // await below, but the spawn() call returning before t0 guarantees the
    // measurement excludes our own bookkeeping.
    let t0 = Instant::now();

    let mut total: u64 = 0;
    let mut per_task_max = Duration::ZERO;
    for h in handles {
        let (bytes, elapsed) = h.await.context("task join failed")??;
        total += bytes;
        if elapsed > per_task_max {
            per_task_max = elapsed;
        }
    }
    let t1 = Instant::now();
    let wall = t1.duration_since(t0).as_secs_f64();
    let agg = if wall > 0.0 {
        (total as f64) / wall / 1_048_576.0
    } else {
        0.0
    };

    println!(
        "FETCH ts={} n={} bytes={} wall_elapsed_ms={} aggregate_mbps={:.2}",
        iso_now(),
        tickets.len(),
        total,
        (wall * 1000.0) as u64,
        agg,
    );
    eprintln!(
        "[fetch] DONE n={} total_bytes={} wall={:.2}s aggregate={:.2} MB/s (longest_task={:.2}s)",
        tickets.len(),
        total,
        wall,
        agg,
        per_task_max.as_secs_f64(),
    );
    Ok(())
}

/// Per-attempt timeout inside one staggered-retry batch. 1:1 with
/// `uc-infra/src/network/iroh/connect.rs::ATTEMPT_TIMEOUT`. Keep
/// these two values in sync — the bench's whole point is to mirror
/// production cost.
const STORM_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(3);

/// Stagger pattern for the three concurrent dial attempts inside a single
/// dispatch. 1:1 with `uc-infra/src/network/iroh/connect.rs::STAGGERED_DELAYS`.
const STORM_STAGGERED_DELAYS: [Duration; 3] = [
    Duration::from_millis(0),
    Duration::from_millis(500),
    Duration::from_millis(1500),
];

/// `offline-peer-dispatch-storm` subcommand.
///
/// Spawns `dispatches` background tasks, one every `interval_ms` ms, each
/// running an inlined copy of `connect_with_staggered_retry` against an
/// unreachable synthetic peer. Counts how many raw `iroh connect`
/// attempts happen across the whole storm and how many of the
/// dispatches reach the "all attempts failed → would call mark_offline"
/// branch.
///
/// The final wall-clock window starts when the first dispatch is fired
/// and ends when every background staggered-retry has settled — this is
/// what the user perceives as "how long until the noise stops" and is
/// the metric the issue's acceptance table moves from `> 30s` to `< 6s`.
async fn dispatch_storm(
    cli: &Cli,
    dispatches: u32,
    interval_ms: u64,
    fake_target: SocketAddr,
    alpn_str: String,
) -> Result<()> {
    if dispatches == 0 {
        anyhow::bail!("--dispatches must be >= 1");
    }
    // `Endpoint::connect` wants `&'static [u8]`; leak the user-provided ALPN
    // once up front so the per-task closures can keep a static slice.
    let alpn_static: &'static [u8] = Box::leak(alpn_str.into_bytes().into_boxed_slice());

    let endpoint = build_endpoint(cli, None).await?;
    let endpoint = Arc::new(endpoint);
    eprintln!(
        "[storm] node_id={} fake_target={} alpn={}",
        endpoint.id(),
        fake_target,
        String::from_utf8_lossy(alpn_static),
    );

    // Wait until the local endpoint is online; otherwise the very first
    // dispatch races endpoint init and skews the wall-clock measurement.
    let online_started = Instant::now();
    endpoint.online().await;
    eprintln!(
        "[storm] endpoint online after {} ms",
        online_started.elapsed().as_millis()
    );

    // Synthetic unreachable peer: a fresh random NodeId paired with a
    // non-routable IP (TEST-NET-1 by default). No relay / no discovery →
    // every dial hits STORM_ATTEMPT_TIMEOUT.
    let fake_secret = SecretKey::generate();
    let fake_node_id = fake_secret.public();
    let fake_addr = EndpointAddr::from_parts(fake_node_id, [TransportAddr::Ip(fake_target)]);
    eprintln!(
        "[storm] fake peer node_id={} (synthetic, unreachable by construction)",
        fake_node_id.fmt_short()
    );

    let connect_attempts = Arc::new(AtomicUsize::new(0));
    let mock_offline_calls = Arc::new(AtomicUsize::new(0));

    let storm_started = Instant::now();
    let mut tasks: JoinSet<()> = JoinSet::new();

    for k in 0..dispatches {
        if k > 0 {
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
        let endpoint = Arc::clone(&endpoint);
        let fake_addr = fake_addr.clone();
        let connect_attempts = Arc::clone(&connect_attempts);
        let mock_offline_calls = Arc::clone(&mock_offline_calls);
        let dispatch_started_at = storm_started.elapsed();
        eprintln!(
            "[storm] dispatch[{k}] fire t+{}ms",
            dispatch_started_at.as_millis()
        );
        tasks.spawn(async move {
            mock_dispatch(
                endpoint,
                fake_addr,
                alpn_static,
                k,
                connect_attempts,
                mock_offline_calls,
            )
            .await;
        });
    }

    // Wait for every background staggered-retry to settle. The wall clock
    // we report below covers the full noise window from first fire to
    // last quiet, which is the symptom the refactor in #886 is trying to
    // shrink.
    while tasks.join_next().await.is_some() {}

    let wall = storm_started.elapsed();
    let attempts = connect_attempts.load(Ordering::SeqCst);
    let offline = mock_offline_calls.load(Ordering::SeqCst);

    println!(
        "STORM ts={} dispatches={} interval_ms={} wall_elapsed_ms={} total_iroh_connect_attempts={} mock_mark_offline_calls={} per_dispatch_avg_ms={}",
        iso_now(),
        dispatches,
        interval_ms,
        wall.as_millis() as u64,
        attempts,
        offline,
        (wall.as_millis() as u64) / (dispatches as u64),
    );
    eprintln!(
        "[storm] DONE dispatches={} wall={:.2}s attempts={} mock_mark_offline={}",
        dispatches,
        wall.as_secs_f64(),
        attempts,
        offline,
    );

    if let Ok(ep) = Arc::try_unwrap(endpoint) {
        ep.close().await;
    }
    Ok(())
}

/// In-line replica of `uc-infra/src/network/iroh/connect.rs::
/// connect_with_staggered_retry`. Kept here verbatim (modulo
/// `tracing::debug!` → `eprintln!`) so the bench's baseline numbers map
/// 1:1 onto production's dial cost.
///
/// Each call:
///   1. Spawns three concurrent attempts staggered by `STORM_STAGGERED_DELAYS`
///   2. Bumps `attempts_counter` once per attempt actually started
///   3. If every attempt fails, bumps `offline_counter` (this is the
///      branch that production calls `PresencePort::mark_offline` on).
///
/// All attempts are guaranteed to fail because `addr` is synthetic; we
/// keep the structure faithful so the "abort siblings on first success"
/// path is still exercised by the compiler / borrow checker (defensive
/// against future iroh API changes, not because it can fire here).
async fn mock_dispatch(
    endpoint: Arc<Endpoint>,
    addr: EndpointAddr,
    alpn: &'static [u8],
    dispatch_idx: u32,
    attempts_counter: Arc<AtomicUsize>,
    offline_counter: Arc<AtomicUsize>,
) {
    let mut attempts: JoinSet<Result<u32, (u32, String)>> = JoinSet::new();

    for (idx, delay) in STORM_STAGGERED_DELAYS.iter().copied().enumerate() {
        let endpoint = Arc::clone(&endpoint);
        let addr = addr.clone();
        let attempts_counter = Arc::clone(&attempts_counter);
        attempts.spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let attempt_no = (idx + 1) as u32;
            attempts_counter.fetch_add(1, Ordering::SeqCst);
            eprintln!("[storm] dispatch[{dispatch_idx}] attempt {attempt_no} started");
            match tokio::time::timeout(STORM_ATTEMPT_TIMEOUT, endpoint.connect(addr, alpn)).await {
                Ok(Ok(_conn)) => Ok(attempt_no),
                Ok(Err(err)) => Err((attempt_no, err.to_string())),
                Err(_) => Err((
                    attempt_no,
                    format!("timed out after {}ms", STORM_ATTEMPT_TIMEOUT.as_millis()),
                )),
            }
        });
    }

    let mut any_success = false;
    while let Some(joined) = attempts.join_next().await {
        match joined {
            Ok(Ok(_attempt_no)) => {
                attempts.abort_all();
                any_success = true;
                break;
            }
            Ok(Err((attempt_no, err))) => {
                eprintln!("[storm] dispatch[{dispatch_idx}] attempt {attempt_no} failed: {err}");
            }
            Err(err) => {
                eprintln!("[storm] dispatch[{dispatch_idx}] join error: {err}");
            }
        }
    }

    if !any_success {
        offline_counter.fetch_add(1, Ordering::SeqCst);
        eprintln!("[storm] dispatch[{dispatch_idx}] all attempts failed → would call mark_offline");
    }
}

fn iso_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ms = dur.subsec_millis();
    let t = time::OffsetDateTime::from_unix_timestamp(dur.as_secs() as i64)
        .expect("unix timestamp fits OffsetDateTime");
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        ms,
    )
}
