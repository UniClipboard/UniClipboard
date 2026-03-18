//! Unix socket JSON-RPC server.
//!
//! Provides the RPC accept loop and stale socket detection for the daemon.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::rpc::handler::handle_request;
use crate::rpc::types::{RpcRequest, RpcResponse};
use crate::state::RuntimeState;

/// Check for a stale socket file and remove it if the existing daemon is not responsive.
///
/// Per CONTEXT.md locked decision, we send a full `ping` RPC request to verify
/// liveness (not just a TCP connect check).
pub async fn check_or_remove_stale_socket(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    // Try to connect with a timeout
    let connect_result =
        tokio::time::timeout(Duration::from_millis(500), UnixStream::connect(path)).await;

    match connect_result {
        Ok(Ok(stream)) => {
            // Connected — send a ping to check if daemon is truly alive
            match verify_daemon_alive(stream).await {
                Ok(true) => {
                    anyhow::bail!("daemon already running at {:?}", path);
                }
                Ok(false) | Err(_) => {
                    // Connected but not responding correctly — stale
                    warn!(
                        "removing stale socket at {:?} (connected but no valid pong)",
                        path
                    );
                    std::fs::remove_file(path)?;
                }
            }
        }
        Ok(Err(_)) | Err(_) => {
            // Connection failed or timed out — stale socket
            warn!("removing stale socket at {:?}", path);
            std::fs::remove_file(path)?;
        }
    }

    Ok(())
}

/// Send a ping RPC to a connected stream and check for pong response.
async fn verify_daemon_alive(stream: UnixStream) -> anyhow::Result<bool> {
    let (reader, mut writer) = stream.into_split();

    let ping_msg = r#"{"jsonrpc":"2.0","method":"ping","id":0}"#;
    writer.write_all(ping_msg.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;

    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    let read_result =
        tokio::time::timeout(Duration::from_millis(500), buf_reader.read_line(&mut line)).await;

    match read_result {
        Ok(Ok(n)) if n > 0 => {
            // Check if response contains "pong"
            if let Ok(resp) = serde_json::from_str::<RpcResponse>(&line) {
                if let Some(result) = &resp.result {
                    return Ok(result == &serde_json::json!("pong"));
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// Accept RPC connections on the given listener until cancelled.
///
/// The listener must already be bound (bind happens in `DaemonApp::run()` for fail-fast).
/// Connection handlers are tracked in a `JoinSet` and drained with timeout on shutdown.
pub async fn run_rpc_accept_loop(
    listener: UnixListener,
    state: Arc<RwLock<RuntimeState>>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    info!("RPC server accepting connections");

    let mut connection_tasks = JoinSet::new();

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        debug!("accepted RPC connection");
                        connection_tasks.spawn(handle_connection(stream, state.clone()));
                    }
                    Err(e) => {
                        warn!("failed to accept RPC connection: {}", e);
                    }
                }
            }
            _ = cancel.cancelled() => {
                break;
            }
        }
    }

    // Drain in-flight connections with timeout
    info!("draining in-flight RPC connections");
    tokio::time::timeout(Duration::from_secs(5), async {
        while connection_tasks.join_next().await.is_some() {}
    })
    .await
    .ok();

    info!("RPC server stopped");
    Ok(())
}

/// Handle a single RPC connection: read one request, dispatch, write response.
async fn handle_connection(stream: UnixStream, state: Arc<RwLock<RuntimeState>>) {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    match buf_reader.read_line(&mut line).await {
        Ok(0) => {
            debug!("RPC client disconnected before sending request");
            return;
        }
        Ok(_) => {}
        Err(e) => {
            warn!("failed to read RPC request: {}", e);
            return;
        }
    }

    let response = match serde_json::from_str::<RpcRequest>(&line) {
        Ok(request) => {
            debug!(method = %request.method, "handling RPC request");
            handle_request(&request, &state).await
        }
        Err(e) => {
            warn!("failed to parse RPC request: {}", e);
            RpcResponse::error(None, -32700, format!("Parse error: {}", e))
        }
    };

    match serde_json::to_string(&response) {
        Ok(json) => {
            if let Err(e) = writer.write_all(json.as_bytes()).await {
                warn!("failed to write RPC response: {}", e);
                return;
            }
            if let Err(e) = writer.write_all(b"\n").await {
                warn!("failed to write response newline: {}", e);
                return;
            }
            if let Err(e) = writer.flush().await {
                warn!("failed to flush RPC response: {}", e);
            }
        }
        Err(e) => {
            warn!("failed to serialize RPC response: {}", e);
        }
    }
}
