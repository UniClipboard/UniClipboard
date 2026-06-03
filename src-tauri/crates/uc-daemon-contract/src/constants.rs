//! Daemon wire-protocol string constants shared between uc-daemon (server) and uc-daemon-client (consumer).

/// WebSocket topic names used to subscribe to event streams.
pub mod ws_topic {
    pub const STATUS: &str = "status";
    pub const PEERS: &str = "peers";
    pub const PAIRED_DEVICES: &str = "paired-devices";
    pub const PAIRING: &str = "pairing";
    pub const PAIRING_SESSION: &str = "pairing/session";
    pub const PAIRING_VERIFICATION: &str = "pairing/verification";
    pub const SETUP: &str = "setup";
    pub const CLIPBOARD: &str = "clipboard";
    pub const FILE_TRANSFER: &str = "file-transfer";
    pub const ENCRYPTION: &str = "encryption";
    /// Search index events topic (Phase 92).
    pub const SEARCH: &str = "search";
}

/// WebSocket event type names emitted within topics.
pub mod ws_event {
    pub const STATUS_SNAPSHOT: &str = "status.snapshot";
    pub const STATUS_UPDATED: &str = "status.updated";
    pub const PEERS_SNAPSHOT: &str = "peers.snapshot";
    pub const PEERS_CHANGED: &str = "peers.changed";
    pub const PEERS_NAME_UPDATED: &str = "peers.nameUpdated";
    pub const PEERS_CONNECTION_CHANGED: &str = "peers.connectionChanged";
    pub const PAIRED_DEVICES_SNAPSHOT: &str = "paired-devices.snapshot";
    pub const PAIRED_DEVICES_CHANGED: &str = "paired-devices.changed";
    pub const PAIRING_SNAPSHOT: &str = "pairing.snapshot";
    pub const PAIRING_UPDATED: &str = "pairing.updated";
    pub const PAIRING_VERIFICATION_REQUIRED: &str = "pairing.verification_required";
    pub const PAIRING_COMPLETE: &str = "pairing.complete";
    pub const PAIRING_FAILED: &str = "pairing.failed";
    /// Setup pairing invitation issued (Slice4 P3 T3.1) — sponsor side after `issue_pairing_invitation`.
    pub const SETUP_INVITATION_ISSUED: &str = "setup.invitationIssued";
    /// Setup pairing completed (Slice4 P3 T3.1) — both sponsor and joiner receive once handshake terminates.
    pub const SETUP_PAIRING_COMPLETED: &str = "setup.pairingCompleted";
    /// Setup invitation revoked (Slice4 P3 T3.1) — invitation cancelled or expired before redemption.
    pub const SETUP_INVITATION_REVOKED: &str = "setup.invitationRevoked";
    pub const CLIPBOARD_NEW_CONTENT: &str = "clipboard.new_content";
    /// 接收端收到 inbound clipboard,V3 envelope 已解码,blob 拉取尚未完成。
    /// 携带最终 entry_id —— 前端在剪贴板列表中插入占位卡片,与
    /// `file-transfer.progress` 一起显示传输进度。后续 `clipboard.new_content`
    /// 到达时占位卡片自然被真实 entry 替换(同 entry_id)。
    pub const CLIPBOARD_INCOMING_PENDING: &str = "clipboard.incoming_pending";
    pub const FILE_TRANSFER_STATUS_CHANGED: &str = "file-transfer.status_changed";
    pub const FILE_TRANSFER_PROGRESS: &str = "file-transfer.progress";
    pub const ENCRYPTION_SESSION_READY: &str = "encryption.session_ready";
    /// Search availability snapshot event (Phase 92).
    pub const SEARCH_STATUS_SNAPSHOT: &str = "search.status_snapshot";
    /// Search rebuild progress event (Phase 92).
    pub const SEARCH_REBUILD_PROGRESS: &str = "search.rebuild_progress";
    /// Inbound clipboard notice with full V3 envelope payload (ADR-008 P2.5).
    /// Emitted alongside `CLIPBOARD_NEW_CONTENT`; carries base64-encoded
    /// plaintext so CLI `watch` can decode and render without an extra HTTP
    /// round-trip.
    pub const CLIPBOARD_INBOUND_NOTICE: &str = "clipboard.inbound_notice";
}

/// Pairing stage labels used in pairing session state payloads.
pub mod pairing_stage {
    pub const REQUEST: &str = "request";
    pub const VERIFICATION: &str = "verification";
    pub const VERIFYING: &str = "verifying";
    pub const COMPLETE: &str = "complete";
    pub const FAILED: &str = "failed";
}

/// Reasons emitted when a pairing request is rejected because the host is busy.
pub mod pairing_busy_reason {
    pub const HOST_NOT_DISCOVERABLE: &str = "host_not_discoverable";
    pub const NO_LOCAL_PAIRING_PARTICIPANT_READY: &str = "no_local_pairing_participant_ready";
    pub const BUSY: &str = "busy";
}

/// HTTP/JSON error codes returned by the daemon pairing API endpoints.
pub mod pairing_error_code {
    pub const ACTIVE_SESSION_EXISTS: &str = "active_session_exists";
    pub const HOST_NOT_DISCOVERABLE: &str = "host_not_discoverable";
    pub const NO_LOCAL_PARTICIPANT: &str = "no_local_participant";
    pub const SESSION_NOT_FOUND: &str = "session_not_found";
    pub const INTERNAL: &str = "internal";
    pub const BAD_REQUEST: &str = "bad_request";
    pub const RUNTIME_UNAVAILABLE: &str = "runtime_unavailable";
}

