//! Throwaway diagnostic spike — NOT shipped, NOT depended on by anything.
//!
//! Two-mode CLI to measure raw iroh + iroh-blobs throughput, completely
//! isolated from uc-infra / uc-application. Goal: answer "is the slow blob
//! transfer caused by something in our app stack, or by iroh-blobs / iroh
//! QUIC itself, on this exact LAN, between these two machines?"
//!
//! Subcommands:
//!   serve  --path <file>             prints a ticket, then accepts fetches
//!   fetch  --ticket <t> --out <dst>  fetches into <dst>, prints per-chunk timings
//!
//! Key knobs (apply to both subcommands unless noted):
//!   --tuned           (default) match uniclipboard's production QUIC config
//!   --vanilla         use iroh's stock QuicTransportConfig defaults
//!   --window-mb N     override stream_receive_window (after --tuned/--vanilla)
//!   --store {mem,fs}  receiver backing store (default fs); mem isolates disk I/O
//!   --no-relay        sender disables relay → LAN-only
//!
//! Output line shape (fetch):
//!   FETCH ts=<ISO8601> bytes=<n> elapsed_ms=<n> inst_mbps=<f> avg_mbps=<f>
//! → can be piped into ~/.../dual-side-debug for the same analysis.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use futures_lite::StreamExt;
use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::{presets, QuicTransportConfig, VarInt};
use iroh::protocol::Router;
use iroh::{Endpoint, RelayMode};
use iroh_blobs::api::downloader::DownloadProgressItem;
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::store::mem::MemStore;
use iroh_blobs::ticket::BlobTicket;
use iroh_blobs::BlobsProtocol;
use noq_proto::congestion::{BbrConfig, CubicConfig};

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
    /// Use 4 for "1-chunk window" reproduction, 256 for big-window sanity check.
    #[arg(long, global = true)]
    window_mb: Option<u32>,

    /// Disable iroh relay (LAN-only). Useful to remove relay as a variable.
    #[arg(long, default_value_t = false, global = true)]
    no_relay: bool,

    /// Congestion controller. `bbr` matches production; `cubic` tests the
    /// noq#657 hypothesis (BBRv3 in noq 0.18 has a broken on_packet_sent hook
    /// that craters throughput). Defaults to bbr.
    #[arg(long, value_enum, default_value_t = Cc::Bbr, global = true)]
    cc: Cc,
}

#[derive(Copy, Clone, ValueEnum, PartialEq, Eq, Debug)]
enum Cc {
    Bbr,
    Cubic,
}

#[derive(Subcommand)]
enum Cmd {
    /// Publish a file and print a fetch ticket. Stays alive until Ctrl-C.
    Serve {
        #[arg(long)]
        path: PathBuf,

        /// Backing store on the sender side. fs matches production; mem isolates
        /// any sender-side disk I/O (BAO outboard scan, etc).
        #[arg(long, value_enum, default_value_t = StoreKind::Fs)]
        store: StoreKind,

        /// Where FsStore lives. Ignored for mem. Defaults to ./p2p-bench-store.
        #[arg(long)]
        store_dir: Option<PathBuf>,
    },

