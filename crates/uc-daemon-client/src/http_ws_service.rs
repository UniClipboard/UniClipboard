//! HTTP + WebSocket implementation of [`DaemonService`] (ADR-008 P2.5).

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tracing::{debug, warn};
use uc_daemon_contract::api::dto::clipboard::{
    EntryDetailDto, EntryProjectionResponseDto, EntryResourceDto,
};
use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferResponse, DispatchFileOutcomeResponse, DispatchOutcomeResponse,
    InboundEntryEvent, InboundNoticeEvent, ResendResponse,
};
use uc_daemon_contract::api::dto::clipboard_delivery::EntryDeliveryViewDto;
use uc_daemon_contract::api::dto::member::{
    ChooseDeviceGroupRequestDto, DeviceGroupChoiceResultDto, DeviceGroupChoicesDto,
    DeviceTrustSnapshotDto, MemberSyncPreferencesDto, MemberSyncPreferencesPatchDto,
    MemberSyncResultDto,
};
use uc_daemon_contract::api::dto::setup_events::SetupPairingCompletedEvent;
use uc_daemon_contract::api::dto::v2::setup::JoinSpaceResponse;
use uc_daemon_contract::api::types::FileTransferProgressPayload;
use uc_daemon_contract::constants::{ws_event, ws_topic};

