//! Status command — connects to daemon via Unix socket RPC.

use crate::exit_codes;

/// Format an uptime duration in human-readable form.
///
/// Examples: "45s", "2m 15s", "2h 15m", "1d 3h".
fn format_uptime(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{}s", seconds);
    }
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if secs > 0 && days == 0 && hours == 0 {
        parts.push(format!("{}s", secs));
    }

    parts.join(" ")
}

/// Resolve the Unix domain socket path for daemon RPC.
///
/// Uses the XDG runtime directory if available, otherwise falls back to
/// the system temp directory. The socket file is always named
/// `uniclipboard-daemon.sock`.
#[cfg(unix)]
fn resolve_socket_path() -> std::path::PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join("uniclipboard-daemon.sock")
}

/// Run the status command (Unix platforms).
///
/// Connects to the daemon via Unix domain socket, sends a JSON-RPC `status`
/// request, and prints the response.
#[cfg(unix)]
pub async fn run(json: bool) -> i32 {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use uc_daemon::rpc::types::{RpcRequest, RpcResponse, StatusResponse};

    let socket_path = resolve_socket_path();

    // Connect with 2-second timeout
    let stream =
        match tokio::time::timeout(Duration::from_secs(2), UnixStream::connect(&socket_path)).await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(_)) | Err(_) => {
                eprintln!("Error: daemon unreachable (is uniclipboard-daemon running?)");
                return exit_codes::EXIT_DAEMON_UNREACHABLE;
            }
        };

    let (reader, mut writer) = stream.into_split();

    // Build and send RPC request
    let request = RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "status".to_string(),
        id: Some(1),
    };

    let request_json = match serde_json::to_string(&request) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Error: failed to serialize request: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    if let Err(e) = writer
        .write_all(format!("{}\n", request_json).as_bytes())
        .await
    {
        eprintln!("Error: failed to write to socket: {}", e);
        return exit_codes::EXIT_ERROR;
    }

    if let Err(e) = writer.flush().await {
        eprintln!("Error: failed to flush socket: {}", e);
        return exit_codes::EXIT_ERROR;
    }

    // Read response
    let mut buf_reader = BufReader::new(reader);
    let mut response_line = String::new();
    if let Err(e) = buf_reader.read_line(&mut response_line).await {
        eprintln!("Error: failed to read response: {}", e);
        return exit_codes::EXIT_ERROR;
    }

    let response: RpcResponse = match serde_json::from_str(&response_line) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: failed to parse response: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    // Check for RPC error
    if let Some(err) = response.error {
        eprintln!(
            "Error: daemon returned error: {} (code {})",
            err.message, err.code
        );
        return exit_codes::EXIT_ERROR;
    }

    // Extract result
    let result_value = match response.result {
        Some(v) => v,
        None => {
            eprintln!("Error: daemon returned empty result");
            return exit_codes::EXIT_ERROR;
        }
    };

    let status: StatusResponse = match serde_json::from_value(result_value) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to parse status response: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    // Print output
    if json {
        match serde_json::to_string_pretty(&status) {
            Ok(s) => println!("{}", s),
            Err(e) => {
                eprintln!("Error: failed to serialize status: {}", e);
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        let healthy_count = status
            .workers
            .iter()
            .filter(|w| matches!(w.health, uc_daemon::worker::WorkerHealth::Healthy))
            .count();
        let total_count = status.workers.len();

        println!("Status: running");
        println!("Uptime: {}", format_uptime(status.uptime_seconds));
        println!("Version: {}", status.version);
        println!("Workers: {}/{} healthy", healthy_count, total_count);
        for w in &status.workers {
            let health_str = match &w.health {
                uc_daemon::worker::WorkerHealth::Healthy => "healthy".to_string(),
                uc_daemon::worker::WorkerHealth::Degraded(reason) => {
                    format!("degraded ({})", reason)
                }
                uc_daemon::worker::WorkerHealth::Stopped => "stopped".to_string(),
            };
            println!("  {}: {}", w.name, health_str);
        }
        let peers_str = status
            .connected_peers
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        println!("Connected peers: {}", peers_str);
    }

    exit_codes::EXIT_SUCCESS
}

/// Run the status command (non-Unix platforms).
///
/// Unix socket RPC is not supported on non-Unix platforms.
#[cfg(not(unix))]
pub async fn run(_json: bool) -> i32 {
    eprintln!("Unix socket RPC not supported on this platform");
    exit_codes::EXIT_ERROR
}