/// HTTP route path prefixes for daemon REST endpoints.
pub mod http_route {
    /// POST /clipboard/restore/:entry_id — restore clipboard entry to OS clipboard
    pub const CLIPBOARD_RESTORE: &str = "/clipboard/restore";
    /// GET /clipboard/entries — list clipboard entries with pagination
    pub const CLIPBOARD_ENTRIES: &str = "/clipboard/entries";
    /// GET /clipboard/stats — clipboard statistics
    pub const CLIPBOARD_STATS: &str = "/clipboard/stats";
    /// GET /settings — daemon settings
    pub const SETTINGS: &str = "/settings";
    /// POST /settings/relay-probe — probe a candidate relay URL (ADR-008 P3-3 B2'-1)
    pub const SETTINGS_RELAY_PROBE: &str = "/settings/relay-probe";
    /// GET /encryption/state — encryption state
    pub const ENCRYPTION_STATE: &str = "/encryption/state";
    /// POST /encryption/unlock — unlock encryption with passphrase
    pub const ENCRYPTION_UNLOCK: &str = "/encryption/unlock";
    /// POST /encryption/lock — lock encryption
    pub const ENCRYPTION_LOCK: &str = "/encryption/lock";
    /// GET /storage/stats — storage statistics
    pub const STORAGE_STATS: &str = "/storage/stats";
    /// POST /storage/clear-cache — clear storage cache
    pub const STORAGE_CLEAR_CACHE: &str = "/storage/clear-cache";
    /// GET /clipboard/blobs/:blob_id — serve raw blob binary content
    pub const CLIPBOARD_BLOBS: &str = "/clipboard/blobs";
    /// GET /clipboard/thumbnails/:rep_id — serve raw thumbnail binary content
    pub const CLIPBOARD_THUMBNAILS: &str = "/clipboard/thumbnails";
    /// GET /search/query — execute a structured search query (Phase 92)
    pub const SEARCH_QUERY: &str = "/search/query";
    /// GET /search/status — get search index availability status (Phase 92)
    pub const SEARCH_STATUS: &str = "/search/status";
    /// POST /search/rebuild — trigger manual search index rebuild (Phase 92)
    pub const SEARCH_REBUILD: &str = "/search/rebuild";
    /// GET /upgrade/status — detect upgrade by comparing version cursor to
    /// the running build (P1 thin upgrade detection).
    pub const UPGRADE_STATUS: &str = "/upgrade/status";
    /// POST /upgrade/ack — advance the version cursor to the running build.
    pub const UPGRADE_ACK: &str = "/upgrade/ack";
    /// POST /clipboard/dispatch — dispatch plaintext to online peers (ADR-008 P2.5 / D7)
    pub const CLIPBOARD_DISPATCH: &str = "/clipboard/dispatch";
    /// POST /clipboard/resend — resend a previously captured entry (ADR-008 P2.5 / D7)
    pub const CLIPBOARD_RESEND: &str = "/clipboard/resend";
    /// POST /clipboard/cancel-transfer/:transfer_id — cancel an in-flight inbound transfer
    pub const CLIPBOARD_CANCEL_TRANSFER: &str = "/clipboard/cancel-transfer";
}

/// HTTP route paths for the v2 daemon REST endpoints (Slice4 P3 T3.2).
///
/// Stateless setup pairing endpoints under `/v2/setup/*`. Each route
/// maps to a `SpaceSetupFacade` method; legacy `/setup/*` paths in
/// [`http_route`] above stay live until T3.4 deletes them in one shot.
pub mod http_route_v2 {
    /// POST /v2/setup/initialize — A1 initialise space.
    pub const SETUP_INITIALIZE: &str = "/v2/setup/initialize";
    /// POST /v2/setup/issue-invitation — B1 sponsor mints an invitation.
    pub const SETUP_ISSUE_INVITATION: &str = "/v2/setup/issue-invitation";
    /// POST /v2/setup/redeem — B2 joiner redeems an invitation.
    pub const SETUP_REDEEM: &str = "/v2/setup/redeem";
    /// POST /v2/setup/cancel — drop in-flight invitation; 409 when none.
    pub const SETUP_CANCEL: &str = "/v2/setup/cancel";
    /// POST /v2/setup/reset — clear setup status + pending invitations.
    pub const SETUP_RESET: &str = "/v2/setup/reset";
    /// GET /v2/setup/state — read-only snapshot for the v2 UI.
    pub const SETUP_STATE: &str = "/v2/setup/state";
    /// POST /v2/setup/switch-space — already-setup device joins another sponsor's
    /// space, running the 4-phase clipboard re-encryption migration.
    pub const SETUP_SWITCH_SPACE: &str = "/v2/setup/switch-space";
    /// GET /v2/setup/migration-progress — coarse progress snapshot for UI polling
    /// during a switch-space migration. Returns `phase = null` when idle.
    pub const SETUP_MIGRATION_PROGRESS: &str = "/v2/setup/migration-progress";
}

/// HTTP route paths for daemon auth endpoints.
pub mod auth_route {
    /// POST /auth/connect — exchange bearer token for JWT session token
    pub const AUTH_CONNECT: &str = "/auth/connect";
}