use crate::http::exchange_session_token;
use crate::realtime::{
    ClipboardIncomingPendingEvent, FileTransferProgressEvent, FileTransferStatusChangedEvent,
};
use crate::service::{ControlLeaseGuard, DaemonService, FileExport, InboundActivityEvent};
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
    async fn remove_member(&self, peer_id: String) -> Result<DeviceTrustSnapshotDto> {
        self.ctx.pairing_client().unpair_device(peer_id).await
    }

    async fn query_device_group_choices(&self) -> Result<DeviceGroupChoicesDto> {
        self.ctx.member_client().query_device_group_choices().await
    }

    async fn cancel_join(&self, join_id: &str) -> Result<JoinSpaceResponse> {
        self.ctx.setup_v2_client().cancel_join(join_id).await
    }

    async fn reset_space(&self) -> Result<()> {
        self.ctx.setup_v2_client().reset_space().await
    }

    async fn choose_device_group(
        &self,
        request: &ChooseDeviceGroupRequestDto,
    ) -> Result<DeviceGroupChoiceResultDto> {
        self.ctx.member_client().choose_device_group(request).await
    }

    async fn member_sync_preferences(&self, device_id: &str) -> Result<MemberSyncPreferencesDto> {
        self.ctx
            .member_client()
            .member_sync_preferences(device_id)
            .await
    }

    async fn update_member_sync_preferences(
        &self,
        device_id: &str,
        patch: &MemberSyncPreferencesPatchDto,
    ) -> Result<MemberSyncResultDto> {
        self.ctx
            .member_client()
            .update_member_sync_preferences(device_id, patch)
            .await
    }

    async fn dispatch_text(
        &self,
        text: &str,
        peers: Option<Vec<String>>,
    ) -> Result<DispatchOutcomeResponse> {
        self.ctx.clipboard_client().dispatch_text(text, peers).await
    }

    async fn dispatch_file(
        &self,
        source_path: &str,
        peers: Option<Vec<String>>,
    ) -> Result<DispatchFileOutcomeResponse> {
        self.ctx
            .clipboard_client()
            .dispatch_file(source_path, peers)
            .await
    }

    async fn entry_delivery(&self, entry_id: &str) -> Result<EntryDeliveryViewDto> {
        self.ctx.clipboard_client().entry_delivery(entry_id).await
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

    async fn list_entries(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EntryProjectionResponseDto>> {
        self.ctx
            .clipboard_client()
            .list_entries(limit, offset)
            .await
    }

    async fn entry_detail(&self, entry_id: &str) -> Result<Option<EntryDetailDto>> {
        self.ctx.clipboard_client().entry_detail(entry_id).await
    }

    async fn entry_resource(&self, entry_id: &str) -> Result<Option<EntryResourceDto>> {
        self.ctx.clipboard_client().entry_resource(entry_id).await
    }

    async fn fetch_blob(&self, blob_id: &str) -> Result<Option<Vec<u8>>> {
        self.ctx.clipboard_client().fetch_blob(blob_id).await
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

    async fn subscribe_inbound_activity(&self) -> Result<mpsc::Receiver<InboundActivityEvent>> {
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
        write
            .send(Message::Text(
                serde_json::json!({
                    "action": "subscribe",
                    "topics": [ws_topic::CLIPBOARD, ws_topic::FILE_TRANSFER],
                })
                .to_string(),
            ))
            .await
            .context("failed to send WS subscribe")?;

        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(async move {
            while let Some(message) = read.next().await {
                let text = match message {
                    Ok(Message::Text(text)) => text,
                    Ok(Message::Ping(_)) => continue,
                    Ok(Message::Close(_)) => break,
                    Err(error) => {
                        warn!(error = %error, "inbound activity WS read error");
                        break;
                    }
                    Ok(_) => continue,
                };
                let envelope: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let event_type = envelope.get("type").and_then(|value| value.as_str());
                let Some(payload) = envelope.get("payload").cloned() else {
                    continue;
                };
                let event = match event_type {
                    Some(ws_event::CLIPBOARD_INCOMING_PENDING) => {
                        #[derive(serde::Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        struct Payload {
                            entry_id: String,
                            #[serde(default)]
                            attempt_id: Option<String>,
                            from_device: String,
                            #[serde(default)]
                            total_bytes: Option<u64>,
                            #[serde(default)]
                            filenames: Vec<String>,
                        }
                        serde_json::from_value::<Payload>(payload)
                            .ok()
                            .map(|value| {
                                InboundActivityEvent::Pending(ClipboardIncomingPendingEvent {
                                    entry_id: value.entry_id,
                                    attempt_id: value.attempt_id,
                                    from_device: value.from_device,
                                    total_bytes: value.total_bytes,
                                    filenames: value.filenames,
                                })
                            })
                    }
                    Some(ws_event::FILE_TRANSFER_PROGRESS) => {
                        serde_json::from_value::<FileTransferProgressPayload>(payload)
                            .ok()
                            .map(|value| {
                                InboundActivityEvent::Progress(FileTransferProgressEvent {
                                    transfer_id: value.transfer_id,
                                    entry_id: value.entry_id,
                                    attempt_id: value.attempt_id,
                                    peer_id: value.peer_id,
                                    direction: value.direction,
                                    bytes_transferred: value.bytes_transferred,
                                    total_bytes: value.total_bytes,
                                })
                            })
                    }
                    Some(ws_event::FILE_TRANSFER_STATUS_CHANGED) => {
                        #[derive(serde::Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        struct Payload {
                            transfer_id: String,
                            entry_id: Option<String>,
                            #[serde(default)]
                            attempt_id: Option<String>,
                            status: String,
                            #[serde(default)]
                            reason: Option<String>,
                        }
                        serde_json::from_value::<Payload>(payload)
                            .ok()
                            .map(|value| {
                                InboundActivityEvent::Status(FileTransferStatusChangedEvent {
                                    transfer_id: value.transfer_id,
                                    entry_id: value.entry_id,
                                    attempt_id: value.attempt_id,
                                    status: value.status,
                                    reason: value.reason,
                                })
                            })
                    }
                    Some(ws_event::CLIPBOARD_NEW_CONTENT) => {
                        serde_json::from_value::<InboundEntryEvent>(payload)
                            .ok()
                            .filter(|entry| entry.origin == "remote")
                            .map(InboundActivityEvent::Completed)
                    }
                    _ => None,
                };
                if let Some(event) = event {
                    if tx.send(event).await.is_err() {
                        break;
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
