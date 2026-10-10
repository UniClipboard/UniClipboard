import { getCurrentWindow } from '@tauri-apps/api/window'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { usePlatform } from '@/hooks/usePlatform'
import { createLogger } from '@/lib/logger'

const log = createLogger('window-controls')

/**
 * Minimize / maximize / close actions for the app-drawn window frame, plus
 * the maximized state that decides the maximize button's label.
 */
export function useWindowControls() {
  const { isTauri } = usePlatform()
  const [isMaximized, setIsMaximized] = useState(false)
  const windowRef = useMemo(() => (isTauri ? getCurrentWindow() : null), [isTauri])

  useEffect(() => {
    if (!windowRef) return

    let mounted = true
    windowRef.isMaximized().then(value => {
      if (mounted) setIsMaximized(value)
    })

    const unlistenPromise = windowRef.onResized(async () => {
      if (!mounted) return
      setIsMaximized(await windowRef.isMaximized())
    })

    return () => {
      mounted = false
      unlistenPromise.then(unlisten => unlisten())
    }
  }, [windowRef])

  const minimize = useCallback(async () => {
    if (!windowRef) return
    try {
      await windowRef.minimize()
    } catch (error) {
      log.error({ err: error }, 'Minimize failed')
    }
  }, [windowRef])

  const toggleMaximize = useCallback(async () => {
    if (!windowRef) return
    try {
      const maximized = await windowRef.isMaximized()
      if (maximized) {
        await windowRef.unmaximize()
      } else {
        await windowRef.maximize()
      }
      setIsMaximized(!maximized)
    } catch (error) {
      log.error({ err: error }, 'Toggle maximize failed')
    }
  }, [windowRef])

  const close = useCallback(async () => {
    if (!windowRef) return
    try {
      await windowRef.close()
    } catch (error) {
      log.error({ err: error }, 'Close failed')
    }
  }, [windowRef])

  return { isMaximized, minimize, toggleMaximize, close }
}
