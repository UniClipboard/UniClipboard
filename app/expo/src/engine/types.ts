export type EngineErrorCode =
  | "invalid_argument"
  | "invalid_state"
  | "closed"
  | "cancelled"
  | "not_found"
  | "resource_expired"
  | "conflict"
  | "in_use"
  | "permission_denied"
  | "identity_mismatch"
  | "protocol_mismatch"
  | "sync_disabled"
  | "suspended"
  | "no_targets"
  | "limit_exceeded"
  | "resource_exhausted"
  | "timeout"
  | "unavailable"
  | "unsupported"
  | "incomplete"
  | "invalid"
  | "expired"
  | "rate_limited"
  | "resync_required"
  | "source_changed"
  | "too_large"
  | "busy"
  | "failed"
  | "internal";

export interface EngineErrorPayload {
  code: EngineErrorCode;
  message: string;
  retryable?: boolean;
  retry_after_ms?: number;
}

export class EngineError extends Error {
  readonly code: EngineErrorCode;
  readonly retryable: boolean;
  readonly retryAfterMs?: number;

  constructor(payload: EngineErrorPayload) {
    super(payload.message);
    this.name = "EngineError";
    this.code = payload.code;
    this.retryable = Boolean(payload.retryable);
    this.retryAfterMs = payload.retry_after_ms;
  }
}

export type MigrationSourceKind = "v1_vault" | "mobile_sqlite" | "";

export interface SetupFlowState {
  complete: boolean;
  source: string;
  source_kind?: MigrationSourceKind;
  requires_unlock?: boolean;
  default_source: string;
}

export interface SourceDetectionResult {
  status: "none" | "found" | "invalid" | "error";
  source: string;
  source_kind?: MigrationSourceKind;
  requires_unlock?: boolean;
}

export interface CustomSourceSelectionResult {
  cancelled: boolean;
  source?: string;
  source_kind?: MigrationSourceKind;
  requires_unlock?: boolean;
}

export interface MigrationProgress {
  stage: "import" | "verify" | string;
  processed: number;
  total: number;
}

export interface MigrationRunResult {
  status: "complete" | "done" | "partial" | "cancelled" | "failed" | string;
  total: number;
  imported: number;
  recoverable: number;
  skipped: number;
  failed: number;
  missing_parts: number;
  verified: number;
  verification_failed: number;
  tags: number;
  tag_failures: number;
  unsupported_tags: number;
  tag_mismatches: number;
  tags_repaired: number;
  tag_repairs_skipped: number;
  colors_imported: number;
  colors_preserved: number;
  colors_missing: number;
  colors_verified: number;
  color_failures: number;
  error?: EngineErrorPayload;
}

export type SetupScreenViewState =
  | "loading"
  | "w-detected"
  | "w-clean"
  | "w-custom"
  | "w-encrypted"
  | "w-invalid"
  | "m-auth"
  | "m-auth-err"
  | "m-importing"
  | "m-verifying"
  | "m-stopping"
  | "r-done"
  | "r-partial"
  | "r-cancelled"
  | "r-conflict"
  | "r-failed";

export interface DeviceBinding {
  id: string;
  instance_id: string;
  state: "pending" | "authorized" | string;
  sync_enabled: boolean;
  revision: number;
  last_active_ms?: number;
  last_error?: string;
  connection: "disconnected" | "connecting" | "connected" | string;
  relay_route_ready?: boolean;
  ticket?: boolean;
}

export interface EngineDevice {
  id: string;
  name?: string;
  reported_name?: string;
  fingerprint: string;
  trust: "pending" | "trusted" | string;
  revision: number;
  updated_ms: number;
  bindings?: DeviceBinding[];
}

export interface EngineCandidate {
  device_id: string;
  backend_id: string;
  name: string;
  introducers: Array<{ device_id: string; name: string }>;
  last_seen_ms: number;
  ignored: boolean;
  requested: boolean;
  requested_until_ms?: number;
}

export interface PairingCode {
  code: string;
  expires_at_ms: number;
}

export interface PairingCodeResponse {
  exists: boolean;
  code: string;
  expires_at_ms: number;
}

export interface SensitiveTicket {
  ticket: string;
  expires_at_ms: number;
  qr_png?: string;
}

