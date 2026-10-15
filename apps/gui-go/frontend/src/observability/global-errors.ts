import { createLogger } from '@/lib/logger'
import { serializeUnknown } from '@/observability/serialize-error'

const log = createLogger('global-errors')
const installedTargets = new WeakSet<EventTarget>()

/**
 * Route uncaught errors and unhandled promise rejections through the
 * application logger so they reach the diagnostics log with a full payload.
 * Idempotent per target; call once at app bootstrap. Does not call
 * `preventDefault`, so the browser console and Sentry's own handlers still run.
 */
export function installGlobalErrorHandlers(target: EventTarget = window): void {
  if (installedTargets.has(target)) return
  installedTargets.add(target)

  target.addEventListener('unhandledrejection', event => {
    log.error(
      { reason: serializeUnknown((event as PromiseRejectionEvent).reason) },
      'Unhandled promise rejection'
    )
  })

  target.addEventListener('error', event => {
    const e = event as ErrorEvent
    log.error(
      {
        error: serializeUnknown(e.error ?? e.message),
        filename: e.filename,
        lineno: e.lineno,
        colno: e.colno,
      },
      'Uncaught error'
    )
  })
}
