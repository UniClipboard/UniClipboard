//! Lightweight LAN ticket server for manual IP fallback pairing.
//!
//! When mDNS discovery fails (firewalls, corporate networks, Docker
//! containers), the sponsor can share their LAN IP out-of-band and the
//! joiner enters it alongside the invitation code. This module provides
//! a minimal HTTP server that serves the sponsor's ticket on a
//! well-known port.
//!
//! ## Wire protocol
//!
//! ```text
//! GET /<code_hash> HTTP/1.1
//! ```
//!
//! Returns `200 OK` with the ticket JSON string as the body, or `404`
//! if no ticket is registered for the given hash.
//!
//! ## Security
//!
//! The invitation code is never sent on the wire. The URL path is
//! `hex(blake3(code)[..8])` — the same hash the mDNS publisher uses
//! — so a passive observer learns only the hash prefix, not the code
//! itself. An attacker who already knows the hash can fetch the ticket,
//! but the ticket alone is not sufficient to complete pairing without
//! the passphrase.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{watch, Mutex};
use tracing::{debug, info, warn};

use super::discovery_constants::compute_code_hash;

/// Well-known port for the LAN ticket server. Chosen to be adjacent to
/// the mobile-sync port (42720) and unlikely to collide with common
/// services.
pub const TICKET_SERVER_PORT: u16 = 42721;

/// Holds pending invitation tickets keyed by `code_hash`.
#[derive(Clone, Default)]
struct TicketStore {
    inner: Arc<Mutex<HashMap<String, String>>>,
}

/// Handle returned by [`start_ticket_server`]. Dropping the handle
/// stops the server via the embedded shutdown signal.
pub struct TicketServerHandle {
    cancel: watch::Sender<bool>,
    store: TicketStore,
}

impl TicketServerHandle {
    /// Register a ticket for the given invitation code. The code is
    /// hashed before storage so it is never exposed on the wire.
    pub async fn register(&self, code: &str, ticket_json: &str) {
        let hash = compute_code_hash(code);
        self.store
            .inner
            .lock()
            .await
            .insert(hash.clone(), ticket_json.to_string());
        debug!(code_hash = %hash, "ticket registered on LAN ticket server");
    }

    /// Remove a ticket (on consume / cancel / expire).
    pub async fn remove(&self, code: &str) {
        let hash = compute_code_hash(code);
        if self.store.inner.lock().await.remove(&hash).is_some() {
            debug!(code_hash = %hash, "ticket removed from LAN ticket server");
        }
    }
}

impl Drop for TicketServerHandle {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}

/// Start the LAN ticket server. Returns a handle to register/remove
/// tickets and control the server's lifetime.
pub async fn start_ticket_server() -> Result<TicketServerHandle, String> {
    let store = TicketStore::default();

    let (cancel_tx, cancel_rx) = watch::channel(false);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], TICKET_SERVER_PORT));
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("ticket server bind failed on port {TICKET_SERVER_PORT}: {e}"))?;

    info!(port = TICKET_SERVER_PORT, "LAN ticket server started");

    let server_store = store.clone();
    tokio::spawn(async move {
        run_server(listener, server_store, cancel_rx).await;
        debug!("LAN ticket server stopped");
    });

    Ok(TicketServerHandle {
        cancel: cancel_tx,
        store,
    })
}

async fn run_server(
    listener: TcpListener,
    store: TicketStore,
    mut cancel_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        let store = store.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, store).await {
                                debug!(peer = %peer, error = %e, "ticket server connection error");
                            }
                        });
                    }
                    Err(e) => {
                        warn!(error = %e, "ticket server accept error");
                    }
                }
            }
            _ = cancel_rx.changed() => {
                break;
            }
        }
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    store: TicketStore,
) -> Result<(), String> {
    // Read the HTTP request (limited to 4 KB to prevent abuse).
    let mut buf = [0u8; 4096];
    let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
        .await
        .map_err(|_| "read timeout".to_string())?
        .map_err(|e| format!("read error: {e}"))?;

    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse a minimal GET /<code_hash> line.
    let path = request.lines().next().and_then(|line| {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[0] == "GET" {
            Some(parts[1].trim_start_matches('/').to_string())
        } else {
            None
        }
    });

    let response = match path {
        Some(code_hash) if !code_hash.is_empty() => {
            let tickets = store.inner.lock().await;
            match tickets.get(&code_hash) {
                Some(ticket) => {
                    debug!(code_hash = %code_hash, "ticket resolved via LAN ticket server");
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        ticket.len(),
                        ticket
                    )
                }
                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string(),
            }
        }
        _ => {
            "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
        }
    };

    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|e| format!("write error: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_resolve_ticket() {
        let handle = start_ticket_server().await.expect("server starts");
        handle
            .register("ABCD-1234", r#"{"id":"test","addrs":[]}"#)
            .await;

        let code_hash = compute_code_hash("ABCD-1234");
        let url = format!("http://127.0.0.1:{TICKET_SERVER_PORT}/{code_hash}");

        // Give the server a moment to become ready.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = reqwest::get(&url).await.expect("GET succeeds");
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert_eq!(body, r#"{"id":"test","addrs":[]}"#);

        // After removal, should 404.
        handle.remove("ABCD-1234").await;
        let resp = reqwest::get(&url).await.expect("GET succeeds");
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn unknown_code_returns_404() {
        let handle = start_ticket_server().await.expect("server starts");
        let _ = &handle; // keep alive

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let url = format!("http://127.0.0.1:{TICKET_SERVER_PORT}/deadbeefdeadbeef");
        let resp = reqwest::get(&url).await.expect("GET succeeds");
        assert_eq!(resp.status(), 404);
    }
}