export interface EngineOperation {
  id: string;
  request_id: string;
  kind: string;
  created_at_ms: number;
  updated_at_ms: number;
  phase:
    | "resolving"
    | "connecting"
    | "handshaking"
    | "running"
    | "queued"
    | "sending"
    | "succeeded"
    | "completed"
    | "failed"
    | string;
  error?: EngineErrorPayload;
  targets?: Array<{
    backend_id?: string;
    target_id?: string;
    peer_id?: string;
    state: "queued" | "sending" | "sent" | "discarded" | "failed" | "superseded";
    receipt_stage?: "local_written" | "relay_stored" | "remote_received" | "remote_discarded" | string;
    transport_route?: "p2p" | "relay" | "hybrid" | string;
    progress?: {
      bytes_sent: number;
      total_bytes: number;
      part_index?: number;
      part_count?: number;
    };
  }>;
  pairing?: {
    peer_id: string;
    awaiting_local_approval: boolean;
    awaiting_remote_approval: boolean;
  };
}

export interface HistoryOrigin {
  kind: "local_system" | "remote_device" | "action" | "explicit" | "external" | "migration" | string;
  device_id?: string;
  plugin_id?: string;
}

export interface HistoryPartSummary {
  available: boolean;
  kind: "representation" | "file" | string;
  mime?: string;
  size: number;
}

export interface HistoryItem {
  id: string;
  revision: string;
  kind: "text" | "html" | "image" | "file" | "files" | "directory" | "directory_reference" | "mixed" | string;
  origin: HistoryOrigin;
  source_application?: { name?: string };
  created_at_ms: number;
  updated_at_ms: number;
  expires_at_ms: number;
  tags: string[];
  parts: HistoryPartSummary[];
  total_bytes: number;
  sync_state: string;
}

export interface HistoryPage {
  items: HistoryItem[];
  next_cursor?: string;
  history_revision: string;
}

export interface TextPreview {
  text: string;
  html?: string;
  truncated: boolean;
}

export interface PartDescriptor {
  id: string;
  kind: "representation" | "file" | string;
  mime: string;
  name?: string;
  size: number;
  referenced?: boolean;
}

export interface ContentDescriptor {
  source_application?: { name?: string };
  id: string;
  parts: PartDescriptor[];
  total_size: number;
}

export interface ReceivedContent {
  receipt_id: string;
  content: ContentDescriptor;
  peer_id?: string;
  channel_id?: string;
  auto_apply?: boolean;
  received_at_ms: number;
  expires_at_ms: number;
  apply_state: "available" | "applying" | "applied" | "failed" | "deferred" | string;
  history_id?: string;
}

export interface ActiveApply {
  kind: "received" | "history" | string;
  history_id?: string;
  receipt_id?: string;
  content_id: string;
}

export interface ImagePreviewResult {
  part?: PartDescriptor;
  mime?: string;
  name?: string;
  bytes_written?: number;
  path?: string;
  uri?: string;
}

export interface HistorySearchHit {
  item: HistoryItem;
  preview: {
    text: string;
    highlights: Array<{ start: number; end: number }>;
  };
}

export interface HistorySearchPage {
  hits: HistorySearchHit[];
  next_cursor?: string;
  history_revision: string;
  evaluated_at_ms: number;
  index_incomplete?: boolean;
}

export interface HistorySearchCountsResult {
  counts: number[];
  history_revision: string;
  evaluated_at_ms: number;
}

export interface ActionInfo {
  key: string;
  plugin_id: string;
  id: string;
  title: string;
  accepts: string[];
  placements: string[];
  post_actions: string[];
}

export interface ActionResult {
  request_id: string;
  history_id: string;
}

export interface SubmissionResult {
  request_id: string;
  history_id?: string;
  phase: string;
  sync_disposition?: string;
  operation_id?: string;
}

export interface PickAndCommitFileResult {
  cancelled: boolean;
  submission?: SubmissionResult;
}

export interface HostCapabilities {
  read_formats: string[];
  write_formats: string[];
  read_files: boolean;
  write_files: boolean;
  observe: string;
  apply: string;
}

export interface Capabilities {
  host: HostCapabilities;
  core_formats: string[];
  effective: HostCapabilities;
  restrictions?: string[];
}

