import React, { useCallback, useEffect, useState, type ReactNode } from 'react'
import { SettingContext } from './setting-context'
import { getSettings, updateSettings } from '@/api/daemon'
import { DEFAULT_THEME_COLOR } from '@/constants/theme'
import i18n, { normalizeLanguage, persistLanguage } from '@/i18n'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { emitSettingsChanged } from '@/lib/settings-events'
import { invokeWithTrace } from '@/lib/tauri-command'
import { applyThemePreset } from '@/lib/theme-engine'
import { startThemeTransition } from '@/lib/theme-transition'
import { setFrontendTelemetryEnabled } from '@/observability/otlp'
import type { SettingContextType, Settings } from '@/types/setting'

// 设置提供者属性接口
interface SettingProviderProps {
  children: ReactNode
}

// 设置提供者组件
export const SettingProvider: React.FC<SettingProviderProps> = ({ children }) => {
  const [setting, setSetting] = useState<Settings | null>(null)
  const [loading, setLoading] = useState<boolean>(true)
  const [error, setError] = useState<string | null>(null)

  // 加载设置
  const loadSetting = useCallback(async () => {
    try {
      setLoading(true)
      // Ensure daemon is connected before making API calls — the connection may not
      // have been established yet if this fires before AppContent calls connectDaemonWs().
      await connectDaemonWs()
      const settingObj = await getSettings()
      setSetting(settingObj)
      setError(null)
    } catch (err) {
      console.error('加载设置失败:', err)
      setError(`加载设置失败: ${err}`)
    } finally {
      setLoading(false)
    }
  }, [])

  // 保存设置
  const saveSetting = async (newSetting: Settings) => {
    try {
      setLoading(true)
      await updateSettings(newSetting)
      setSetting(newSetting)
      setError(null)
      try {
        await emitSettingsChanged(newSetting)
      } catch (err) {
        console.error('Failed to broadcast settings change:', err)
      }
    } catch (err) {
      console.error('保存设置失败:', err)
      setError(`保存设置失败: ${err}`)
      throw err // 重新抛出错误，让调用者可以处理
    } finally {
      setLoading(false)
    }
  }

  // 更新整个设置
  const updateSetting = async (newSetting: Settings) => {
    await saveSetting(newSetting)
  }

  // 更新通用设置
  const updateGeneralSetting = async (newGeneralSetting: Partial<Settings['general']>) => {
    if (!setting) return
    const updatedSetting: Settings = {
      ...setting,
      general: {
        ...setting.general,
        ...newGeneralSetting,
      },
    } as Settings
    await saveSetting(updatedSetting)
  }

  // 更新同步设置
  const updateSyncSetting = async (newSyncSetting: Partial<Settings['sync']>) => {
    if (!setting) return
    const updatedSetting: Settings = {
      ...setting,
      sync: {
        ...setting.sync,
        ...newSyncSetting,
      },
    } as Settings
    await saveSetting(updatedSetting)
  }

  // 更新安全设置
  const updateSecuritySetting = async (newSecuritySetting: Partial<Settings['security']>) => {
    if (!setting) return
    const updatedSetting: Settings = {
      ...setting,
      security: {
        ...setting.security,
        ...newSecuritySetting,
      },
    } as Settings
    await saveSetting(updatedSetting)
  }

  // 更新保留策略
  const updateRetentionPolicy = async (newPolicy: Partial<Settings['retentionPolicy']>) => {
    if (!setting) return
    const updatedSetting: Settings = {
      ...setting,
      retentionPolicy: {
        ...setting.retentionPolicy,
        ...newPolicy,
      },
    } as Settings
    await saveSetting(updatedSetting)
  }

  // Update file sync settings
  const updateFileSyncSetting = async (
    newFileSyncSetting: Partial<Settings['fileSync'] & object>
  ) => {
    if (!setting) return
    const updatedSetting: Settings = {
      ...setting,
      fileSync: {
        ...(setting.fileSync ?? {
          fileSyncEnabled: true,
          smallFileThreshold: 10 * 1024 * 1024,
          maxFileSize: 5 * 1024 * 1024 * 1024,
          fileCacheQuotaPerDevice: 500 * 1024 * 1024,
          fileRetentionHours: 24,
          fileAutoCleanup: true,
        }),
        ...newFileSyncSetting,
      },
    }
    await saveSetting(updatedSetting)
  }

  // Update keyboard shortcuts
  const updateKeyboardShortcuts = async (overrides: Record<string, string | string[]>) => {
    if (!setting) {
      throw new Error('No settings loaded')
    }
    const updatedSetting: Settings = { ...setting, keyboardShortcuts: overrides }
    try {
      await saveSetting(updatedSetting)
    } catch (err) {
      console.error('Failed to update keyboard shortcuts:', err)
      throw err
    }
  }

  // Load settings immediately on mount
  useEffect(() => {
    void loadSetting()
  }, [loadSetting])

  // Note: Cross-window settings sync via daemon WebSocket events (future enhancement)

  // 监听主题变化并应用
  const prevThemeRef = React.useRef<string | undefined>()
  const prevThemeColorRef = React.useRef<string | undefined>()
  const hasAppliedOnceRef = React.useRef(false)

  useEffect(() => {
    // Skip theme application until settings are loaded to avoid
    // flashing the default theme before switching to the user's theme
    if (!setting) return

    const root = window.document.documentElement
    const systemThemeMedia = window.matchMedia('(prefers-color-scheme: dark)')

    const applyTheme = () => {
      const theme = setting.general.theme
      const themeColor = setting.general.themeColor || DEFAULT_THEME_COLOR

      // 1. Apply Mode (Light/Dark)
      root.classList.remove('light', 'dark')

      let resolvedMode: 'light' | 'dark' = 'light'

      if (theme === 'system' || !theme) {
        const systemTheme = systemThemeMedia.matches ? 'dark' : 'light'
        resolvedMode = systemTheme
        root.classList.add(systemTheme)
      } else {
        resolvedMode = theme
        root.classList.add(theme)
      }

      // 2. Apply Theme Color tokens for the resolved mode
      applyThemePreset(themeColor, resolvedMode, root)
    }

    // Use view transition animation only for user-initiated theme changes (not initial load)
    const hasChanged =
      prevThemeRef.current !== setting.general.theme ||
      prevThemeColorRef.current !== (setting.general.themeColor || DEFAULT_THEME_COLOR)

    prevThemeRef.current = setting.general.theme
    prevThemeColorRef.current = setting.general.themeColor || DEFAULT_THEME_COLOR

    if (!hasAppliedOnceRef.current || !hasChanged) {
      hasAppliedOnceRef.current = true
      applyTheme()
    } else {
      startThemeTransition(applyTheme)
    }

    const handleSystemThemeChange = () => {
      if (setting.general.theme === 'system' || !setting.general.theme) {
        applyTheme()
      }
    }

    systemThemeMedia.addEventListener('change', handleSystemThemeChange)

    return () => {
      systemThemeMedia.removeEventListener('change', handleSystemThemeChange)
    }
  }, [setting?.general.theme, setting?.general.themeColor])

  // 监听语言变化并应用
  useEffect(() => {
    const next = normalizeLanguage(setting?.general?.language)
    if (i18n.language !== next) {
      i18n.changeLanguage(next)
    }
    persistLanguage(next)
    // Sync tray menu labels with UI language
    invokeWithTrace('set_tray_language', { language: next }).catch(err => {
      console.error('Failed to sync tray language:', err)
    })
  }, [setting?.general?.language])

  useEffect(() => {
    setFrontendTelemetryEnabled(setting?.general?.telemetryEnabled ?? false)
  }, [setting?.general?.telemetryEnabled])

  const value: SettingContextType = {
    setting,
    loading,
    error,
    updateSetting,
    updateGeneralSetting,
    updateSyncSetting,
    updateSecuritySetting,
    updateRetentionPolicy,
    updateKeyboardShortcuts,
    updateFileSyncSetting,
  }

  return <SettingContext.Provider value={value}>{children}</SettingContext.Provider>
}
