import { listen } from '@tauri-apps/api/event'
import { useEffect } from 'react'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-ui-navigate-listener')

const ALLOWED_ROUTES = ['/settings']

/**
 * Navigation requests from the backend arrive over two channels:
 *
 * 1. A live `ui://navigate` event — works when this webview was already
 *    running when the tray menu fired it.
 * 2. A pending route recorded in native state — the tray "Settings" item may
 *    have to recreate a destroyed main window (destroy-on-close model), and
 *    an event emitted at that moment races this listener's registration. The
 *    backend records the route instead; we drain it once on mount.
 *
 * The tray records AND emits on every click, so after handling a live event
 * we discard the pending record to keep it from leaking into the next boot
 * (which must land on the home route).
 */
export function useUINavigateListener(onNavigate: (route: string) => void) {
  useEffect(() => {
    let cancelled = false

    commands
      .takePendingNavigation()
      .then(route => {
        if (cancelled || !route) return
        if (ALLOWED_ROUTES.includes(route)) {
          onNavigate(route)
        } else {
          log.warn({ route }, 'Blocked pending navigation to non-whitelisted route')
        }
      })
      .catch(error => {
        log.warn({ error }, 'Failed to drain pending navigation')
      })

    const unlistenPromise = listen<string>('ui://navigate', event => {
      const route = event.payload
      // Discard the duplicate pending record for this click (see above).
      commands.takePendingNavigation().catch(() => {})
      if (ALLOWED_ROUTES.includes(route)) {
        onNavigate(route)
      } else {
        log.warn({ route }, 'Blocked navigation to non-whitelisted route')
      }
    })

    return () => {
      cancelled = true
      unlistenPromise.then(unlisten => unlisten())
    }
  }, [onNavigate])
}
