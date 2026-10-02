import { listen } from '@tauri-apps/api/event'
import { useEffect, useLayoutEffect, useRef } from 'react'
import { getSettings } from '@/api/daemon'
import { createLogger } from '@/lib/logger'
import { parseSettingsChangedPayload, SETTINGS_CHANGED_EVENT } from '@/lib/settings-events'
import { createWindowThemeController } from '@/lib/window-theme'
import type { SettingChangedEvent } from '@/types/events'

const log = createLogger('use-theme-sync')

/** Theme the entire window, including startup and error surfaces before settings are available. */
export function useThemeSync(settingsReady = true): void {
  const sessionRef = useRef<{
    theme: ReturnType<typeof createWindowThemeController>
    revision: number
  } | null>(null)

  useLayoutEffect(() => {
    let cancelled = false
    const session = { theme: createWindowThemeController(), revision: 0 }
    sessionRef.current = session
    // Apply the system appearance before the first paint; desktop updates do not need the daemon.
    session.theme.setGeneral(null)

    const unlistenPromise = listen<SettingChangedEvent>(SETTINGS_CHANGED_EVENT, event => {
      if (cancelled) return
      const nextSettings = parseSettingsChangedPayload(event.payload)
      if (!nextSettings) return
      session.revision += 1
      session.theme.setGeneral(nextSettings.general)
    }).catch(err => {
      if (!cancelled) {
        log.error({ err }, 'Failed to subscribe to settings changes for theme sync')
      }
      return () => {}
    })

    return () => {
      cancelled = true
      sessionRef.current = null
      session.theme.dispose()
      void unlistenPromise.then(unlisten => unlisten())
    }
  }, [])

  useEffect(() => {
    const session = sessionRef.current
    if (!settingsReady || !session) return
    let cancelled = false
    const revision = session.revision
    void getSettings()
      .then(settings => {
        // A live settings update takes precedence over an older startup response.
        if (!cancelled && session.revision === revision) session.theme.setGeneral(settings.general)
      })
      .catch(err => {
        if (!cancelled) log.error({ err }, 'Failed to load settings for theme')
      })
    return () => {
      cancelled = true
    }
  }, [settingsReady])
}
