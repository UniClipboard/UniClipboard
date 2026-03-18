//! JSON-RPC method dispatch handler.
//!
//! Routes incoming RPC requests to the appropriate handler based on method name.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::rpc::types::{RpcRequest, RpcResponse, StatusResponse};
use crate::state::RuntimeState;

/// Dispatch a JSON-RPC request to the appropriate handler.
pub async fn handle_request(
    request: &RpcRequest,
    state: &Arc<RwLock<RuntimeState>>,
) -> RpcResponse {
    match request.method.as_str() {
        "ping" => RpcResponse::success(request.id, serde_json::json!("pong")),
        "status" => handle_status(request.id, state).await,
        "device_list" => handle_device_list(request.id),
        _ => RpcResponse::error(
            request.id,
            -32601,
            format!("Method not found: {}", request.method),
        ),
    }
}

/// Handle the `status` method: return uptime, version, worker statuses.
async fn handle_status(id: Option<u64>, state: &Arc<RwLock<RuntimeState>>) -> RpcResponse {
    let state = state.read().await;
    let status = StatusResponse {
        uptime_seconds: state.uptime_seconds(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        workers: state.worker_statuses().to_vec(),
        connected_peers: None, // Placeholder — PeerDirectoryPort query deferred
    };
    match serde_json::to_value(&status) {
        Ok(value) => RpcResponse::success(id, value),
        Err(e) => RpcResponse::error(id, -32603, format!("Internal error: {}", e)),
    }
}

/// Handle the `device_list` method: not yet implemented via RPC.
fn handle_device_list(id: Option<u64>) -> RpcResponse {
    RpcResponse::error(
        id,
        -32601,
        "device_list via RPC not yet implemented".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::types::WorkerStatus;
    use crate::worker::WorkerHealth;

    fn make_request(method: &str, id: Option<u64>) -> RpcRequest {
        RpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            id,
        }
    }

    #[tokio::test]
    async fn test_ping_returns_pong() {
        let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
        let req = make_request("ping", Some(1));
        let resp = handle_request(&req, &state).await;

        assert_eq!(resp.id, Some(1));
        assert!(resp.error.is_none());
        assert_eq!(resp.result, Some(serde_json::json!("pong")));
    }

    #[tokio::test]
    async fn test_status_returns_uptime_and_version() {
        let workers = vec![WorkerStatus {
            name: "clipboard-watcher".to_string(),
            health: WorkerHealth::Healthy,
        }];
        let state = Arc::new(RwLock::new(RuntimeState::new(workers)));
        let req = make_request("status", Some(2));
        let resp = handle_request(&req, &state).await;

        assert_eq!(resp.id, Some(2));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert!(result.get("uptime_seconds").is_some());
        assert!(result.get("version").is_some());
        assert!(result.get("workers").is_some());
        let workers_arr = result["workers"].as_array().unwrap();
        assert_eq!(workers_arr.len(), 1);
        assert_eq!(workers_arr[0]["name"], "clipboard-watcher");
    }

    #[tokio::test]
    async fn test_unknown_method_returns_error() {
        let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
        let req = make_request("nonexistent", Some(3));
        let resp = handle_request(&req, &state).await;

        assert_eq!(resp.id, Some(3));
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_device_list_returns_not_implemented() {
        let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
        let req = make_request("device_list", Some(4));
        let resp = handle_request(&req, &state).await;

        assert_eq!(resp.id, Some(4));
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("not yet implemented"));
    }
}
