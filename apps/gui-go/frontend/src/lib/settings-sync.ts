/**
 * The window's authoritative copy of the daemon settings (ADR-013 section 9).
 *
 * - Follows the daemon WS topics `settings` and `system`. The first read waits for
 *   `settings.snapshot`. Every snapshot (first subscribe and every reconnect), `settings.changed`
 *   and `system.refresh_required` causes a `GET /settings`. Revisions are never compared: a
 *   restarted daemon starts again at 0.
 * - Reads and this window's writes run one at a time in one queue, so a read result never
 *   replaces a newer one. Read requests made while a read waits in the queue share it.
 * - Every write is followed by a read, also when the write failed, so the copy always shows what
 *   the daemon stored before the caller sees the result.
 *
 * Nothing else in the page is an authority for settings: other windows and the Go host follow the
 * daemon themselves.
 */
import { getSettings, type Settings } from '@/api/daemon'
import { daemonWs, type DaemonWsEvent } from '@/lib/daemon-ws'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { createLogger } from '@/lib/logger'

const log = createLogger('settings-sync')

const READ_TRIGGERS = new Set(['settings.snapshot', 'settings.changed', 'system.refresh_required'])

export interface SettingsSyncState {
  settings: Settings | null
  /** The last read failed; `settings` keeps the last good copy. */
  error: unknown
}

type Listener = (state: SettingsSyncState) => void

let state: SettingsSyncState = { settings: null, error: null }
const listeners = new Set<Listener>()
let queue: Promise<unknown> = Promise.resolve()
let queuedRead: Promise<Settings> | null = null
let stopFollowing: (() => void) | null = null

function publish(next: SettingsSyncState): void {
  state = next
  for (const listener of listeners) listener(state)
}

function enqueue<T>(task: () => Promise<T>): Promise<T> {
  const run = queue.then(task)
  queue = run.catch(() => undefined)
  return run
}

async function read(): Promise<Settings> {
  try {
    const settings = await getSettings()
    publish({ settings, error: null })
    return settings
  } catch (err) {
    publish({ settings: state.settings, error: err })
    throw err
  }
}

/** Read the daemon settings after everything already queued. */
export function refreshSettings(): Promise<Settings> {
  if (queuedRead) return queuedRead
  const pending = enqueue(() => {
    queuedRead = null
    return read()
  })
  queuedRead = pending
  return pending
}

/**
 * Run a settings write in the window's queue, then read the stored state, also when the write
 * failed. The write's own result or error is returned after the read.
 */
export function writeSettings<T>(write: () => Promise<T>): Promise<T> {
  return enqueue(async () => {
    let result: T
    try {
      result = await write()
    } catch (err) {
      await read().catch(readErr => {
        log.warn({ err: readErr }, 'Failed to read settings after a failed save')
      })
      throw err
    }
    await read().catch(readErr => {
      log.warn({ err: readErr }, 'Failed to read settings after a save')
    })
    return result
  })
}

function onDaemonEvent(event: DaemonWsEvent): void {
  if (READ_TRIGGERS.has(event.eventType)) {
    void refreshSettings().catch(err => log.warn({ err }, 'Failed to refresh settings'))
  }
}

function follow(): void {
  if (stopFollowing) return
  // `settings.snapshot` is the read gate: the daemon sends it once the subscription is active,
  // on the first subscribe and on the re-subscribe after every reconnect. A read made only after
  // it cannot miss a write that lands between the read and the subscription.
  stopFollowing = daemonWs.subscribe(['settings', 'system'], onDaemonEvent)
  void connectDaemonWs().catch(err => {
    // Without a socket no snapshot comes; read once so the window shows the load error or,
    // when HTTP still works, the stored state.
    log.warn({ err }, 'Failed to connect the daemon socket')
    return refreshSettings().catch(readErr => log.warn({ err: readErr }, 'Failed to load settings'))
  })
}

/** Current copy. */
export function getSettingsSyncState(): SettingsSyncState {
  return state
}

/**
 * Listen to the window's copy. The first listener connects, follows the daemon and reads;
 * the listener is called on every read, so it must be idempotent.
 */
export function subscribeSettings(listener: Listener): () => void {
  listeners.add(listener)
  if (!stopFollowing) {
    follow()
  } else if (state.settings) {
    listener(state)
  }
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0 && stopFollowing) {
      stopFollowing()
      stopFollowing = null
    }
  }
}

/** Reset module state between test cases. */
export function resetSettingsSyncForTest(): void {
  stopFollowing?.()
  stopFollowing = null
  listeners.clear()
  state = { settings: null, error: null }
  queue = Promise.resolve()
  queuedRead = null
}
