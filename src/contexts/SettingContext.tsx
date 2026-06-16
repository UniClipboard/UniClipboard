import React, { useCallback, useEffect, useState, type ReactNode } from 'react'
import { getSettings, updateSettings } from '@/api/daemon'
import {
  updateKeyboardShortcuts as persistKeyboardShortcuts,
  setQuickPanelEnabled as persistQuickPanelEnabled,
  setQuickPanelPosition as persistQuickPanelPosition,
  updateAutostart as persistAutostart,
} from '@/api/tauri-command'
import { DEFAULT_THEME_COLOR } from '@/constants/theme'
import i18n, { normalizeLanguage, persistLanguage } from '@/i18n'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { emitSettingsChanged } from '@/lib/settings-events'
import { applyThemeOverrides, applyThemePreset } from '@/lib/theme-engine'
import { startThemeTransition } from '@/lib/theme-transition'
import { setFrontendSentryEnabled } from '@/observability/sentry'
import type { SettingContextType, Settings } from '@/types/setting'
import { SettingContext } from './setting-context'

const log = createLogger('setting-context')

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
      log.error({ err }, '加载设置失败')
      setError(`加载设置失败: ${err}`)
    } finally {
      setLoading(false)
    }
  }, [])

  // 保存设置
  // Phase 95: 返回 { restartRequired } 透传 daemon PUT /settings 响应；
  // 现有调用方 await 但不读返回值，向后兼容（Promise<X> 可被忽略）。
  const saveSetting = async (newSetting: Settings): Promise<{ restartRequired: boolean }> => {
    try {
      setLoading(true)
      const result = await updateSettings(newSetting)
      setSetting(newSetting)
      setError(null)
      try {
        await emitSettingsChanged(newSetting)
      } catch (err) {
        log.error({ err }, 'Failed to broadcast settings change')
      }
      return { restartRequired: result.restartRequired }
    } catch (err) {
      log.error({ err }, '保存设置失败')
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

  // 更新通用设置。autoStart 被排除在外:它是桌面宿主 OS 副作用,必须走专用的
  // updateAutostart 命令,否则会静默跳过 OS 启动项注册(daemon settings 管线
  // 不触碰操作系统)。
  const updateGeneralSetting = async (
    newGeneralSetting: Partial<Omit<Settings['general'], 'autoStart'>>
  ) => {
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

  // 切换开机自启动。必须走 Tauri in-process command：OS 启动项注册是桌面宿主
  // 副作用，daemon HTTP settings API 只做持久化、不触碰操作系统。命令内部会
  // 持久化 auto_start 并应用 OS 注册（失败回滚），这里只把落地后的值合并进
  // 内存 state 并广播，避免与 updateKeyboardShortcuts 一样的展示态漂移。
  const updateAutostart = async (enabled: boolean) => {
    if (!setting) {
      throw new Error('No settings loaded')
    }
    try {
      setLoading(true)
      await persistAutostart(enabled)
      const updatedSetting: Settings = {
        ...setting,
        general: { ...setting.general, autoStart: enabled },
      }
      setSetting(prev =>
        prev ? { ...prev, general: { ...prev.general, autoStart: enabled } } : updatedSetting
      )
      setError(null)
      try {
        await emitSettingsChanged(updatedSetting)
      } catch (err) {
        log.error({ err }, 'Failed to broadcast settings change')
      }
    } catch (err) {
      log.error({ err }, '更改自启动状态失败')
      setError(`保存设置失败: ${err}`)
      throw err
    } finally {
      setLoading(false)
    }
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

  // Update network settings (Phase 95)
  // 镜像 partial 进 setting.network 后调 saveSetting；透传 restartRequired。
  // 反向命名铁律：此处 partial 真值传递，绝不取反；UI 取反点仅在 NetworkSection.tsx。
  const updateNetworkSetting = async (
    newNetworkSetting: Partial<Settings['network']>
  ): Promise<{ restartRequired: boolean }> => {
    if (!setting) return { restartRequired: false }
    const updatedSetting: Settings = {
      ...setting,
      network: {
        ...setting.network,
        ...newNetworkSetting,
      },
    }
    return await saveSetting(updatedSetting)
  }

  // Update quick panel settings.
  //
  // GUI 路径必须走 Tauri in-process command:`enabled` 启用/禁用会即时触发
  // 全局快捷键注册/反注册 + 隐藏窗口的创建/销毁；`position` 则刷新后端缓存
  // 的展示位置（供同步执行的 show() 读取）。这些副作用都只能在 GUI 进程内
  // 完成,命令内部已协调好 OS 副作用与 facade 持久化的事务性,所以这里只在
  // 成功后更新本地 setState + 广播,不再额外走 daemon HTTP PUT。
  //
  // 两个字段对应两条独立命令,按本次 patch 里实际出现的字段分别下发。
  const updateQuickPanelSetting = async (
    newQuickPanelSetting: Partial<Settings['quickPanel']>
  ): Promise<{ restartRequired: boolean }> => {
    if (!setting) return { restartRequired: false }
    const { enabled, position } = newQuickPanelSetting
    if (enabled === undefined && position === undefined) {
      return { restartRequired: false }
    }
    try {
      setLoading(true)
      if (enabled !== undefined) {
        await persistQuickPanelEnabled(enabled)
      }
      if (position !== undefined) {
        await persistQuickPanelPosition(position)
      }
      const updatedSetting: Settings = {
        ...setting,
        quickPanel: { ...setting.quickPanel, ...newQuickPanelSetting },
      }
      setSetting(prev =>
        prev
          ? { ...prev, quickPanel: { ...prev.quickPanel, ...newQuickPanelSetting } }
          : updatedSetting
      )
      setError(null)
      try {
        await emitSettingsChanged(updatedSetting)
      } catch (err) {
        log.error({ err }, 'Failed to broadcast settings change')
      }
      return { restartRequired: false }
    } catch (err) {
      log.error({ err }, 'Failed to update quick panel setting')
      setError(`保存设置失败: ${err}`)
      throw err
    } finally {
      setLoading(false)
    }
  }

  // 更新快捷键。GUI 路径必须走 Tauri in-process command，因为快捷面板全局
  // 快捷键需要同步更新 OS 注册状态；daemon HTTP settings API 只负责持久化。
  const updateKeyboardShortcuts = async (overrides: Record<string, string | string[]>) => {
    if (!setting) {
      throw new Error('No settings loaded')
    }
    try {
      setLoading(true)
      const keyboardShortcuts = await persistKeyboardShortcuts(
        setting.keyboardShortcuts ?? {},
        overrides
      )
      // 合并到 await 之后的最新 state，避免覆盖期间发生的并发更新。
      // 广播仍用 await 前快照——任何并发更新会自己 emit，这里只承诺广播
      // 本次快捷键变更落地后的视图。
      const updatedSetting: Settings = { ...setting, keyboardShortcuts }
      setSetting(prev => (prev ? { ...prev, keyboardShortcuts } : updatedSetting))
      setError(null)
      try {
        await emitSettingsChanged(updatedSetting)
      } catch (err) {
        log.error({ err }, 'Failed to broadcast settings change')
      }
    } catch (err) {
      log.error({ err }, 'Failed to update keyboard shortcuts')
      setError(`保存设置失败: ${err}`)
      throw err
    } finally {
      setLoading(false)
    }
  }

  // Load settings immediately on mount
  useEffect(() => {
    void loadSetting()
  }, [loadSetting])

  // Note: Cross-window settings sync via daemon WebSocket events (future enhancement)

  // 监听主题变化并应用。
  //
  // # 抖动防御
  // transition reveal 动画只在**实际渲染结果**变化时触发,而不是 raw theme
  // 字段变化时触发。例如用户切换 Follow system 开关:`theme: dark → system`
  // 但当前媒体查询恰好也是 dark → resolved mode 不变 → 颜色也不变,这时
  // 不应该跑 500ms 的圆形 reveal,否则就是无意义的"闪一下"。
  const prevResolvedModeRef = React.useRef<'light' | 'dark' | undefined>(undefined)
  const prevAppliedColorRef = React.useRef<string | undefined>(undefined)
  const prevAppliedOverridesRef = React.useRef<string | undefined>(undefined)
  const hasAppliedOnceRef = React.useRef(false)

  useEffect(() => {
    // Skip theme application until settings are loaded to avoid
    // flashing the default theme before switching to the user's theme
    if (!setting) return

    const root = window.document.documentElement
    const systemThemeMedia = window.matchMedia('(prefers-color-scheme: dark)')

    // 选取当前 mode 应用的预设：优先 light/dark 拆分字段,缺失时回退到旧
    // themeColor 字段（v0.7 之前持久化的偏好）,再缺失时使用引擎默认。
    const resolveThemeColor = (mode: 'light' | 'dark'): string => {
      const split =
        mode === 'dark' ? setting.general.themeColorDark : setting.general.themeColorLight
      return split || setting.general.themeColor || DEFAULT_THEME_COLOR
    }

    const resolveOverrides = (mode: 'light' | 'dark'): Record<string, string> => {
      return mode === 'dark'
        ? setting.general.themeOverridesDark || {}
        : setting.general.themeOverridesLight || {}
    }

    const resolveMode = (): 'light' | 'dark' => {
      const theme = setting.general.theme
      if (theme === 'light' || theme === 'dark') return theme
      return systemThemeMedia.matches ? 'dark' : 'light'
    }

    const applyTheme = () => {
      const resolvedMode = resolveMode()
      root.classList.remove('light', 'dark')
      root.classList.add(resolvedMode)
      applyThemePreset(resolveThemeColor(resolvedMode), resolvedMode, root)
      applyThemeOverrides(resolveOverrides(resolvedMode), root)
    }

    // 比较"实际生效的 mode + color + overrides"是否变化,而不是 raw theme 字段。
    const nextResolvedMode = resolveMode()
    const nextAppliedColor = resolveThemeColor(nextResolvedMode)
    const nextOverridesKey = JSON.stringify(resolveOverrides(nextResolvedMode))
    const hasVisualChange =
      prevResolvedModeRef.current !== nextResolvedMode ||
      prevAppliedColorRef.current !== nextAppliedColor ||
      prevAppliedOverridesRef.current !== nextOverridesKey

    prevResolvedModeRef.current = nextResolvedMode
    prevAppliedColorRef.current = nextAppliedColor
    prevAppliedOverridesRef.current = nextOverridesKey

    if (!hasAppliedOnceRef.current || !hasVisualChange) {
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
    // 依赖列表包含 `setting` 本体，react-doctor / exhaustive-deps 的
    // "需要整个 setting" 与作者的"只关心主题字段"两个语义在这里合二为一：
    // 多出来的 effect 重跑会被上方 prevResolvedMode/Color/Overrides 三个 ref
    // 比较拦下来，不会触发 startThemeTransition 的 reveal 动画。
  }, [
    setting,
    setting?.general.theme,
    setting?.general.themeColor,
    setting?.general.themeColorLight,
    setting?.general.themeColorDark,
    setting?.general.themeOverridesLight,
    setting?.general.themeOverridesDark,
  ])

  // 监听语言变化并应用
  useEffect(() => {
    const next = normalizeLanguage(setting?.general?.language)
    if (i18n.language !== next) {
      i18n.changeLanguage(next)
    }
    persistLanguage(next)
    // Sync tray menu labels with UI language
    commands.setTrayLanguage(next).catch(err => {
      log.error({ err }, 'Failed to sync tray language')
    })
  }, [setting?.general?.language])

  // 将用户侧遥测开关同步到前端观测出口；初次加载设置和后续变更都会立即生效。
  // 前端通过 Sentry 的 beforeSend / beforeBreadcrumb / beforeSendLog 钩子
  // 在 sentryRuntimeEnabled=false 时直接丢弃事件；后端 Sentry 由
  // uc-observability 的 telemetry_gate 同步，不需要重启。
  useEffect(() => {
    const enabled = setting?.general?.telemetryEnabled
    if (typeof enabled !== 'boolean') return
    setFrontendSentryEnabled(enabled)
  }, [setting?.general?.telemetryEnabled])

  const value: SettingContextType = {
    setting,
    loading,
    error,
    reloadSetting: loadSetting,
    updateSetting,
    updateGeneralSetting,
    updateAutostart,
    updateSyncSetting,
    updateSecuritySetting,
    updateRetentionPolicy,
    updateKeyboardShortcuts,
    updateFileSyncSetting,
    updateNetworkSetting,
    updateQuickPanelSetting,
  }

  return <SettingContext.Provider value={value}>{children}</SettingContext.Provider>
}
