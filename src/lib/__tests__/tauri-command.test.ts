import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invokeWithTrace } from '@/lib/tauri-command'
import { captureDiagnosticException, recordDiagnosticBreadcrumb } from '@/observability/diagnostics'
import { redactSensitiveArgs } from '@/observability/redaction'
import { traceManager } from '@/observability/trace'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('@/observability/trace', () => ({
  traceManager: {
    startTrace: vi.fn(),
    endTrace: vi.fn(),
  },
}))

vi.mock('@/observability/diagnostics', () => ({
  recordDiagnosticBreadcrumb: vi.fn(),
  captureDiagnosticException: vi.fn(),
}))

describe('invokeWithTrace', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('invokes command with trace metadata and args', async () => {
    const trace = { traceId: 'trace-1', startTime: 1234, operation: 'command' }
    const args = { limit: 1, token: 'secret' }
    const safeArgs = redactSensitiveArgs(args)

    vi.mocked(traceManager.startTrace).mockReturnValue(trace)
    vi.mocked(invoke).mockResolvedValueOnce({ ok: true })

    await invokeWithTrace('get_clipboard_entries', args)

    expect(traceManager.startTrace).toHaveBeenCalledWith('get_clipboard_entries')
    expect(recordDiagnosticBreadcrumb).toHaveBeenCalledWith({
      category: 'tauri_command',
      message: 'get_clipboard_entries',
      level: 'info',
      data: { traceId: trace.traceId, args: safeArgs },
    })
    expect(invoke).toHaveBeenCalledWith('get_clipboard_entries', {
      ...args,
      _trace: {
        trace_id: trace.traceId,
        timestamp: trace.startTime,
      },
    })
    expect(traceManager.endTrace).toHaveBeenCalledWith(trace)
  })

  it('redacts sensitive args for sentry breadcrumbs and errors', async () => {
    const trace = { traceId: 'trace-2', startTime: 5678, operation: 'command' }
    const args = { password: 'secret', nested: { token: 'value' } }
    const safeArgs = redactSensitiveArgs(args)
    const error = new Error('boom')

    vi.mocked(traceManager.startTrace).mockReturnValue(trace)
    vi.mocked(invoke).mockRejectedValueOnce(error)

    await expect(invokeWithTrace('set_clipboard', args)).rejects.toThrow('boom')

    expect(recordDiagnosticBreadcrumb).toHaveBeenCalledWith({
      category: 'tauri_command',
      message: 'set_clipboard',
      level: 'info',
      data: { traceId: trace.traceId, args: safeArgs },
    })
    expect(captureDiagnosticException).toHaveBeenCalledWith(error, {
      tags: { command: 'set_clipboard', traceId: trace.traceId },
      extra: { args: safeArgs },
    })
    expect(invoke).toHaveBeenCalledWith('set_clipboard', {
      ...args,
      _trace: {
        trace_id: trace.traceId,
        timestamp: trace.startTime,
      },
    })
    expect(traceManager.endTrace).toHaveBeenCalledWith(trace)
  })

  it('does NOT report expected user/validation errors to Sentry', async () => {
    const trace = { traceId: 'trace-3', startTime: 1, operation: 'command' }
    // A typed-error envelope whose `code` is a known user error — normal
    // product flow, not an alert. (`ValidationError` is classified UserError in
    // `severity.rs` / `USER_FACING_ERROR_CODES`. Mobile-sync's USERNAME_TAKEN
    // moved to the daemon HTTP API in ADR-008 P3-b and no longer flows through
    // this Tauri-command severity path.)
    const userError = { code: 'ValidationError', message: 'bad input' }

    vi.mocked(traceManager.startTrace).mockReturnValue(trace)
    vi.mocked(invoke).mockRejectedValueOnce(userError)

    await expect(invokeWithTrace('update_keyboard_shortcuts')).rejects.toEqual(userError)

    // Still rethrows for the caller to handle, and still leaves a breadcrumb…
    expect(recordDiagnosticBreadcrumb).toHaveBeenCalled()
    // …but no exception is captured.
    expect(captureDiagnosticException).not.toHaveBeenCalled()
    expect(traceManager.endTrace).toHaveBeenCalledWith(trace)
  })

  it('reports unexpected system errors to Sentry', async () => {
    const trace = { traceId: 'trace-4', startTime: 2, operation: 'command' }
    const systemError = { code: 'PERSISTENCE_FAILED', message: 'disk full' }

    vi.mocked(traceManager.startTrace).mockReturnValue(trace)
    vi.mocked(invoke).mockRejectedValueOnce(systemError)

    await expect(invokeWithTrace('register_mobile_device')).rejects.toEqual(systemError)

    expect(captureDiagnosticException).toHaveBeenCalledTimes(1)
    expect(traceManager.endTrace).toHaveBeenCalledWith(trace)
  })
})
