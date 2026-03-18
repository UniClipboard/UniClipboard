//! Shared JSON-RPC request/response types.
//!
//! These types are used by both the daemon (server) and uc-cli (client)
//! for communication over the Unix domain socket / named pipe.

use serde::{Deserialize, Serialize};

use crate::worker::WorkerHealth;

/// JSON-RPC 2.0 request.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub id: Option<u64>,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<RpcError>,
    pub id: Option<u64>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

/// Response payload for the `status` RPC method.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub uptime_seconds: u64,
    pub version: String,
    pub workers: Vec<WorkerStatus>,
    pub connected_peers: Option<u32>,
}

/// Status of an individual daemon worker.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub name: String,
    pub health: WorkerHealth,
}

impl RpcResponse {
    /// Create a success response with the given result value.
    pub fn success(id: Option<u64>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response with the given code and message.
    pub fn error(id: Option<u64>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(RpcError { code, message }),
            id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_request_serde_roundtrip() {
        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "status".to_string(),
            id: Some(1),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: RpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn test_rpc_response_success_format() {
        let resp = RpcResponse::success(Some(1), serde_json::json!({"ok": true}));
        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        assert_eq!(resp.id, Some(1));

        // Verify roundtrip
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: RpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_rpc_response_error_format() {
        let resp = RpcResponse::error(Some(2), -32601, "Method not found".to_string());
        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
        let err = resp.error.as_ref().unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");

        // Verify roundtrip
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: RpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, parsed);
    }

    #[test]
    fn test_status_response_serde_roundtrip() {
        let status = StatusResponse {
            uptime_seconds: 120,
            version: "0.1.0".to_string(),
            workers: vec![
                WorkerStatus {
                    name: "clipboard-watcher".to_string(),
                    health: WorkerHealth::Healthy,
                },
                WorkerStatus {
                    name: "peer-discovery".to_string(),
                    health: WorkerHealth::Degraded("timeout".to_string()),
                },
            ],
            connected_peers: Some(2),
        };
        let json = serde_json::to_string(&status).unwrap();
        let parsed: StatusResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(status, parsed);
    }
}
