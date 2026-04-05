import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef } from 'react'
import { getSettings } from '@/api/daemon'
import { parseSettingsChangedPayload, SETTINGS_CHANGED_EVENT } from '@/lib/settings-events'
import { applyThemePreset, DEFAULT_THEME_COLOR } from '@/lib/theme-engine'
import type { ThemeMode } from '@/lib/theme-engine'
import type { SettingChangedEvent } from '@/types/events'
import type { Settings } from '@/types/setting'

function resolveThemeMode(theme: string | undefined | null): ThemeMode {
  if (theme === 'light' || theme === 'dark') return theme
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function applyFullTheme(settings: Settings | null): void {
  const root = document.documentElement
  const theme = settings?.general?.theme
  const themeColor = settings?.general?.themeColor || DEFAULT_THEME_COLOR
  const resolvedMode = resolveThemeMode(theme)

  root.classList.remove('light', 'dark')
  root.classList.add(resolvedMode)
  applyThemePreset(themeColor, resolvedMode, root)
}

export function useThemeSync(): void {
  const settingsRef = useRef<Settings | null>(null)

  useEffect(() => {
    let cancelled = false

    // Load initial theme from daemon settings API
    void getSettings()
      .then(settings => {
        if (cancelled) return
        settingsRef.current = settings
        applyFullTheme(settings)
      })
      .catch(err => {
        if (cancelled) return
        console.error('Failed to load settings for theme:', err)
        applyFullTheme(null)
      })

    const unlistenPromise = listen<SettingChangedEvent>(SETTINGS_CHANGED_EVENT, event => {
      const nextSettings = parseSettingsChangedPayload(event.payload)
      if (!nextSettings) return

      settingsRef.current = nextSettings
      applyFullTheme(nextSettings)
    }).catch(err => {
      if (!cancelled) {
        console.error('Failed to subscribe to settings changes for theme sync:', err)
      }
      return () => {}
    })

    // Watch for system theme changes when user prefers 'system' theme
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
    const handleSystemChange = () => {
      const settings = settingsRef.current
      if (!settings?.general?.theme || settings.general.theme === 'system') {
        applyFullTheme(settings)
      }
    }

    mediaQuery.addEventListener('change', handleSystemChange)

    return () => {
      cancelled = true
      mediaQuery.removeEventListener('change', handleSystemChange)
      void unlistenPromise.then(unlisten => unlisten())
    }
  }, [])
}
