// src-tauri/crates/p2p-bench/src/bin/http_blob_bench.rs
//
// Throwaway perf spike for ADR-008 OQ-perf-gate.
// Reproduces the production full-buffer blob endpoint
// (uc-webserver/src/api/blob.rs -> BlobReaderPort::get -> Vec<u8> -> Body::from)
// over loopback HTTP/1.1, and measures TTFB / throughput / (externally) RSS.
//
// Two server variants:
//   full-buffer  : EXACTLY matches production (Vec<u8> -> Full<Bytes> body).
//   streaming    : HYPOTHETICAL, NOT production. Shown to quantify what a
//                  streaming port redesign WOULD save. Never ship this shape.
//
// NOT shipped, NOT depended on by anything. publish=false, version 0.0.0.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use clap::{Parser, ValueEnum};
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::io::ReaderStream;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    /// Production-faithful: full Vec<u8> buffered into Full<Bytes> body.
    FullBuffer,
    /// Hypothetical streaming (NOT production) — for comparison only.
    Streaming,
}

#[derive(Parser, Debug)]
#[command(
    name = "http_blob_bench",
    about = "Loopback HTTP micro-bench reproducing the production full-buffer blob endpoint (ADR-008 OQ-perf-gate)."
)]
struct Cli {
    /// Loopback address to bind the bench server on.
    #[arg(long, default_value = "127.0.0.1:0")]
    addr: SocketAddr,

    /// Payload size per blob, in bytes. Use the payload tiers, e.g. 65536, 1048576, 8388608, 67108864, 268435456.
    #[arg(long, default_value_t = 1_048_576)]
    payload_bytes: usize,

    /// Number of concurrent client tasks hammering the endpoint.
    #[arg(long, default_value_t = 1)]
    concurrent: usize,

    /// Measured rounds PER client task (after warmup).
    #[arg(long, default_value_t = 50)]
    rounds: usize,

    /// Warmup rounds per task, discarded from stats.
    #[arg(long, default_value_t = 5)]
    warmup: usize,

    /// Which server body strategy to exercise.
    #[arg(long, value_enum, default_value_t = Mode::FullBuffer)]
    mode: Mode,

    /// Enable TCP_NODELAY on the reqwest client (disable Nagle).
    #[arg(long, default_value_t = true)]
    tcp_nodelay: bool,
}

/// Server state: one preallocated blob, shared immutably (Arc) so each request
/// clones the Vec<u8> — mirroring production where the port hands the handler an
/// owned Vec<u8> (read_to_end result) that is then moved into the response body.
#[derive(Clone)]
struct BlobState {
    blob: Arc<Vec<u8>>,
    /// On-disk copy of the same payload, used ONLY by the streaming variant so it
    /// can read in chunks from a file (like a streaming FilesystemBlobStore) without
    /// ever materializing the whole payload in memory.
    blob_path: Arc<std::path::PathBuf>,
    mode: Mode,
}

/// PRODUCTION-FAITHFUL handler.
/// Mirror of uc-webserver/src/api/blob.rs:43-48:
///   (StatusCode::OK, [(CONTENT_TYPE, ..)], bytes).into_response()
/// where `bytes: Vec<u8>` -> axum Body::from(Vec<u8>) == Full<Bytes>, fully buffered.
async fn handle_blob(
    State(state): State<BlobState>,
    Path(_blob_id): Path<String>,
) -> impl IntoResponse {
    match state.mode {
        Mode::FullBuffer => {
            // Production allocates an owned Vec<u8> (the read_to_end buffer) and
            // moves it into the response. We clone the shared blob to reproduce
            // the per-request owned allocation of `payload_bytes` bytes.
            let bytes: Vec<u8> = state.blob.as_ref().clone();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/octet-stream")],
                bytes, // Vec<u8> -> Full<Bytes>, fully buffered (NOT chunked/streamed)
            )
                .into_response()
        }
        Mode::Streaming => {
            // HYPOTHETICAL — NOT how production works (the BlobReaderPort signature
            // forces full-buffer today). Faithful streaming model: read the blob from
            // a real file in fixed-size chunks (what a streaming port over
            // FilesystemBlobStore would do), so the process NEVER holds the whole
            // payload — only chunk-sized buffers. This is what quantifies the RSS the
            // current full-buffer path could reclaim.
            match tokio::fs::File::open(&*state.blob_path).await {
                Ok(file) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/octet-stream")],
                    Body::from_stream(ReaderStream::new(file)),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("open blob file: {e}"),
                )
                    .into_response(),
            }
        }
    }
}

