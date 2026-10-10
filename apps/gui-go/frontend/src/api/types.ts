// Shared DTO and command contract types for Tauri IPC boundary
// 与后端约定保持同步的数据传输对象和命令错误类型

// Lifecycle status DTO shared with the desktop host.
// 与桌面宿主共享的生命周期状态 DTO。
export type LifecycleState = 'Idle' | 'Pending' | 'Ready' | 'WatcherFailed' | 'NetworkFailed'

export interface LifecycleStatusDto {
  state: LifecycleState
  pendingReason?: 'space_locked' | 'membership_recovery'
}

// `GET /lifecycle/status` now returns the canonical `{ data, ts }` envelope
// (ADR-008 §H). The lifecycle state lives inside `data`; readers extract
// `envelope.data.state`.
export interface LifecycleStatusEnvelope {
  data: LifecycleStatusDto
  ts: number
}

// CommandError serialization uses serde `tag = "code", content = "message"`.
// 在前端表现为 { code: string, message: string } 判别联合。
export type CommandErrorCode =
  | 'NotFound'
  | 'InternalError'
  | 'Timeout'
  | 'Cancelled'
  | 'ValidationError'
  | 'Conflict'
  | 'AccessibilityPermissionRequired'

export interface CommandError {
  code: CommandErrorCode
  message: string
}
