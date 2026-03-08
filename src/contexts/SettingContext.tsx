import { listen } from '@tauri-apps/api/event'
import React, { useCallback, useEffect, useState, type ReactNode } from 'react'
import { SettingContext } from './setting-context'
import { DEFAULT_THEME_COLOR } from '@/constants/theme'
import i18n, { normalizeLanguage, persistLanguage } from '@/i18n'
import { invokeWithTrace } from '@/lib/tauri-command'
import { applyThemePreset } from '@/lib/theme-engine'
import { startThemeTransition } from '@/lib/theme-transition'
import type { SettingChangedEvent } from '@/types/events'
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
      const settingObj = await invokeWithTrace<Settings>('get_settings')
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
      // New command: update_settings, takes JSON object directly
      await invokeWithTrace('update_settings', { settings: newSetting })
      setSetting(newSetting)
      setError(null)
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
    }
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
    }
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
    }
    await saveSetting(updatedSetting)
  }

  // 更新保留策略
  const updateRetentionPolicy = async (newPolicy: Partial<Settings['retention_policy']>) => {
    if (!setting) return
    const updatedSetting: Settings = {
      ...setting,
      retention_policy: {
        ...setting.retention_policy,
        ...newPolicy,
      },
    }
    await saveSetting(updatedSetting)
  }

  // Load settings immediately on mount
  useEffect(() => {
    void loadSetting()
  }, [loadSetting])

  // 监听来自其他窗口的设置变更事件
  useEffect(() => {
    let unlisten: (() => void) | undefined

    const setupSettingChangeListener = async () => {
      try {
        unlisten = await listen<SettingChangedEvent>('setting-changed', event => {
          console.log('收到设置变更事件:', event.payload)

          // 解析新的设置
          try {
            const newSetting = JSON.parse(event.payload.settingJson) as Settings

            // 更新本地状态 (不触发再次保存)
            setSetting(newSetting)
          } catch (err) {
            console.error('解析设置变更事件失败:', err)
          }
        })
      } catch (err) {
        console.error('设置设置变更监听器失败:', err)
      }
    }

    setupSettingChangeListener()

    return () => {
      if (unlisten) {
        unlisten()
      }
    }
  }, [])

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
      const themeColor = setting.general.theme_color || DEFAULT_THEME_COLOR

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
      prevThemeColorRef.current !== (setting.general.theme_color || DEFAULT_THEME_COLOR)

    prevThemeRef.current = setting.general.theme
    prevThemeColorRef.current = setting.general.theme_color || DEFAULT_THEME_COLOR

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
  }, [setting?.general.theme, setting?.general.theme_color])

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

  const value: SettingContextType = {
    setting,
    loading,
    error,
    updateSetting,
    updateGeneralSetting,
    updateSyncSetting,
    updateSecuritySetting,
    updateRetentionPolicy,
  }

  return <SettingContext.Provider value={value}>{children}</SettingContext.Provider>
}
