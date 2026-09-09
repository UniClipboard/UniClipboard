import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { commands } from '@/lib/ipc'
import type { DesktopTheme, DesktopThemeSnapshot } from '@/lib/ipc-bindings.generated'
import { createLogger } from '@/lib/logger'

export type { DesktopTheme } from '@/lib/ipc-bindings.generated'
const log = createLogger('desktop-theme')

/** Subscribe before querying so startup and hidden windows cannot miss a change. */
export function subscribeDesktopTheme(onTheme: (theme: DesktopTheme | null) => void): () => void {
  if (!isTauri()) return () => {}
  let disposed = false
  let revision = -1
  let unlisten: (() => void) | undefined
  const accept = (snapshot: DesktopThemeSnapshot) => {
    if (disposed || snapshot.revision <= revision) return
    revision = snapshot.revision
    onTheme(snapshot.theme)
  }
  const refresh = async () => {
    try {
      accept(await commands.getDesktopTheme())
    } catch (err) {
      if (!disposed) log.error({ err }, 'Failed to read desktop theme')
    }
  }
  const handleVisibility = () => {
    if (document.visibilityState === 'visible') void refresh()
  }
  void (async () => {
    try {
      unlisten = await listen<DesktopThemeSnapshot>('desktop-theme://changed', event => {
        accept(event.payload)
      })
      if (disposed) {
        unlisten()
        return
      }
      document.addEventListener('visibilitychange', handleVisibility)
      await refresh()
    } catch (err) {
      if (!disposed) log.error({ err }, 'Failed to subscribe to desktop theme')
    }
  })()
  return () => {
    disposed = true
    document.removeEventListener('visibilitychange', handleVisibility)
    unlisten?.()
  }
}
