/**
 * Daemon WS Bootstrap — connects the frontend WebSocket client to the daemon.
 *
 * Polls the `get_daemon_connection_info` Tauri command until the daemon is
 * ready, then:
 *   1. Initializes `daemonClient` with the connection config.
 *   2. Connects `daemonWs` to the daemon's WebSocket endpoint.
 *
 * After this runs, `daemonWs` will maintain its own connection with automatic
 * reconnect (exponential backoff, max 10 attempts). All `daemonWs.subscribe()`
 * calls in hooks will automatically receive events once connected.
 */
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { daemonClient } from '@/api/daemon/client'
import { waitForDaemonConnectionInfo } from '@/lib/daemon-connection-info'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'

const log = createLogger('daemon-ws-bootstrap')

/** Tauri event the Rust shell emits right before tearing down the in-process
 *  daemon — see `src-tauri/crates/uc-tauri/src/run.rs::FRONTEND_SHUTDOWN_EVENT`. */
const APP_SHUTDOWN_EVENT = 'app://shutting-down'

let connectionEstablished = false
let connectionPromise: Promise<void> | null = null
let shutdownListenerUnlisten: UnlistenFn | null = null

/** Reset the bootstrap state so the next `connectDaemonWs()` call re-fetches
 *  connection_info + refreshes the JWT session. Used by the test helper. */
function resetConnectionState(): void {
  connectionEstablished = false
  connectionPromise = null
}

/**
 * Connect the frontend WebSocket client to the daemon.
 *
 * Idempotent — safe to call multiple times. Returns immediately if already connected.
 * The full bootstrap sequence is:
 *   1. Poll `get_daemon_connection_info` until the daemon reports ready.
 *   2. Initialize `daemonClient` with the received connection config.
 *   3. Ask native Tauri for a JWT session.
 *   4. Open the WebSocket with the session token in the URL.
 *
 * WebSocket connect never starts before step 3 is complete, ensuring the daemon
 * receives an authenticated session on the first connection attempt.
 *
 * @returns Promise that resolves when the WebSocket is open,
 *          or immediately if already connected.
 */
export function connectDaemonWs(): Promise<void> {
  if (connectionEstablished) {
    return Promise.resolve()
  }

  if (connectionPromise) {
    return connectionPromise
  }

  connectionPromise = waitForDaemonConnectionInfo()
    .then(async payload => {
      // Step 1: Initialize the HTTP client with the connection config.
      daemonClient.initialize({
        baseUrl: payload.baseUrl,
        wsUrl: payload.wsUrl,
      })

      // Step 2: Receive a short-lived daemon session before opening the WebSocket.
      // This is the critical ordering requirement — daemonWs._openSocket() reads
      // daemonClient.currentSession.token to build the auth URL, so the session
      // must exist before connect() is called.
      await daemonClient.refreshSession()

      // Step 3: Connect the WebSocket client. daemonWs will auto-reconnect on disconnect.
      await daemonWs.connect(payload.wsUrl)
    })
    .then(() => {
      connectionEstablished = true
      log.info('connected to daemon WebSocket')
    })
    .catch(err => {
      log.error({ err }, 'failed to connect to daemon WebSocket')
      throw err
    })
    .finally(() => {
      if (!connectionEstablished) {
        connectionPromise = null
      }
    })

  return connectionPromise
}

/**
 * Reset the module-level `connectionEstablished` flag.
 * Exported for test use only — do not call in production.
 */
export function resetConnectDaemonWsForTests(): void {
  resetConnectionState()
}

/**
 * Subscribe to the `app://shutting-down` Tauri event so the WebSocket
 * disconnects cleanly *before* the Rust shell tears down the daemon.
 *
 * Without this, axum's `with_graceful_shutdown` on daemon side would wait
 * for the long-lived `/ws` handler to finish — and browser WebSocket
 * clients don't send a close frame when the webview is destroyed, so the
 * daemon would hang on its 30s heartbeat timeout before shutting down.
 *
 * Idempotent — calling more than once is a no-op (the existing listener
 * stays installed). Safe to call before `connectDaemonWs()` resolves.
 */
export async function registerDaemonShutdownListener(): Promise<void> {
  if (shutdownListenerUnlisten) {
    return
  }
  try {
    shutdownListenerUnlisten = await listen(APP_SHUTDOWN_EVENT, () => {
      log.info('received app://shutting-down — disconnecting daemon WebSocket')
      daemonWs.disconnect()
    })
  } catch (err) {
    log.warn(
      { err },
      'failed to register app://shutting-down listener; daemon shutdown ' +
        'will fall back to heartbeat-driven WS disconnect'
    )
  }
}
