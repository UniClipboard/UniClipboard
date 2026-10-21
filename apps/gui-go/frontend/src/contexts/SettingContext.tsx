import React, { useCallback, useEffect, useState, type ReactNode } from 'react'
import {
  CustomRelayMutationError,
  getCustomRelays,
  mutateCustomRelay as persistCustomRelayMutation,
  updateSettings,
  type SettingsPatchInput,
} from '@/api/daemon'
import {
  updateKeyboardShortcuts as persistKeyboardShortcuts,
  setQuickPanelDoubleTapModifier as persistQuickPanelDoubleTapModifier,
  setQuickPanelEnabled as persistQuickPanelEnabled,
  setQuickPanelPosition as persistQuickPanelPosition,
  updateAutostart as persistAutostart,
} from '@/api/tauri-command'
import { useWindowTheme } from '@/hooks/useWindowTheme'
import i18n, { normalizeLanguage, persistLanguage } from '@/i18n'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import {
  getSettingsSyncState,
  refreshSettings,
  subscribeSettings,
  writeSettings,
  type SettingsSyncState,
} from '@/lib/settings-sync'
import { setDiagnosticsEnabled } from '@/observability/diagnostics'
import type {
  CustomRelay,
  CustomRelayMutation,
  CustomRelayMutationResult,
  SettingContextType,
  Settings,
} from '@/types/setting'
import { SettingContext } from './setting-context'

const log = createLogger('setting-context')

interface SettingProviderProps {
  children: ReactNode
}

// A patch whose sections hold no defined field changes nothing and is not sent.
function hasChangedLeaf(patch: SettingsPatchInput): boolean {
  return Object.values(patch).some(
    section =>
      section !== undefined &&
      (typeof section !== 'object' ||
        section === null ||
        Object.values(section).some(value => value !== undefined))
  )
}

function loadErrorText(state: SettingsSyncState): string | null {
  return state.error === null ? null : `加载设置失败: ${state.error}`
}

/**
 * The main window's settings. The authoritative copy lives in `@/lib/settings-sync`
 * (ADR-013 section 9): this provider only renders it and sends the changed leaves of each save.
 * A failed save is followed by a read before the caller sees the error, so controls fall back to
 * the stored state; save errors are reported by the caller, `error` is the load error only.
 */
