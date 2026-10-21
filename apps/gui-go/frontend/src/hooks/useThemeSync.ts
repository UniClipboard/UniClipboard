import { useEffect, useLayoutEffect, useRef } from 'react'
import { subscribeSettings } from '@/lib/settings-sync'
import { createWindowThemeController } from '@/lib/window-theme'

/**
 * Theme the entire window, including startup and error surfaces before settings are available.
 * The theme follows the window's authoritative settings copy (`@/lib/settings-sync`), which reads
 * the daemon after its own snapshot and after every settings change.
 */
export function useThemeSync(settingsReady = true): void {
  const themeRef = useRef<ReturnType<typeof createWindowThemeController> | null>(null)

  useLayoutEffect(() => {
    const theme = createWindowThemeController()
    themeRef.current = theme
    // Apply the system appearance before the first paint; desktop updates do not need the daemon.
    theme.setGeneral(null)
    return () => {
      themeRef.current = null
      theme.dispose()
    }
  }, [])

  useEffect(() => {
    if (!settingsReady) return
    return subscribeSettings(({ settings }) => {
      if (settings) themeRef.current?.setGeneral(settings.general)
    })
  }, [settingsReady])
}
