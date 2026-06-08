import { commands } from '@/lib/ipc'
import type {
  DaemonBootstrapFailure,
  DaemonConnectionPayload as GeneratedDaemonConnectionPayload,
} from '@/lib/ipc'

const POLL_INTERVAL_MS = 500

/**
 * Upper bound on how long we poll `get_daemon_connection_info` before giving up.
 *
 * The native side sets the daemon connection state only when
 * `bootstrap_daemon_in_process` succeeds. Its worst-case *successful* path
 * (detached spawn + health poll, or terminate-and-replace of an older daemon)
 * completes well under 30s. But when the daemon can never be reached the state
 * stays unset forever — most notably when the running daemon is a strictly
 * newer version and the native side returns `RefusedNewerDaemon` *without*
 * populating the connection state (ADR-008 P4-7 downgrade protection), but also
 * on spawn failure or a health-check timeout.
 *
 * Without a ceiling the poll loops indefinitely and the main window is stuck on
 * a blank loading screen with no way to recover or even learn why. Timing out
 * lets `connectDaemonWs()` reject so the UI can surface an error instead of
 * hanging. The bound is generous (well past the ~30s worst-case success path) so
 * a slow-but-healthy cold start is never misreported as a failure.
 */
const CONNECTION_INFO_TIMEOUT_MS = 60_000

export type DaemonConnectionPayload = GeneratedDaemonConnectionPayload

/**
 * Thrown when `get_daemon_connection_info` never reports a ready daemon within
 * {@link CONNECTION_INFO_TIMEOUT_MS}. Distinct type so callers can tell a
 * connection-bootstrap timeout apart from a malformed-payload / transport error.
 */
export class DaemonConnectionInfoTimeoutError extends Error {
  constructor(timeoutMs: number) {
    super(
      `Timed out after ${Math.round(timeoutMs / 1000)}s waiting for the background ` +
        `service to become reachable. It may have failed to start or be running an ` +
        `incompatible version.`
    )
    this.name = 'DaemonConnectionInfoTimeoutError'
  }
}

/**
 * Thrown when the native daemon bootstrap recorded a terminal failure — the
 * connection state will never be populated, so there is no point polling until
 * the timeout. Carries the classified {@link DaemonBootstrapFailure} so the UI
 * can branch on `failure.kind`: `versionTooOld` → "update the app",
 * `unavailable` → "restart".
 */
export class DaemonBootstrapFailedError extends Error {
  readonly failure: DaemonBootstrapFailure
  constructor(failure: DaemonBootstrapFailure) {
    super(failure.detail || 'The background service failed to start.')
    this.name = 'DaemonBootstrapFailedError'
    this.failure = failure
  }
}

let connectionInfoPromise: Promise<DaemonConnectionPayload> | null = null

export function waitForDaemonConnectionInfo(): Promise<DaemonConnectionPayload> {
  if (connectionInfoPromise) {
    return connectionInfoPromise
  }

  connectionInfoPromise = pollForDaemonConnectionInfo().catch(error => {
    connectionInfoPromise = null
    throw error
  })

  return connectionInfoPromise
}

export function resetDaemonConnectionInfoPollingForTests(): void {
  connectionInfoPromise = null
}

async function pollForDaemonConnectionInfo(): Promise<DaemonConnectionPayload> {
  const deadline = Date.now() + CONNECTION_INFO_TIMEOUT_MS
  while (true) {
    const payload = await commands.getDaemonConnectionInfo()
    if (payload) {
      validatePayload(payload)
      return payload
    }

    // Fail fast on a terminal failure the native bootstrap recorded — the
    // connection state will never be populated (e.g. RefusedNewerDaemon), so
    // there is no point polling until the timeout. This surfaces the typed
    // failure within one poll interval instead of waiting out the ceiling, and
    // lets the UI distinguish "update the app" from "restart".
    const failure = await commands.getDaemonBootstrapFailure()
    if (failure) {
      throw new DaemonBootstrapFailedError(failure)
    }

    // Give up once past the deadline instead of polling forever. `waitFor…`'s
    // catch clears the cached promise, so a later caller (e.g. a manual retry)
    // starts a fresh polling sequence rather than re-throwing this stale error.
    if (Date.now() >= deadline) {
      throw new DaemonConnectionInfoTimeoutError(CONNECTION_INFO_TIMEOUT_MS)
    }

    await sleep(POLL_INTERVAL_MS)
  }
}

function validatePayload(payload: unknown): asserts payload is DaemonConnectionPayload {
  if (
    typeof payload !== 'object' ||
    payload === null ||
    !('baseUrl' in payload) ||
    !('wsUrl' in payload) ||
    typeof (payload as DaemonConnectionPayload).baseUrl !== 'string' ||
    typeof (payload as DaemonConnectionPayload).wsUrl !== 'string' ||
    !(payload as DaemonConnectionPayload).baseUrl ||
    !(payload as DaemonConnectionPayload).wsUrl
  ) {
    throw new Error('Malformed daemon connection payload: missing required fields')
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms))
}
