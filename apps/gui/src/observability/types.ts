/** Application-owned diagnostics contracts. Provider SDK types stay private. */
export interface DiagnosticExceptionContext {
  tags?: Record<string, string>
  extra?: Record<string, unknown>
}

export interface DiagnosticBreadcrumb {
  category: string
  message: string
  level?: 'debug' | 'info' | 'warning' | 'error' | 'fatal'
  data?: Record<string, unknown>
}

export interface DiagnosticFeedback {
  message: string
  email?: string
}

export interface DiagnosticTrace {
  traceId: string
  finish(): void
}

export type DiagnosticLogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error' | 'fatal'

export interface DiagnosticDeviceContext {
  deviceId: string
  deviceRole: string
  platform: string
  appVersion: string
  appChannel: string
}
