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
import { daemonClient } from '@/api/daemon/client'
import { waitForDaemonConnectionInfo } from '@/lib/daemon-connection-info'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'

const log = createLogger('daemon-ws-bootstrap')

let connectionEstablished = false
let connectionPromise: Promise<void> | null = null

/**
 * Connect the frontend WebSocket client to the daemon.
 *
 * Idempotent — safe to call multiple times. Returns immediately if already connected.
 * The full bootstrap sequence is:
 *   1. Poll `get_daemon_connection_info` until the daemon reports ready.
 *   2. Initialize `daemonClient` with the received connection config.
 *   3. Exchange the bearer token for a JWT session via POST /auth/connect.
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
        token: payload.token,
        pid: 0,
      })

      // Step 2: Exchange the bearer token for a JWT session before opening the WebSocket.
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
  connectionEstablished = false
  connectionPromise = null
}
