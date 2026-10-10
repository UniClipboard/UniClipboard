// Daemon startup progress payload (`GET /startup`).
//
// Owner: the daemon contract (crates/uc-daemon-contract/src/startup.rs). That route is not part of the OpenAPI document
// (schema/openapi.json), so there is no generated TypeScript for it and these declarations are a hand-kept mirror of
// the Rust DTOs. The Go host passes the payload through untouched (`GetDaemonStartupStatus` returns opaque JSON), and
// `lib/ipc.ts` narrows the command result to `DaemonStartupStatus`. If the route ever joins the OpenAPI document,
// delete this file and import the generated types instead.

export type DaemonStartupStatus = {
  package_version: string
  service_ready: boolean
  service_failed: boolean
  progress: StartupSnapshotDto
}

export type StartupActionsDto = {
  retry: boolean
  export_diagnostics: boolean
}

export type StartupFailureDto = {
  reason: StartupFailureReasonDto
  retryable: boolean
}

export type StartupFailureReasonDto =
  | 'backup_failed'
  | 'storage_full'
  | 'permission_denied'
  | 'storage_unavailable'
  | 'protection_unavailable'
  | 'corrupt_data'
  | 'source_changed'
  | 'already_running'
  | 'startup_failed'

export type StartupSnapshotDto = {
  attempt_id: string
  sequence: number
  state: StartupStateDto
  elapsed_ms: number
  upgrade: StartupUpgradeDto | null
  failure: StartupFailureDto | null
  allowed_actions: StartupActionsDto
}

export type StartupStateDto =
  | 'preparing'
  | 'upgrading'
  | 'starting_services'
  | 'ready'
  | 'recovery_available'
  | 'failed'
  | 'interrupted'

export type StartupStepDto =
  | 'backing_up'
  | 'checking'
  | 'converting_contents'
  | 'converting_large_contents'
  | 'converting_related_records'
  | 'verifying'
  | 'preparing'

export type StartupStepProgressDto = {
  step: StartupStepDto
  processed: number
  total: number | null
  unit: StartupUnitDto | null
  warning_count: number | null
  completed: boolean
}

export type StartupUnitDto = 'content_representations' | 'large_contents' | 'related_records'

export type StartupUpgradeDto = {
  required: boolean
  recovering: boolean
  completed: boolean
  current_step: StartupStepDto | null
  steps: StartupStepProgressDto[]
}