    /// Fetch a ticketed blob into <out>. Prints per-chunk timing + final summary.
    Fetch {
        #[arg(long)]
        ticket: String,

        #[arg(long)]
        out: PathBuf,

        /// Backing store on the receiver. fs matches production; mem removes
        /// the receiver-side disk write entirely.
        #[arg(long, value_enum, default_value_t = StoreKind::Fs)]
        store: StoreKind,

        /// Where FsStore lives. Ignored for mem. Defaults to ./p2p-bench-store.
        #[arg(long)]
        store_dir: Option<PathBuf>,
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

    // For fetch, parse the ticket up front so we can seed receiver's
    // MemoryLookup with the sender's known addresses BEFORE binding the
    // endpoint. iroh's Downloader takes only EndpointIds, then asks
    // AddressLookup services to resolve them; without seeding, it has to
    // wait for mDNS/pkarr/relay discovery to find the sender — which races
    // hard in scripted timing and usually fails fast with "bytes_so_far=0".
    let seeded_lookup = if let Cmd::Fetch { ref ticket, .. } = cli.cmd {
        let parsed: BlobTicket = ticket
            .parse()
            .context("parse ticket before endpoint bind")?;
        let lookup = MemoryLookup::new();
        lookup.add_endpoint_info(parsed.addr().clone());
        eprintln!(
            "[bench] seeded MemoryLookup with provider {} (relays={}, ips={})",
            parsed.addr().id.fmt_short(),
            parsed.addr().relay_urls().count(),
            parsed.addr().ip_addrs().count(),
        );
        Some(lookup)
    } else {
        None
    };

    let endpoint = build_endpoint(&cli, seeded_lookup).await?;
    eprintln!("[bench] node_id = {}", endpoint.id());

    match cli.cmd {
        Cmd::Serve {
            ref path,
            store,
            ref store_dir,
        } => serve(endpoint, path.clone(), store, store_dir.clone()).await,
        Cmd::Fetch {
            ref ticket,
            ref out,
            store,
            ref store_dir,
        } => {
            fetch(
                endpoint,
                ticket.clone(),
                out.clone(),
                store,
                store_dir.clone(),
            )
            .await
        }
    }
}

async fn build_endpoint(cli: &Cli, seeded_lookup: Option<MemoryLookup>) -> Result<Endpoint> {
    let transport = build_transport(cli);

    let relay_mode = if cli.no_relay {
        RelayMode::Disabled
    } else {
        // presets::N0 already wires the default relay; passing RelayMode::Default
        // is the "use whatever the preset gave us" answer.
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
        // Vanilla: skip all the production tuning. --cc still applies below so
        // you can isolate "vanilla + BBR vs vanilla + CUBIC".
        eprintln!("[bench] transport config: VANILLA (iroh defaults)");
    } else {
        // Tuned: mirror src-tauri/crates/uc-infra/src/network/iroh/node.rs:260
        // — except the CC factory, which is set below based on --cc.
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

    // Apply CC factory regardless of --tuned/--vanilla. Default `bbr` matches
    // current production; `cubic` validates the noq#657 hypothesis.
    eprintln!("[bench] congestion controller: {:?}", cli.cc);
    builder = match cli.cc {
        Cc::Bbr => builder.congestion_controller_factory(Arc::new(BbrConfig::default())),
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
) -> Result<()> {
    // Wait for relay handshake + address discovery to populate. Without this,
    // `endpoint.addr()` returns an EndpointAddr with no relay URL and no
    // direct IPs — receivers can't connect because they don't know where we
    // are (only our node_id). iroh's recommended way is `endpoint.online()`
    // which waits until home_relay is set; we also give a small extra grace
    // window for the first net_report cycle so direct LAN addrs land too.
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

    let (ticket, _router) = match store_kind {
        StoreKind::Mem => {
            let store = MemStore::new();
            let tag = store.blobs().add_path(abs.clone()).await?;
            // Use full endpoint.addr() (NodeAddr) so the ticket carries our LAN IPs +
            // relay URL, not just node_id. Without this the receiver has to do mDNS /
            // pkarr discovery from scratch, which can race and time out in scripted
            // orchestration. uc-infra/blobs.rs:286 makes the same choice.
            let addr = endpoint.addr();
            eprintln!("[serve] endpoint.addr() = {:?}", addr);
            let ticket = BlobTicket::new(addr, tag.hash, tag.format);
            let blobs = BlobsProtocol::new(&store, None);
            let router = Router::builder(endpoint.clone())
                .accept(iroh_blobs::ALPN, blobs)
                .spawn();
            (ticket, RouterGuard::new(router, None::<FsStore>))
        }
        StoreKind::Fs => {
            let dir = store_dir.unwrap_or_else(|| PathBuf::from("./p2p-bench-store"));
            std::fs::create_dir_all(&dir).context("create fs store dir")?;
            let store = FsStore::load(&dir).await.context("load FsStore")?;
            let tag = store.blobs().add_path(abs.clone()).await?;
            // Use full endpoint.addr() (NodeAddr) so the ticket carries our LAN IPs +
            // relay URL, not just node_id. Without this the receiver has to do mDNS /
            // pkarr discovery from scratch, which can race and time out in scripted
            // orchestration. uc-infra/blobs.rs:286 makes the same choice.
            let addr = endpoint.addr();
            eprintln!("[serve] endpoint.addr() = {:?}", addr);
            let ticket = BlobTicket::new(addr, tag.hash, tag.format);
            let blobs = BlobsProtocol::new(&store, None);
            let router = Router::builder(endpoint.clone())
                .accept(iroh_blobs::ALPN, blobs)
                .spawn();
            (ticket, RouterGuard::new(router, Some(store)))
        }
    };

    let add_ms = started.elapsed().as_millis() as u64;
    eprintln!(
        "[serve] file={} store={:?} add_path_ms={}",
        abs.display(),
        match store_kind {
            StoreKind::Mem => "mem",
            StoreKind::Fs => "fs",
        },
        add_ms,
    );
    println!("{}", ticket);
    eprintln!("[serve] ticket printed to stdout — pipe / copy to receiver");
    eprintln!("[serve] waiting for fetches, Ctrl-C to stop");

    tokio::signal::ctrl_c().await?;
    eprintln!("[serve] shutting down");
    Ok(())
}

/// Holds the router (so Drop doesn't immediately tear it down) and optionally
/// the FsStore (which must outlive any in-flight transfer). The type parameter
/// lets us hand a `None::<FsStore>` for the MemStore branch without unifying
/// the store types.
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
    endpoint: Endpoint,
    ticket: String,
    out: PathBuf,
    store_kind: StoreKind,
    store_dir: Option<PathBuf>,
) -> Result<()> {
    let ticket: BlobTicket = ticket.parse().context("parse ticket")?;
    let provider_id = ticket.addr().id;
    let hash = ticket.hash();
    let abs_out = std::path::absolute(&out).context("resolve out path")?;

    eprintln!(
        "[fetch] provider={} hash={} store={:?}",
        provider_id.fmt_short(),
        &hash.to_string()[..16],
        match store_kind {
            StoreKind::Mem => "mem",
            StoreKind::Fs => "fs",
        },
    );

    let started = Instant::now();
    let total_bytes = match store_kind {
        StoreKind::Mem => {
            let store = MemStore::new();
            run_download(&endpoint, &store, ticket.clone(), started).await?
        }
        StoreKind::Fs => {
            let dir = store_dir.unwrap_or_else(|| PathBuf::from("./p2p-bench-store"));
            std::fs::create_dir_all(&dir).context("create fs store dir")?;
            let store = FsStore::load(&dir).await.context("load FsStore")?;
            let n = run_download(&endpoint, &store, ticket.clone(), started).await?;
            store.blobs().export(hash, abs_out.clone()).await?;
            eprintln!("[fetch] exported to {}", abs_out.display());
            n
        }
    };

    let elapsed = started.elapsed();
    let mbps = (total_bytes as f64) / elapsed.as_secs_f64() / 1_048_576.0;
    eprintln!(
        "[fetch] DONE bytes={} elapsed={:.2}s avg={:.2} MB/s",
        total_bytes,
        elapsed.as_secs_f64(),
        mbps,
    );
    endpoint.close().await;
    Ok(())
}

/// Subscribes to the Downloader progress stream and prints one stdout line per
/// observed `Progress(total)` event in a shape compatible with
/// `transfer-speed.sh` (timestamp + bytes + elapsed_ms).
///
/// The trait bound is structural: both `MemStore` and `FsStore` expose
/// `.downloader(&endpoint)` returning the same `Downloader` type, but they're
/// distinct nominal types, so we monomorphize per call site.
async fn run_download<S>(
    endpoint: &Endpoint,
    store: &S,
    ticket: BlobTicket,
    started: Instant,
) -> Result<u64>
where
    S: HasDownloader,
{
    let downloader = store.spawn_downloader(endpoint);
    let mut stream = downloader
        .download(ticket.hash_and_format(), [ticket.addr().id])
        .stream()
        .await
        .context("downloader.stream open failed")?;

    let mut bytes_so_far: u64 = 0;
    let mut last_logged: u64 = 0;
    let mut last_inst_at = started;
    let mut last_inst_bytes: u64 = 0;
    const LOG_STEP: u64 = 4 * 1024 * 1024;

    while let Some(item) = stream.next().await {
        match item {
            DownloadProgressItem::Progress(total) => {
                bytes_so_far = total;
                if total >= last_logged + LOG_STEP {
                    let now = Instant::now();
                    let elapsed_ms = now.duration_since(started).as_millis() as u64;
                    let inst_dt = now.duration_since(last_inst_at).as_secs_f64();
                    let inst_db = total - last_inst_bytes;
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
                        "FETCH ts={} bytes={} elapsed_ms={} inst_mbps={:.2} avg_mbps={:.2}",
                        iso_now(),
                        total,
                        elapsed_ms,
                        inst_mbps,
                        avg_mbps,
                    );
                    last_logged = total;
                    last_inst_at = now;
                    last_inst_bytes = total;
                }
            }
            DownloadProgressItem::PartComplete { .. } => {
                eprintln!(
                    "[fetch] part complete bytes_so_far={} elapsed_ms={}",
                    bytes_so_far,
                    started.elapsed().as_millis()
                );
            }
            DownloadProgressItem::TryProvider { id, .. } => {
                eprintln!("[fetch] trying provider {}", id.fmt_short());
            }
            DownloadProgressItem::ProviderFailed { id, .. } => {
                eprintln!(
                    "[fetch] provider {} failed (bytes_so_far={})",
                    id.fmt_short(),
                    bytes_so_far
                );
            }
            DownloadProgressItem::Error(e) => {
                anyhow::bail!("download error: {e}");
            }
            _ => {}
        }
    }

    Ok(bytes_so_far)
}

/// Tiny abstraction over MemStore / FsStore so `run_download` can be generic.
/// Both stores expose `.downloader(&endpoint) -> Downloader` in iroh-blobs 0.100.
/// The trait method is named `spawn_downloader` (not `downloader`) so that the
/// impl body `self.downloader(endpoint)` resolves to the inherent method on
/// the concrete store type instead of recursing back into the trait method.
trait HasDownloader {
    fn spawn_downloader(&self, endpoint: &Endpoint) -> iroh_blobs::api::downloader::Downloader;
}

impl HasDownloader for MemStore {
    fn spawn_downloader(&self, endpoint: &Endpoint) -> iroh_blobs::api::downloader::Downloader {
        self.downloader(endpoint)
    }
}

impl HasDownloader for FsStore {
    fn spawn_downloader(&self, endpoint: &Endpoint) -> iroh_blobs::api::downloader::Downloader {
        self.downloader(endpoint)
    }
}

fn iso_now() -> String {
    // ISO-8601 in UTC with millisecond precision, matching the production log
    // so transfer-speed.sh can parse our output unmodified.
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
