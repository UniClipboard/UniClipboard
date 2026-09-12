import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { commands } from '@/lib/ipc'
import type { DesktopTheme, DesktopThemeSnapshot } from '@/lib/ipc-bindings.generated'
import { createLogger } from '@/lib/logger'

export type { DesktopTheme } from '@/lib/ipc-bindings.generated'
const log = createLogger('desktop-theme')
// Retain only in memory so the React owner can reuse the palette applied before mounting.
let latestSnapshot: DesktopThemeSnapshot | undefined

/** Subscribe before querying so startup and hidden windows cannot miss a change. */
export function subscribeDesktopTheme(
  onTheme: (theme: DesktopTheme | null, windowCornerRadius?: number | null) => void
): () => void {
  if (!isTauri()) {
    onTheme(null)
    return () => {}
  }
  let disposed = false
  let revision = latestSnapshot?.revision ?? -1
  if (latestSnapshot) onTheme(latestSnapshot.theme, latestSnapshot.windowCornerRadius)
  let unlisten: (() => void) | undefined
  const accept = (snapshot: DesktopThemeSnapshot) => {
    if (disposed || snapshot.revision <= revision) return
    revision = snapshot.revision
    latestSnapshot = snapshot
    onTheme(snapshot.theme, snapshot.windowCornerRadius)
  }
  const refresh = async () => {
    try {
      accept(await commands.getDesktopTheme())
    } catch (err) {
      if (!disposed) {
        log.error({ err }, 'Failed to read desktop theme')
        if (revision < 0) onTheme(null)
      }
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
      if (!disposed) {
        log.error({ err }, 'Failed to subscribe to desktop theme')
        await refresh()
      }
    }
  })()
  return () => {
    disposed = true
    document.removeEventListener('visibilitychange', handleVisibility)
    unlisten?.()
  }
}
