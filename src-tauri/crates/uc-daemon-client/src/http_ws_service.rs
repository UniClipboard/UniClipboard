//! HTTP + WebSocket implementation of [`DaemonService`] (ADR-008 P2.5).

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tracing::{debug, warn};
use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferResponse, DispatchOutcomeResponse, InboundEntryEvent, InboundNoticeEvent,
    ResendResponse,
};
use uc_daemon_contract::api::dto::setup_events::SetupPairingCompletedEvent;
use uc_daemon_contract::constants::{ws_event, ws_topic};

use crate::http::exchange_session_token;
use crate::service::{ControlLeaseGuard, DaemonService, FileExport};
use crate::DaemonClientContext;

pub struct HttpWsDaemonService {
    ctx: DaemonClientContext,
}

impl HttpWsDaemonService {
    pub fn new(ctx: DaemonClientContext) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl DaemonService for HttpWsDaemonService {
    async fn dispatch_text(
        &self,
        text: &str,
        peers: Option<Vec<String>>,
    ) -> Result<DispatchOutcomeResponse> {
        self.ctx.clipboard_client().dispatch_text(text, peers).await
    }

    async fn resend_entry(
        &self,
        entry_id: &str,
        peers: Option<Vec<String>>,
    ) -> Result<ResendResponse> {
        self.ctx
            .clipboard_client()
            .resend_entry(entry_id, peers)
            .await
    }

    async fn cancel_transfer(
        &self,
        transfer_id: &str,
        reason: &str,
    ) -> Result<CancelTransferResponse> {
        self.ctx
            .clipboard_client()
            .cancel_transfer(transfer_id, reason)
            .await
    }

    async fn export_entry_file(&self, entry_id: &str) -> Result<Option<FileExport>> {
        self.ctx
            .clipboard_client()
            .export_entry_file(entry_id)
            .await
    }

    async fn subscribe_inbound_notices(&self) -> Result<mpsc::Receiver<InboundNoticeEvent>> {
        let conn = self
            .ctx
            .connection_state()
            .get()
            .ok_or_else(|| anyhow::anyhow!("daemon connection info not available"))?;

        let session_token = exchange_session_token(
            &self.ctx.http(),
            &self.ctx.connection_state(),
            conn.pid,
            self.ctx.client_type(),
        )
        .await
        .context("failed to exchange session token for WS")?;

        let ws_parsed = url::Url::parse(&conn.ws_url).context("invalid daemon WS URL")?;
        let host = ws_parsed.host_str().context("daemon WS URL missing host")?;
        let port = ws_parsed
            .port_or_known_default()
            .context("daemon WS URL missing port")?;

        let mut request = conn
            .ws_url
            .as_str()
            .into_client_request()
            .map_err(|e| anyhow::anyhow!("invalid WS request: {e}"))?;
        request.headers_mut().insert(
            "Authorization",
            format!("Session {}", session_token)
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid auth header: {e}"))?,
        );

        let tcp = tokio::net::TcpStream::connect((host, port))
            .await
            .map_err(|e| anyhow::anyhow!("failed to connect to daemon WS at {host}:{port}: {e}"))?;

        let (ws_stream, _) = tokio_tungstenite::client_async(request, tcp)
            .await
            .map_err(|e| anyhow::anyhow!("WS handshake failed: {e}"))?;

        let (mut write, mut read) = ws_stream.split();

        let subscribe_msg = serde_json::json!({
            "action": "subscribe",
            "topics": [ws_topic::CLIPBOARD],
        });
        write
            .send(Message::Text(subscribe_msg.to_string()))
            .await
            .context("failed to send WS subscribe")?;

        let (tx, rx) = mpsc::channel::<InboundNoticeEvent>(64);

        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                let msg = match msg {
                    Ok(Message::Text(t)) => t,
                    Ok(Message::Ping(_)) => continue,
                    Ok(Message::Close(_)) => {
                        debug!("WS closed by server");
                        break;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        warn!(error = %e, "WS read error");
                        break;
                    }
                };

                let event: serde_json::Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let event_type = event
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                if event_type != ws_event::CLIPBOARD_INBOUND_NOTICE {
                    continue;
                }

                if let Some(payload) = event.get("payload") {
                    match serde_json::from_value::<InboundNoticeEvent>(payload.clone()) {
                        Ok(notice) => {
                            if tx.send(notice).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to decode inbound notice payload");
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn subscribe_inbound_entries(&self) -> Result<mpsc::Receiver<InboundEntryEvent>> {
        // Reuses the connection / token / handshake / topic-subscribe structure
        // of `subscribe_inbound_notices`; the spawn-read loop differs only in
        // the event type it admits (`clipboard.new_content`) and the
        // `origin == "remote"` filter that drops local-capture events.
        let conn = self
            .ctx
            .connection_state()
            .get()
            .ok_or_else(|| anyhow::anyhow!("daemon connection info not available"))?;

        let session_token = exchange_session_token(
            &self.ctx.http(),
            &self.ctx.connection_state(),
            conn.pid,
            self.ctx.client_type(),
        )
        .await
        .context("failed to exchange session token for WS")?;

        let ws_parsed = url::Url::parse(&conn.ws_url).context("invalid daemon WS URL")?;
        let host = ws_parsed.host_str().context("daemon WS URL missing host")?;
        let port = ws_parsed
            .port_or_known_default()
            .context("daemon WS URL missing port")?;

        let mut request = conn
            .ws_url
            .as_str()
            .into_client_request()
            .map_err(|e| anyhow::anyhow!("invalid WS request: {e}"))?;
        request.headers_mut().insert(
            "Authorization",
            format!("Session {}", session_token)
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid auth header: {e}"))?,
        );

        let tcp = tokio::net::TcpStream::connect((host, port))
            .await
            .map_err(|e| anyhow::anyhow!("failed to connect to daemon WS at {host}:{port}: {e}"))?;

        let (ws_stream, _) = tokio_tungstenite::client_async(request, tcp)
            .await
            .map_err(|e| anyhow::anyhow!("WS handshake failed: {e}"))?;

        let (mut write, mut read) = ws_stream.split();

        let subscribe_msg = serde_json::json!({
            "action": "subscribe",
            "topics": [ws_topic::CLIPBOARD],
        });
        write
            .send(Message::Text(subscribe_msg.to_string()))
            .await
            .context("failed to send WS subscribe")?;

        let (tx, rx) = mpsc::channel::<InboundEntryEvent>(64);

        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                let msg = match msg {
                    Ok(Message::Text(t)) => t,
                    Ok(Message::Ping(_)) => continue,
                    Ok(Message::Close(_)) => {
                        debug!("WS closed by server");
                        break;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        warn!(error = %e, "WS read error");
                        break;
                    }
                };

                let event: serde_json::Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let event_type = event
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                if event_type != ws_event::CLIPBOARD_NEW_CONTENT {
                    continue;
                }

                if let Some(payload) = event.get("payload") {
                    match serde_json::from_value::<InboundEntryEvent>(payload.clone()) {
                        Ok(entry) => {
                            // Local clipboard captures also emit new_content; only
                            // forward remote arrivals to the caller.
                            if entry.origin != "remote" {
                                continue;
                            }
                            if tx.send(entry).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to decode new_content payload");
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn subscribe_setup_pairing_completion(
        &self,
    ) -> Result<mpsc::Receiver<SetupPairingCompletedEvent>> {
        let conn = self
            .ctx
            .connection_state()
            .get()
            .ok_or_else(|| anyhow::anyhow!("daemon connection info not available"))?;

        let session_token = exchange_session_token(
            &self.ctx.http(),
            &self.ctx.connection_state(),
            conn.pid,
            self.ctx.client_type(),
        )
        .await
        .context("failed to exchange session token for WS")?;

        let ws_parsed = url::Url::parse(&conn.ws_url).context("invalid daemon WS URL")?;
        let host = ws_parsed.host_str().context("daemon WS URL missing host")?;
        let port = ws_parsed
            .port_or_known_default()
            .context("daemon WS URL missing port")?;

        let mut request = conn
            .ws_url
            .as_str()
            .into_client_request()
            .map_err(|e| anyhow::anyhow!("invalid WS request: {e}"))?;
        request.headers_mut().insert(
            "Authorization",
            format!("Session {}", session_token)
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid auth header: {e}"))?,
        );

        let tcp = tokio::net::TcpStream::connect((host, port))
            .await
            .map_err(|e| anyhow::anyhow!("failed to connect to daemon WS at {host}:{port}: {e}"))?;

        let (ws_stream, _) = tokio_tungstenite::client_async(request, tcp)
            .await
            .map_err(|e| anyhow::anyhow!("WS handshake failed: {e}"))?;

        let (mut write, mut read) = ws_stream.split();

        let subscribe_msg = serde_json::json!({
            "action": "subscribe",
            "topics": [ws_topic::SETUP],
        });
        write
            .send(Message::Text(subscribe_msg.to_string()))
            .await
            .context("failed to send WS subscribe")?;

        let (tx, rx) = mpsc::channel::<SetupPairingCompletedEvent>(64);

        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                let msg = match msg {
                    Ok(Message::Text(t)) => t,
                    Ok(Message::Ping(_)) => continue,
                    Ok(Message::Close(_)) => {
                        debug!("WS closed by server");
                        break;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        warn!(error = %e, "WS read error");
                        break;
                    }
                };

                let event: serde_json::Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let event_type = event
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                if event_type != ws_event::SETUP_PAIRING_COMPLETED {
                    continue;
                }

                if let Some(payload) = event.get("payload") {
                    match serde_json::from_value::<SetupPairingCompletedEvent>(payload.clone()) {
                        Ok(completed) => {
                            if tx.send(completed).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to decode setup pairing completed payload");
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn hold_control_lease(&self) -> Result<ControlLeaseGuard> {
        // Reuse the exact connection setup from `subscribe_inbound_notices`:
        // resolve the connection state, exchange a session token, build the
        // authenticated WS request, and connect. The daemon's
        // `handle_connection` acquires a lease the moment the authenticated WS
        // connects (BEFORE any subscribe), so a bare connection already holds
        // the lease — we deliberately do NOT send a subscribe message here.
        let conn = self
            .ctx
            .connection_state()
            .get()
            .ok_or_else(|| anyhow::anyhow!("daemon connection info not available"))?;

        let session_token = exchange_session_token(
            &self.ctx.http(),
            &self.ctx.connection_state(),
            conn.pid,
            self.ctx.client_type(),
        )
        .await
        .context("failed to exchange session token for WS")?;

        let ws_parsed = url::Url::parse(&conn.ws_url).context("invalid daemon WS URL")?;
        let host = ws_parsed.host_str().context("daemon WS URL missing host")?;
        let port = ws_parsed
            .port_or_known_default()
            .context("daemon WS URL missing port")?;

        let mut request = conn
            .ws_url
            .as_str()
            .into_client_request()
            .map_err(|e| anyhow::anyhow!("invalid WS request: {e}"))?;
        request.headers_mut().insert(
            "Authorization",
            format!("Session {}", session_token)
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid auth header: {e}"))?,
        );

        let tcp = tokio::net::TcpStream::connect((host, port))
            .await
            .map_err(|e| anyhow::anyhow!("failed to connect to daemon WS at {host}:{port}: {e}"))?;

        let (ws_stream, _) = tokio_tungstenite::client_async(request, tcp)
            .await
            .map_err(|e| anyhow::anyhow!("WS handshake failed: {e}"))?;

        let (mut write, mut read) = ws_stream.split();

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        // The keep-alive task owns BOTH halves. Draining `read` drives
        // tungstenite's automatic Ping→Pong, which keeps the lease alive; the
        // shutdown signal makes it send a best-effort clean Close and return.
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        let _ = write.send(Message::Close(None)).await;
                        return;
                    }
                    next = read.next() => match next {
                        Some(Ok(Message::Close(_))) | None => {
                            debug!("control-lease WS closed by server");
                            return;
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(e)) => {
                            warn!(error = %e, "control-lease WS read error");
                            return;
                        }
                    }
                }
            }
        });

        Ok(ControlLeaseGuard::new(shutdown_tx, handle))
    }
}
