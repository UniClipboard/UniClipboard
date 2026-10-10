import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect } from 'react'
import { usePlatform } from '@/hooks/usePlatform'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('traffic-lights')

/**
 * Re-applies a macOS traffic-light offset on mount and after every resize
 * (the system resets the buttons after unmaximize / fullscreen changes).
 */
export function useMacTrafficLightPosition(offset: { x: number; y: number }) {
  const { isMac, isTauri } = usePlatform()
  const { x, y } = offset

  useEffect(() => {
    if (!isMac || !isTauri) return
    const sync = () => {
      commands.setTrafficLightPosition(x, y).catch(error => {
        log.error({ err: error }, 'Failed to set traffic light position')
      })
    }
    sync()
    const unlistenPromise = getCurrentWindow().onResized(sync)
    return () => {
      unlistenPromise.then(unlisten => unlisten())
    }
  }, [isMac, isTauri, x, y])
}
