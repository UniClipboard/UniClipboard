import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef } from 'react'
import { invokeWithTrace } from '@/lib/tauri-command'
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
  const themeColor = settings?.general?.theme_color || DEFAULT_THEME_COLOR
  const resolvedMode = resolveThemeMode(theme)

  root.classList.remove('light', 'dark')
  root.classList.add(resolvedMode)
  applyThemePreset(themeColor, resolvedMode, root)
}

export function useThemeSync(): void {
  const settingsRef = useRef<Settings | null>(null)

  useEffect(() => {
    let cancelled = false

    void invokeWithTrace<Settings>('get_settings')
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

    const unlistenSettings = listen<SettingChangedEvent>('setting-changed', event => {
      try {
        const newSettings = JSON.parse(event.payload.settingJson) as Settings
        settingsRef.current = newSettings
        applyFullTheme(newSettings)
      } catch (err) {
        console.error('Failed to parse setting-changed event:', err)
      }
    })

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
      unlistenSettings.then(fn => fn())
      mediaQuery.removeEventListener('change', handleSystemChange)
    }
  }, [])
}
