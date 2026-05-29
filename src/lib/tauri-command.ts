import { invoke } from '@tauri-apps/api/core'
import { isExpectedCommandError, toReportableError } from '@/observability/errors'
import { redactSensitiveArgs } from '@/observability/redaction'
import { Sentry } from '@/observability/sentry'
import { traceManager } from '@/observability/trace'

/**
 * Invoke a Tauri command with trace instrumentation, breadcrumb logging, and
 * sanitized argument payloads to avoid leaking sensitive fields.
 *
 * 调用带有追踪、面包屑日志并自动脱敏参数的 Tauri 命令，确保上下文可观测。
 *
 * @param command Command name registered on the Tauri side.
 * @param args Optional argument bag passed to the command; sensitive values will be redacted.
 * @example
 * ```ts
 * await invokeWithTrace('clipboard.fetch_all', { limit: 50 })
 * ```
 */
export async function invokeWithTrace<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  const trace = traceManager.startTrace(command)
  const safeArgs = redactSensitiveArgs(args)

  Sentry.addBreadcrumb({
    category: 'tauri_command',
    message: command,
    level: 'info',
    data: { traceId: trace.traceId, args: safeArgs },
  })

  try {
    return await invoke<T>(command, {
      ...args,
      _trace: {
        trace_id: trace.traceId,
        timestamp: trace.startTime,
      },
    })
  } catch (error) {
    // Skip expected user/validation errors (see `isExpectedCommandError`) so
    // input-validation rejections don't raise Sentry alerts. The breadcrumb
    // above still records the invocation for context on real failures.
    if (!isExpectedCommandError(error)) {
      Sentry.captureException(toReportableError(error, command), {
        tags: { command, traceId: trace.traceId },
        extra: { args: safeArgs },
      })
    }
    throw error
  } finally {
    traceManager.endTrace()
  }
}
