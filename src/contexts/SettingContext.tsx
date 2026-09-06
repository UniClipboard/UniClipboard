import React, { useCallback, useEffect, useState, type ReactNode } from 'react'
import { getSettings, saveRelay as persistRelay, updateSettings } from '@/api/daemon'
import {
  updateKeyboardShortcuts as persistKeyboardShortcuts,
  setQuickPanelDoubleTapModifier as persistQuickPanelDoubleTapModifier,
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
import type {
  RelaySaveContextResult,
  RelaySaveMutation,
  SettingContextType,
  Settings,
} from '@/types/setting'
import { SettingContext } from './setting-context'

const log = createLogger('setting-context')

function relayUrlsMatch(left: string, right: string): boolean {
  try {
    return new URL(left.trim()).toString() === new URL(right.trim()).toString()
  } catch {
    return left.trim() === right.trim()
  }
}

// 设置提供者属性接口
interface SettingProviderProps {
  children: ReactNode
}

// 设置提供者组件
export const SettingProvider: React.FC<SettingProviderProps> = ({ children }) => {
  const [setting, setSetting] = useState<Settings | null>(null)
  const [loading, setLoading] = useState<boolean>(true)
  const [error, setError] = useState<string | null>(null)
  const latestSettingRef = React.useRef<Settings | null>(null)
  const mutationQueueRef = React.useRef<Promise<void>>(Promise.resolve())

  const enqueueTask = useCallback(<T,>(task: () => Promise<T>): Promise<T> => {
    const operation = mutationQueueRef.current.then(task)
    mutationQueueRef.current = operation.then(
      () => undefined,
      () => undefined
    )
    return operation
  }, [])

  // 加载设置
  const loadSetting = useCallback(async () => {
    try {
      setLoading(true)
      await enqueueTask(async () => {
        // Ensure daemon is connected before making API calls — the connection may not
        // have been established yet if this fires before AppContent calls connectDaemonWs().
        await connectDaemonWs()
        const settingObj = await getSettings()
        latestSettingRef.current = settingObj
        setSetting(settingObj)
        setError(null)
      })
    } catch (err) {
      log.error({ err }, '加载设置失败')
      setError(`加载设置失败: ${err}`)
    } finally {
      setLoading(false)
    }
  }, [enqueueTask])

  const enqueueSettingMutation = useCallback(
    <T,>(mutate: (current: Settings) => Promise<{ next: Settings; result: T }>): Promise<T> => {
      return enqueueTask(async () => {
        const current = latestSettingRef.current
        if (!current) throw new Error('No settings loaded')
        const { next, result } = await mutate(current)
        latestSettingRef.current = next
        setSetting(next)
        setError(null)
        try {
          await emitSettingsChanged(next)
        } catch (err) {
          log.error({ err }, 'Failed to broadcast settings change')
        }
        return result
      })
    },
    [enqueueTask]
  )

  // 保存设置
  // Phase 95: 返回 { restartRequired } 透传 daemon PUT /settings 响应；
  // 现有调用方 await 但不读返回值，向后兼容（Promise<X> 可被忽略）。
  const saveSetting = useCallback(
    async (buildNext: (current: Settings) => Settings): Promise<{ restartRequired: boolean }> => {
      // Per-section saves must NOT flip the global `loading` flag. Every settings
      // section subscribes to this context and ORs `loading` into its own
      // `disabled` state (`isBusy = loading || saving`), so toggling `loading`
      // here would disable/dim every sibling section's controls at once — a
      // full-panel flash on each save. `loading` is reserved for the initial
      // load / reload in `loadSetting`; an in-flight per-section save is tracked
      // by that section's local `saving` flag instead.
      try {
        return await enqueueSettingMutation(async current => {
          const next = buildNext(current)
          const result = await updateSettings(next)
          if (!result.success) throw new Error('Settings update was rejected')
          return { next, result: { restartRequired: result.restartRequired } }
        })
      } catch (err) {
        log.error({ err }, '保存设置失败')
        setError(`保存设置失败: ${err}`)
        throw err // 重新抛出错误，让调用者可以处理
      }
    },
    [enqueueSettingMutation]
  )

  // 更新整个设置
  const updateSetting = async (newSetting: Settings) => {
    await saveSetting(() => newSetting)
  }

  // 更新通用设置。autoStart 被排除在外:它是桌面宿主 OS 副作用,必须走专用的
  // updateAutostart 命令,否则会静默跳过 OS 启动项注册(daemon settings 管线
  // 不触碰操作系统)。
  const updateGeneralSetting = useCallback(
    async (newGeneralSetting: Partial<Omit<Settings['general'], 'autoStart'>>) => {
      await saveSetting(current => ({
        ...current,
        general: {
          ...current.general,
          ...newGeneralSetting,
        },
      }))
    },
    [saveSetting]
  )

  // 切换开机自启动。必须走 Tauri in-process command：OS 启动项注册是桌面宿主
  // 副作用，daemon HTTP settings API 只做持久化、不触碰操作系统。命令内部会
  // 持久化 auto_start 并应用 OS 注册（失败回滚），这里只把落地后的值合并进
  // 内存 state 并广播，避免与 updateKeyboardShortcuts 一样的展示态漂移。
  const updateAutostart = async (enabled: boolean) => {
    // See `saveSetting`: `loading` is not flipped for per-section saves. The
    // Startup section already tracks this mutation via its local `saving`.
    try {
      await enqueueSettingMutation(async current => {
        await persistAutostart(enabled)
        return {
          next: { ...current, general: { ...current.general, autoStart: enabled } },
          result: undefined,
        }
      })
    } catch (err) {
      log.error({ err }, '更改自启动状态失败')
      setError(`保存设置失败: ${err}`)
      throw err
    }
  }

  // 更新同步设置
  const updateSyncSetting = useCallback(
    async (newSyncSetting: Partial<Settings['sync']>) => {
      await saveSetting(current => ({
        ...current,
        sync: {
          ...current.sync,
          ...newSyncSetting,
        },
      }))
    },
    [saveSetting]
  )

  // 更新安全设置
  const updateSecuritySetting = async (newSecuritySetting: Partial<Settings['security']>) => {
    await saveSetting(current => ({
      ...current,
      security: {
        ...current.security,
        ...newSecuritySetting,
      },
    }))
  }

  // 更新保留策略
  const updateRetentionPolicy = async (newPolicy: Partial<Settings['retentionPolicy']>) => {
    await saveSetting(current => ({
      ...current,
      retentionPolicy: {
        ...current.retentionPolicy,
        ...newPolicy,
      },
    }))
  }

  // Update file sync settings
  const updateFileSyncSetting = useCallback(
    async (newFileSyncSetting: Partial<Settings['fileSync'] & object>) => {
      await saveSetting(current => ({
        ...current,
        fileSync: {
          ...(current.fileSync ?? {
            fileSyncEnabled: true,
            smallFileThreshold: 10 * 1024 * 1024,
            maxFileSize: 5 * 1024 * 1024 * 1024,
            fileCacheQuotaPerDevice: 500 * 1024 * 1024,
            fileRetentionHours: 24,
            fileAutoCleanup: true,
          }),
          ...newFileSyncSetting,
        },
      }))
    },
    [saveSetting]
  )

  // Update network settings (Phase 95)
  // 镜像 partial 进 setting.network 后调 saveSetting；透传 restartRequired。
  // 反向命名铁律：此处 partial 真值传递，绝不取反；UI 取反点仅在 NetworkSection.tsx。
  const updateNetworkSetting = async (
    newNetworkSetting: Partial<Settings['network']>
  ): Promise<{ restartRequired: boolean }> => {
    return await saveSetting(current => ({
      ...current,
      network: {
        ...current.network,
        ...newNetworkSetting,
      },
    }))
  }

  const saveRelay = async (mutation: RelaySaveMutation): Promise<RelaySaveContextResult> => {
    try {
      return await enqueueSettingMutation(async current => {
        const customRelayUrls = [...current.network.customRelayUrls]
        let credentialUrl: string

        if (mutation.index === null) {
          if (mutation.previousUrl !== null || mutation.nextUrl === null) {
            throw new Error('Invalid relay addition')
          }
          customRelayUrls.push(mutation.nextUrl)
          credentialUrl = mutation.nextUrl
        } else {
          if (
            mutation.index < 0 ||
            mutation.index >= customRelayUrls.length ||
            customRelayUrls[mutation.index] !== mutation.previousUrl
          ) {
            throw new Error('Relay settings changed before the save could run')
          }
          credentialUrl = mutation.nextUrl ?? mutation.previousUrl
          if (mutation.nextUrl === null) customRelayUrls.splice(mutation.index, 1)
          else customRelayUrls[mutation.index] = mutation.nextUrl
        }

        const credential =
          mutation.nextUrl === null &&
          mutation.credential.action === 'delete' &&
          customRelayUrls.some(url => relayUrlsMatch(url, credentialUrl))
            ? { action: 'keep' as const }
            : mutation.credential
        const result = await persistRelay(
          { network: { customRelayUrls } },
          credentialUrl,
          credential
        )
        if (!result.success) throw new Error('Relay settings update was rejected')

        return {
          next: result.settings,
          result: {
            restartRequired: result.restartRequired,
            credentialStatus: result.credentialStatus,
          },
        }
      })
    } catch (err) {
      log.error({ err }, 'Failed to save relay settings')
      setError(`保存设置失败: ${err}`)
      throw err
    }
  }

  // Update quick panel settings.
  //
  // Each quick-panel field uses a dedicated in-process command because enable,
  // placement, and modifier double-tap all have GUI-process side effects. The
  // commands coordinate those effects with daemon persistence, so this layer
  // only updates React state and broadcasts after each command succeeds.
  const updateQuickPanelSetting = async (
    newQuickPanelSetting: Partial<Settings['quickPanel']>
  ): Promise<{ restartRequired: boolean }> => {
    const { doubleTapModifier, enabled, position } = newQuickPanelSetting
    if (enabled === undefined && position === undefined && doubleTapModifier === undefined) {
      return { restartRequired: false }
    }
    // See `saveSetting`: `loading` is not flipped for per-section saves. The
    // Quick Panel section already tracks this mutation via its local `saving`.
    try {
      return await enqueueSettingMutation(async current => {
        if (enabled !== undefined) await persistQuickPanelEnabled(enabled)
        if (position !== undefined) await persistQuickPanelPosition(position)
        if (doubleTapModifier !== undefined) {
          await persistQuickPanelDoubleTapModifier(doubleTapModifier)
        }
        return {
          next: {
            ...current,
            quickPanel: { ...current.quickPanel, ...newQuickPanelSetting },
          },
          result: { restartRequired: false },
        }
      })
    } catch (err) {
      log.error({ err }, 'Failed to update quick panel setting')
      setError(`保存设置失败: ${err}`)
      throw err
    }
  }

  // 更新快捷键。GUI 路径必须走 Tauri in-process command，因为快捷面板全局
  // 快捷键需要同步更新 OS 注册状态；daemon HTTP settings API 只负责持久化。
  const updateKeyboardShortcuts = async (
    previousOverrides: Record<string, string | string[]>,
    nextOverrides: Record<string, string | string[]>
  ) => {
    // See `saveSetting`: `loading` is not flipped for per-section saves. The
    // shortcut editors manage their own in-flight state locally.
    try {
      await enqueueSettingMutation(async current => {
        const currentOverrides = current.keyboardShortcuts ?? {}
        const desiredOverrides = { ...currentOverrides }
        const touchedIds = new Set([
          ...Object.keys(previousOverrides),
          ...Object.keys(nextOverrides),
        ])
        for (const id of touchedIds) {
          if (JSON.stringify(previousOverrides[id]) === JSON.stringify(nextOverrides[id])) continue
          if (id in nextOverrides) desiredOverrides[id] = nextOverrides[id]
          else delete desiredOverrides[id]
        }
        const keyboardShortcuts = await persistKeyboardShortcuts(currentOverrides, desiredOverrides)
        return { next: { ...current, keyboardShortcuts }, result: undefined }
      })
    } catch (err) {
      log.error({ err }, 'Failed to update keyboard shortcuts')
      setError(`保存设置失败: ${err}`)
      throw err
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
  const general = setting?.general

  useEffect(() => {
    // Skip theme application until settings are loaded to avoid
    // flashing the default theme before switching to the user's theme
    if (!general) return

    const root = window.document.documentElement
    const systemThemeMedia = window.matchMedia('(prefers-color-scheme: dark)')

    // 选取当前 mode 应用的预设：优先 light/dark 拆分字段,缺失时回退到旧
    // themeColor 字段（v0.7 之前持久化的偏好）,再缺失时使用引擎默认。
    const resolveThemeColor = (mode: 'light' | 'dark'): string => {
      const split = mode === 'dark' ? general.themeColorDark : general.themeColorLight
      return split || general.themeColor || DEFAULT_THEME_COLOR
    }

    const resolveOverrides = (mode: 'light' | 'dark'): Record<string, string> => {
      return mode === 'dark' ? general.themeOverridesDark || {} : general.themeOverridesLight || {}
    }

    const resolveMode = (): 'light' | 'dark' => {
      const theme = general.theme
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
      if (general.theme === 'system' || !general.theme) {
        applyTheme()
      }
    }

    systemThemeMedia.addEventListener('change', handleSystemThemeChange)

    return () => {
      systemThemeMedia.removeEventListener('change', handleSystemThemeChange)
    }
  }, [general])

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

  // Synchronize the user preference to every frontend Sentry payload path.
  // Sampling and before-send hooks read the runtime gate on every capture, so
  // both the initial settings load and later changes take effect immediately.
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
    saveRelay,
    updateQuickPanelSetting,
  }

  return <SettingContext.Provider value={value}>{children}</SettingContext.Provider>
}