fn router(state: BlobState) -> Router {
    Router::new()
        // Mirrors production route shape: GET /clipboard/blobs/:blob_id
        .route("/clipboard/blobs/:blob_id", get(handle_blob))
        .with_state(state)
}

/// Per-request sample.
#[derive(Clone, Copy)]
struct Sample {
    ttfb_ms: f64,
    total_ms: f64,
    bytes: u64,
}

/// One full request: measure TTFB (time to first received body byte) and total
/// time (last byte). Uses bytes_stream so TTFB is genuine first-byte, matching
/// the recon's StreamExt approach.
async fn one_request(client: &reqwest::Client, url: &str) -> Result<Sample> {
    let start = Instant::now();
    let resp = client.get(url).send().await.context("send failed")?;
    if resp.status() != reqwest::StatusCode::OK {
        anyhow::bail!("non-200 status: {}", resp.status());
    }
    let mut stream = resp.bytes_stream();
    let mut ttfb_ms: Option<f64> = None;
    let mut total_bytes: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("stream chunk error")?;
        if ttfb_ms.is_none() {
            ttfb_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
        }
        total_bytes += chunk.len() as u64;
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(Sample {
        ttfb_ms: ttfb_ms.unwrap_or(total_ms),
        total_ms,
        bytes: total_bytes,
    })
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0) * (sorted.len() as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = rank - lo as f64;
        sorted[lo] * (1.0 - frac) + sorted[hi] * frac
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Mode-aware setup so each variant measures its OWN true RSS profile:
    //  - full-buffer: hold the whole payload in memory (mirrors read_to_end -> Vec<u8>),
    //    no temp file.
    //  - streaming: do NOT hold the payload in memory at all; write a temp file in
    //    1 MiB chunks (never allocating N), and serve it chunked from disk. This is
    //    what a streaming BlobReaderPort over FilesystemBlobStore would cost.
    let empty_path = Arc::new(std::path::PathBuf::new());
    let (blob, blob_path): (Arc<Vec<u8>>, Arc<std::path::PathBuf>) = match cli.mode {
        Mode::FullBuffer => (
            Arc::new(vec![0xABu8; cli.payload_bytes]),
            empty_path.clone(),
        ),
        Mode::Streaming => {
            let path =
                std::env::temp_dir().join(format!("http_blob_bench_{}.bin", std::process::id()));
            let mut f = tokio::fs::File::create(&path)
                .await
                .with_context(|| format!("create temp blob file {}", path.display()))?;
            let chunk = vec![0xABu8; 1 << 20]; // 1 MiB scratch, reused — never holds N
            let mut remaining = cli.payload_bytes;
            while remaining > 0 {
                let w = remaining.min(chunk.len());
                tokio::io::AsyncWriteExt::write_all(&mut f, &chunk[..w])
                    .await
                    .context("write temp blob chunk")?;
                remaining -= w;
            }
            tokio::io::AsyncWriteExt::flush(&mut f)
                .await
                .context("flush temp blob")?;
            (Arc::new(Vec::new()), Arc::new(path))
        }
    };
    let state = BlobState {
        blob: blob.clone(),
        blob_path: blob_path.clone(),
        mode: cli.mode,
    };

    // Bind first so we can learn the actual port when --addr uses :0.
    let listener = TcpListener::bind(cli.addr)
        .await
        .with_context(|| format!("bind {}", cli.addr))?;
    let local_addr = listener.local_addr().context("local_addr")?;
    eprintln!(
        "[http_blob_bench] pid={} listening on {} mode={:?} payload_bytes={} concurrent={} rounds={} warmup={}",
        std::process::id(),
        local_addr,
        cli.mode,
        cli.payload_bytes,
        cli.concurrent,
        cli.rounds,
        cli.warmup,
    );

    // Spawn the server. axum::serve mirrors production (server.rs:404-406).
    let app = router(state);
    let server = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[http_blob_bench] server error: {e}");
        }
    });

    // Give the server a beat to be ready, then build the client.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let client = reqwest::Client::builder()
        .tcp_nodelay(cli.tcp_nodelay)
        // No gzip/brotli features compiled in this bench's reqwest; the bench
        // server never sends Content-Encoding, matching production (no compression).
        .pool_max_idle_per_host(cli.concurrent.max(1))
        .build()
        .context("build reqwest client")?;

    let url = Arc::new(format!("http://{}/clipboard/blobs/bench", local_addr));

    // Warmup (discarded) — one task is enough to warm caches/connections.
    {
        let warm_client = client.clone();
        let warm_url = url.clone();
        for _ in 0..cli.warmup {
            let _ = one_request(&warm_client, &warm_url).await;
        }
    }
    eprintln!(
        "[http_blob_bench] warmup done; starting measured run. (sample RSS NOW via external ps)"
    );

    // Measured concurrent run.
    let wall_start = Instant::now();
    let mut tasks: JoinSet<Result<Vec<Sample>>> = JoinSet::new();
    for _ in 0..cli.concurrent.max(1) {
        let c = client.clone();
        let u = url.clone();
        let rounds = cli.rounds;
        tasks.spawn(async move {
            let mut local = Vec::with_capacity(rounds);
            for _ in 0..rounds {
                local.push(one_request(&c, &u).await?);
            }
            Ok(local)
        });
    }

    let mut all: Vec<Sample> = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        let samples = joined.context("client task panicked")??;
        all.extend(samples);
    }
    let wall_ms = wall_start.elapsed().as_secs_f64() * 1000.0;

    server.abort();

    // Aggregate.
    let n = all.len();
    if n == 0 {
        anyhow::bail!("no samples collected");
    }
    let mut ttfbs: Vec<f64> = all.iter().map(|s| s.ttfb_ms).collect();
    let mut totals: Vec<f64> = all.iter().map(|s| s.total_ms).collect();
    ttfbs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    totals.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let total_bytes: u64 = all.iter().map(|s| s.bytes).sum();
    // Aggregate throughput = all bytes moved / wall-clock (captures concurrency).
    let agg_throughput_mibps = (total_bytes as f64 / (1024.0 * 1024.0)) / (wall_ms / 1000.0);
    // Per-request mean throughput (single-stream feel).
    let per_req_mibps: Vec<f64> = all
        .iter()
        .map(|s| (s.bytes as f64 / (1024.0 * 1024.0)) / (s.total_ms / 1000.0))
        .collect();
    let mean_per_req_mibps = per_req_mibps.iter().sum::<f64>() / per_req_mibps.len() as f64;

    println!("==== http_blob_bench RESULT ====");
    println!(
        "mode={:?} payload_bytes={} concurrent={} rounds_per_task={} samples={}",
        cli.mode, cli.payload_bytes, cli.concurrent, cli.rounds, n
    );
    println!(
        "TTFB_ms  p50={:.3} p95={:.3} p99={:.3} min={:.3} max={:.3}",
        percentile(&ttfbs, 50.0),
        percentile(&ttfbs, 95.0),
        percentile(&ttfbs, 99.0),
        ttfbs.first().copied().unwrap_or(0.0),
        ttfbs.last().copied().unwrap_or(0.0),
    );
    println!(
        "TOTAL_ms p50={:.3} p95={:.3} p99={:.3}",
        percentile(&totals, 50.0),
        percentile(&totals, 95.0),
        percentile(&totals, 99.0),
    );
    println!(
        "throughput_MiBps agg={:.1} per_req_mean={:.1}",
        agg_throughput_mibps, mean_per_req_mibps
    );
    println!(
        "wall_ms={:.1} total_bytes={} (KxN concurrent-buffer peak ~= {} bytes held)",
        wall_ms,
        total_bytes,
        cli.payload_bytes as u64 * cli.concurrent.max(1) as u64
    );
    println!("================================");

    // Best-effort cleanup of the on-disk streaming copy.
    let _ = std::fs::remove_file(&*blob_path);

    Ok(())
}
