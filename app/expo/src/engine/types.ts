export type EngineErrorCode =
  | "invalid_argument"
  | "invalid_state"
  | "cancelled"
  | "not_found"
  | "conflict"
  | "in_use"
  | "permission_denied"
  | "identity_mismatch"
  | "timeout"
  | "unavailable"
  | "suspended"
  | "unsupported"
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

export interface SetupFlowState {
  complete: boolean;
  source: string;
  default_source: string;
}

export interface SourceDetectionResult {
  status: "none" | "found" | "invalid" | "error";
  source: string;
}

export interface CustomSourceSelectionResult {
  cancelled: boolean;
  source?: string;
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

export interface SensitiveTicket {
  ticket: string;
  expires_at_ms: number;
}

export interface EngineOperation {
  id: string;
  request_id: string;
  kind: string;
  created_at_ms: number;
  updated_at_ms: number;
  phase: "running" | "succeeded" | "completed" | "failed" | string;
  error?: EngineErrorPayload;
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
  kind: "text" | "html" | "image" | "file" | "mixed" | string;
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

export interface ImagePreviewResult {
  mime?: string;
  name?: string;
  bytes_written?: number;
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

export interface EngineStateSnapshot {
  instance_id: string;
  revision: string;
  lifecycle: string;
  runtime: string;
  self: { id: string; name: string };
  settings: {
    device_name?: string;
    sync?: { auto_sync?: boolean };
  };
  capabilities: Capabilities;
  devices: EngineDevice[];
  pairing_code: { active: boolean; expires_at_ms?: number };
  operations: EngineOperation[];
  history_revision: string;
}
