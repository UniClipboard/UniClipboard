import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef } from 'react'
import { getSettings } from '@/api/daemon'
import { createLogger } from '@/lib/logger'
import { parseSettingsChangedPayload, SETTINGS_CHANGED_EVENT } from '@/lib/settings-events'
import { applyThemeOverrides, applyThemePreset, DEFAULT_THEME_COLOR } from '@/lib/theme-engine'
import type { ThemeMode } from '@/lib/theme-engine'
import type { SettingChangedEvent } from '@/types/events'
import type { Settings } from '@/types/setting'

const log = createLogger('use-theme-sync')

function resolveThemeMode(theme: string | undefined | null): ThemeMode {
  if (theme === 'light' || theme === 'dark') return theme
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function applyFullTheme(settings: Settings | null): void {
  const root = document.documentElement
  const theme = settings?.general?.theme
  const resolvedMode = resolveThemeMode(theme)
  // light / dark 各自的预设；缺失时回退到旧 themeColor,再回退到引擎默认。
  const split =
    resolvedMode === 'dark' ? settings?.general?.themeColorDark : settings?.general?.themeColorLight
  const themeColor = split || settings?.general?.themeColor || DEFAULT_THEME_COLOR
  const overrides =
    resolvedMode === 'dark'
      ? settings?.general?.themeOverridesDark
      : settings?.general?.themeOverridesLight

  root.classList.remove('light', 'dark')
  root.classList.add(resolvedMode)
  applyThemePreset(themeColor, resolvedMode, root)
  applyThemeOverrides(overrides ?? null, root)
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
        log.error({ err }, 'Failed to load settings for theme')
        applyFullTheme(null)
      })

    const unlistenPromise = listen<SettingChangedEvent>(SETTINGS_CHANGED_EVENT, event => {
      const nextSettings = parseSettingsChangedPayload(event.payload)
      if (!nextSettings) return

      settingsRef.current = nextSettings
      applyFullTheme(nextSettings)
    }).catch(err => {
      if (!cancelled) {
        log.error({ err }, 'Failed to subscribe to settings changes for theme sync')
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