export const SettingProvider: React.FC<SettingProviderProps> = ({ children }) => {
  const [syncState, setSyncState] = useState<SettingsSyncState>(getSettingsSyncState)
  const [customRelays, setCustomRelays] = useState<CustomRelay[]>([])
  const [relayLoading, setRelayLoading] = useState<boolean>(true)
  const [relayError, setRelayError] = useState<string | null>(null)
  const setting = syncState.settings
  const loading = setting === null && syncState.error === null
  const error = loadErrorText(syncState)

  useEffect(() => subscribeSettings(setSyncState), [])

  const reloadCustomRelays = useCallback(async () => {
    setRelayLoading(true)
    try {
      await connectDaemonWs()
      setCustomRelays(await getCustomRelays())
      setRelayError(null)
    } catch (err) {
      log.error({ err }, 'Failed to load custom relays')
      setRelayError(`加载中继列表失败: ${err}`)
      throw err
    } finally {
      setRelayLoading(false)
    }
  }, [])

  useEffect(() => {
    reloadCustomRelays().catch(() => {})
  }, [reloadCustomRelays])

  const reloadSetting = useCallback(async () => {
    await refreshSettings()
  }, [])

  // Send exactly the leaves the caller changed: a field that is absent is not changed, while
  // `false`, `0` and `""` are values. Only `network` or `general.debugMode` make the daemon ask
  // for a restart.
  const saveSetting = useCallback(
    async (patch: SettingsPatchInput): Promise<{ restartRequired: boolean }> => {
      if (!hasChangedLeaf(patch)) return { restartRequired: false }
      try {
        return await writeSettings(async () => {
          const result = await updateSettings(patch)
          if (!result.success) throw new Error('Settings update was rejected')
          return { restartRequired: result.restartRequired }
        })
      } catch (err) {
        log.error({ err }, '保存设置失败')
        throw err
      }
    },
    []
  )

  // 更新通用设置。autoStart 被排除在外:它是桌面宿主 OS 副作用,必须走专用的
  // updateAutostart 命令,否则会静默跳过 OS 启动项注册(daemon settings 管线
  // 不触碰操作系统)。
  const updateGeneralSetting = useCallback(
    async (newGeneralSetting: Partial<Omit<Settings['general'], 'autoStart'>>) => {
      await saveSetting({ general: newGeneralSetting })
    },
    [saveSetting]
  )

  // 切换开机自启动。必须走宿主命令：OS 启动项注册是桌面宿主副作用，daemon HTTP
  // settings API 只做持久化、不触碰操作系统。命令内部持久化 autoStart 并应用 OS
  // 注册（失败回滚）；之后读取 daemon 的权威状态。
  const updateAutostart = async (enabled: boolean) => {
    try {
      await writeSettings(() => persistAutostart(enabled))
    } catch (err) {
      log.error({ err }, '更改自启动状态失败')
      throw err
    }
  }

  const updateSyncSetting = useCallback(
    async (newSyncSetting: Partial<Settings['sync']>) => {
      await saveSetting({ sync: newSyncSetting })
    },
    [saveSetting]
  )

  const updateSecuritySetting = async (newSecuritySetting: Partial<Settings['security']>) => {
    await saveSetting({ security: newSecuritySetting })
  }

  const updateRetentionPolicy = async (newPolicy: Partial<Settings['retentionPolicy']>) => {
    await saveSetting({ retentionPolicy: newPolicy })
  }

  const updateFileSyncSetting = useCallback(
    async (newFileSyncSetting: Partial<Settings['fileSync'] & object>) => {
      await saveSetting({ fileSync: newFileSyncSetting })
    },
    [saveSetting]
  )

  // Update network settings (Phase 95); 透传 restartRequired。
  // 反向命名铁律：此处 partial 真值传递，绝不取反；UI 取反点仅在 NetworkSection.tsx。
  const updateNetworkSetting = async (
    newNetworkSetting: Partial<Settings['network']>
  ): Promise<{ restartRequired: boolean }> => {
    return await saveSetting({ network: newNetworkSetting })
  }

  const mutateCustomRelay = async (
    mutation: CustomRelayMutation
  ): Promise<CustomRelayMutationResult> => {
    try {
      return await writeSettings(async () => {
        try {
          const result = await persistCustomRelayMutation(mutation)
          setCustomRelays(result.relays)
          setRelayError(null)
          return result
        } catch (err) {
          if (err instanceof CustomRelayMutationError && err.kind === 'notFound') {
            try {
              setCustomRelays(await getCustomRelays())
              setRelayError(null)
            } catch (refreshErr) {
              log.error({ err: refreshErr }, 'Failed to refresh custom relays after not found')
              setRelayError(`加载中继列表失败: ${refreshErr}`)
            }
          }
          throw err
        }
      })
    } catch (err) {
      log.error({ err }, 'Failed to mutate custom relay')
      throw err
    }
  }

  // Update quick panel settings.
  //
  // Each quick-panel field uses a dedicated in-process command because enable,
  // placement, and modifier double-tap all have GUI-process side effects. The
  // commands coordinate those effects with daemon persistence; the window then
  // reads the stored state.
  const updateQuickPanelSetting = async (
    newQuickPanelSetting: Partial<Settings['quickPanel']>
  ): Promise<{ restartRequired: boolean }> => {
    const { doubleTapModifier, enabled, position } = newQuickPanelSetting
    if (enabled === undefined && position === undefined && doubleTapModifier === undefined) {
      return { restartRequired: false }
    }
    try {
      await writeSettings(async () => {
        if (enabled !== undefined) await persistQuickPanelEnabled(enabled)
        if (position !== undefined) await persistQuickPanelPosition(position)
        if (doubleTapModifier !== undefined) {
          await persistQuickPanelDoubleTapModifier(doubleTapModifier)
        }
      })
      return { restartRequired: false }
    } catch (err) {
      log.error({ err }, 'Failed to update quick panel setting')
      throw err
    }
  }

  // 更新快捷键。GUI 路径必须走宿主命令，因为快捷面板全局快捷键需要先更新 OS
  // 注册状态（冲突时拒绝保存）；daemon HTTP settings API 只负责持久化。只发送
  // 本次编辑改动的 id，其他 id（包括文件中保留的非法原值）不重新提交。
  const updateKeyboardShortcuts = async (
    previousOverrides: Record<string, string | string[]>,
    nextOverrides: Record<string, string | string[]>
  ) => {
    try {
      await writeSettings(async () => {
        const currentOverrides = getSettingsSyncState().settings?.keyboardShortcuts ?? {}
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
        await persistKeyboardShortcuts(currentOverrides, desiredOverrides)
      })
    } catch (err) {
      log.error({ err }, 'Failed to update keyboard shortcuts')
      throw err
    }
  }

  useWindowTheme(setting?.general)

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
    setDiagnosticsEnabled(enabled)
  }, [setting?.general?.telemetryEnabled])

  const value: SettingContextType = {
    setting,
    loading,
    error,
    customRelays,
    relayLoading,
    relayError,
    reloadSetting,
    reloadCustomRelays,
    updateGeneralSetting,
    updateAutostart,
    updateSyncSetting,
    updateSecuritySetting,
    updateRetentionPolicy,
    updateKeyboardShortcuts,
    updateFileSyncSetting,
    updateNetworkSetting,
    mutateCustomRelay,
    updateQuickPanelSetting,
  }

  return <SettingContext.Provider value={value}>{children}</SettingContext.Provider>
}