export interface DirectorySyncSettings {
  automatic: boolean;
}

export interface RelaySyncSettings {
  enabled: boolean;
  url?: string;
  has_access_key?: boolean;
  max_bytes?: number;
}

export type CloudAuthState = "signed_out" | "active" | "reauth_required";

export interface CloudStatusSnapshot {
  signed_in: boolean;
  reauth_required?: boolean;
  state: CloudAuthState;
  cloud_url?: string;
  account?: string;
  email?: string;
  device_name?: string;
  signed_in_at_ms?: number;
  relay_url?: string;
  credential_prefix?: string;
  credential_expires_at_ms?: number;
  credential_rotate_after_ms?: number;
}

export interface SyncSettings {
  directories: DirectorySyncSettings;
  relay?: RelaySyncSettings;
  enabled: boolean;
  automatic: boolean;
  on_restore: boolean;
  files: boolean;
  introductions: boolean;
}

export interface BackendSyncPolicy {
  text: boolean;
  images: boolean;
  files: boolean;
  auto_file_max_bytes: number;
}

export interface HistoryCleanupSettings {
  auto_cleanup: boolean;
  retention_days: number;
  max_items: number;
  excluded_tags: string[];
}

export interface FileCacheSettings {
  max_bytes: number;
}

export interface LimitsSettings {
  max_file_bytes: number;
  max_total_bytes: number;
}

export interface EngineSettings {
  history: HistoryCleanupSettings;
  file_cache: FileCacheSettings;
  device_name: string;
  rendezvous_url: string;
  limits: LimitsSettings;
  sync: SyncSettings;
  backend_policies: Record<string, BackendSyncPolicy>;
}

export interface SettingsState {
  revision: string;
  value: EngineSettings;
}

export interface FileCacheState {
  stored_bytes: number;
  reserved_bytes: number;
}

export interface HistoryFolder {
  entries: Array<{ path: string; directory: boolean; size: number }>;
  truncated: boolean;
}

export interface EngineStateSnapshot {
  instance_id: string;
  revision: string;
  lifecycle: string;
  runtime: string;
  self: { id: string; name: string };
  settings: SettingsState;
  capabilities: Capabilities;
  devices: EngineDevice[];
  pairing_code: { active: boolean; expires_at_ms?: number };
  operations: EngineOperation[];
  received: ReceivedContent[];
  history_revision: string;
  active_apply?: ActiveApply;
}

export interface ClipboardChangedEvent {
  changeCount?: number;
  hasStrings?: boolean;
  hasImages?: boolean;
}

export interface EngineEvent {
  instance_id: string;
  revision: string;
  at_ms: number;
  kind: string;
  objects?: Array<{ type: string; id: string }>;
}

export type ClipboardAccessMethod = "overlay-polling" | "overlay-event" | "shizuku";

export type BackgroundSyncNetwork = "any" | "wifi";

export interface ClipboardAuthorizationState {
  status: "ready" | "unavailable" | "unauthorized" | "incompatible";
  monitoring_status: "ready" | "setup-required";
  setup_url?: string;
  setup_command?: string;
}

export interface BackgroundSyncStatus {
  enabled: boolean;
  temp_disabled: boolean;
  running: boolean;
  foreground_notification: boolean;
  background_download: boolean;
  background_upload: boolean;
  network: BackgroundSyncNetwork;
  clipboard_access_method: ClipboardAccessMethod;
  ignoring_battery_optimizations: boolean;
  notification_permission_granted: boolean;
  application_id: string;
  last_stop_reason?: string;
  last_stop_at?: number;
  authorization: ClipboardAuthorizationState;
}

export interface BackgroundSyncConfigPatch {
  enabled?: boolean;
  temp_disabled?: boolean;
  foreground_notification?: boolean;
  background_download?: boolean;
  background_upload?: boolean;
  network?: BackgroundSyncNetwork;
  clipboard_access_method?: ClipboardAccessMethod;
}

export interface KeyboardExtensionStatus {
  added: boolean;
  has_full_access_seen: boolean;
  last_seen_at_ms?: number;
  sound_enabled: boolean;
  haptic_enabled: boolean;
  auto_sync_enabled: boolean;
  extension_bundle_id?: string;
}
