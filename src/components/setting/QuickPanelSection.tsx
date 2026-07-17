import { openUrl } from '@tauri-apps/plugin-opener'
import { ExternalLink } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getQuickPanelDoubleTapAvailability,
  type ModifierDoubleTapAvailability,
} from '@/api/tauri-command'
import {
  Button,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
} from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { SHORTCUT_DEFINITIONS, type ShortcutDefinition } from '@/shortcuts/definitions'
import type { QuickPanelDoubleTapModifier, QuickPanelPosition } from '@/types/setting'
import { RestartBanner } from './RestartBanner'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { ShortcutRow } from './ShortcutRow'
import { useOptimisticSetting } from './useOptimisticSetting'

const log = createLogger('quick-panel-section')

const QUICK_PANEL_SHORTCUT_ID = 'global.toggleQuickPanel'
const MACOS_ACCESSIBILITY_SETTINGS_URL =
  'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility'

/**
 * Quick panel feature section.
 *
 * - 开启:`set_quick_panel_enabled` Tauri command 即时注册全局快捷键 +
 *   预创建隐藏面板窗口,无需重启。
 * - 关闭:即时反注册全局快捷键(快捷键立刻失效),但隐藏窗口 / 底层
 *   WKWebView / WebContent XPC 进程不会被销毁——macOS 上销毁路径会触发
 *   崩溃。要彻底释放这些资源,提示用户手动重启 GUI(下次启动按 enabled=false
 *   跳过 pre_create 即可)。
 *
 * 这里把"切换快捷面板"的快捷键也一并展示出来,让用户在同一个 section
 * 内完成"开关 + 配快捷键"两件事;Shortcuts section 里仍然保留同一行,
 * 两处共享同一个 `keyboardShortcuts[global.toggleQuickPanel]` 字段。
 */
export default function QuickPanelSection() {
  const { t } = useTranslation()
  const { isMac, isWindows } = usePlatform()
  const { setting, updateQuickPanelSetting, updateKeyboardShortcuts } = useSetting()

  // setting?.keyboardShortcuts 可能在 setting 重新加载时被赋成全新对象，但
  // 内容若未变（同一份持久 JSON re-parse）useCallback 仍会被无谓重建，并把
  // 下游 ShortcutRow 的 React.memo 全部击穿。这里直接用 useMemo 锁住引用，
  // 让 `[overrides]` 依赖只在键位真正变化时翻新。
  const overrides = useMemo(() => setting?.keyboardShortcuts ?? {}, [setting?.keyboardShortcuts])
  const quickPanelDef = useMemo<ShortcutDefinition | undefined>(
    () => SHORTCUT_DEFINITIONS.find(def => def.id === QUICK_PANEL_SHORTCUT_ID),
    []
  )

  // 用户在本次会话里从开启切到关闭后,显示"重启以彻底释放资源"提示。
  // 不持久化:重启 GUI 后这条 hint 自然消失,因为启动期 pre_create 已经
  // 因 enabled=false 跳过,资源也已释放。再次开启时清掉提示。
  const [disabledThisSession, setDisabledThisSession] = useState(false)
  const [restartLoading, setRestartLoading] = useState(false)
  const [restartError, setRestartError] = useState<string | null>(null)
  const [doubleTapAvailability, setDoubleTapAvailability] =
    useState<ModifierDoubleTapAvailability | null>(null)

  useEffect(() => {
    let active = true
    const refreshAvailability = () => {
      void getQuickPanelDoubleTapAvailability()
        .then(availability => {
          if (active) setDoubleTapAvailability(availability)
        })
        .catch(err => {
          log.error({ err }, 'Failed to detect modifier double-tap availability')
          if (active) setDoubleTapAvailability('unsupported_display_session')
        })
    }

    refreshAvailability()
    window.addEventListener('focus', refreshAvailability)
    return () => {
      active = false
      window.removeEventListener('focus', refreshAvailability)
    }
  }, [])

  // Optimistic like every other settings toggle: the switch flips immediately
  // and persists in the background, so it no longer waits a daemon round-trip
  // or dims the group while saving. The session-local "was disabled" hint and
  // the stale-error reset run only after a successful persist.
  const [enabled, setEnabled] = useOptimisticSetting(
    setting?.quickPanel?.enabled ?? false,
    async (next: boolean) => {
      await updateQuickPanelSetting({ enabled: next })
      setDisabledThisSession(prev => (next ? false : prev || true))
      if (next) setRestartError(null)
    },
    { failureLog: 'Failed to toggle quick panel' }
  )
  const [position, setPosition] = useOptimisticSetting<QuickPanelPosition>(
    setting?.quickPanel?.position ?? 'center',
    next => updateQuickPanelSetting({ position: next }),
    { failureLog: 'Failed to change quick panel position' }
  )
  const [doubleTapModifier, setDoubleTapModifier] =
    useOptimisticSetting<QuickPanelDoubleTapModifier>(
      setting?.quickPanel?.doubleTapModifier ?? 'disabled',
      next => updateQuickPanelSetting({ doubleTapModifier: next }),
      {
        failureLog: 'Failed to change quick panel modifier trigger',
        errorKey: error =>
          typeof error === 'object' &&
          error !== null &&
          'code' in error &&
          error.code === 'AccessibilityPermissionRequired'
            ? 'settings.sections.quickPanel.doubleTap.permissionRequired'
            : 'settings.sections.general.saveError',
      }
    )
  const altLabel = isMac
    ? t('settings.sections.quickPanel.doubleTap.option')
    : t('settings.sections.quickPanel.doubleTap.alt')
  const metaLabel = isMac
    ? t('settings.sections.quickPanel.doubleTap.command')
    : isWindows
      ? t('settings.sections.quickPanel.doubleTap.windows')
      : t('settings.sections.quickPanel.doubleTap.super')
  const restartHintVisible = disabledThisSession && !enabled
  const doubleTapSupported = doubleTapAvailability === 'supported'
  const doubleTapDescriptionKey =
    doubleTapAvailability === null
      ? 'settings.sections.quickPanel.doubleTap.checking'
      : doubleTapAvailability === 'accessibility_permission_required'
        ? 'settings.sections.quickPanel.doubleTap.permissionRequired'
        : doubleTapSupported
          ? 'settings.sections.quickPanel.doubleTap.description'
          : 'settings.sections.quickPanel.doubleTap.unsupported'

  const handleOpenAccessibilitySettings = () => {
    void openUrl(MACOS_ACCESSIBILITY_SETTINGS_URL).catch(err => {
      log.error({ err }, 'Failed to open macOS Accessibility settings')
      toast.error(t('settings.sections.quickPanel.doubleTap.openSettingsError'))
    })
  }

  const handleRestart = async () => {
    setRestartLoading(true)
    setRestartError(null)
    try {
      // app.restart() 不返回(进程会 exit),后续代码理论上不可达;
      // 走到 catch 说明 spawn 本身就失败了。
      await commands.restartApp()
      setRestartLoading(false)
      return true
    } catch (err) {
      log.error({ err }, '快捷面板关闭后重启应用失败')
      setRestartError(t('settings.restartBanner.errorMessage'))
      setRestartLoading(false)
      return false
    }
  }

  const getCurrentKey = (def: ShortcutDefinition): string => {
    const override = overrides[def.id]
    if (override != null) {
      return Array.isArray(override) ? (override[0] ?? String(def.key)) : override
    }
    return Array.isArray(def.key) ? (def.key[0] ?? '') : def.key
  }

  const isModified = (defId: string): boolean => defId in overrides

  const shortcutsById = useMemo(() => new Map(SHORTCUT_DEFINITIONS.map(d => [d.id, d])), [])

  const handleOverrideChange = useCallback(
    async (id: string, newKey: string, clearedIds?: string[]) => {
      const newOverrides = { ...overrides }
      newOverrides[id] = newKey
      if (clearedIds && clearedIds.length > 0) {
        for (const clearedId of clearedIds) {
          const clearedDef = shortcutsById.get(clearedId)
          if (clearedDef) {
            const clearedDefaultKey = Array.isArray(clearedDef.key)
              ? clearedDef.key[0]
              : clearedDef.key
            if (clearedDefaultKey === newKey) {
              newOverrides[clearedId] = ''
            } else {
              delete newOverrides[clearedId]
            }
          }
        }
      }
      try {
        await updateKeyboardShortcuts(overrides, newOverrides)
      } catch (err) {
        log.error({ err }, '更新快捷面板快捷键失败')
      }
    },
    [overrides, shortcutsById, updateKeyboardShortcuts]
  )

  const handleResetShortcut = useCallback(
    async (id: string) => {
      const newOverrides = { ...overrides }
      delete newOverrides[id]
      try {
        await updateKeyboardShortcuts(overrides, newOverrides)
      } catch (err) {
        log.error({ err }, '重置快捷面板快捷键失败')
      }
    },
    [overrides, updateKeyboardShortcuts]
  )

  return (
    <div className="space-y-6">
      <SettingGroup title={t('settings.sections.quickPanel.featureTitle')}>
        <RestartBanner
          visible={restartHintVisible}
          message={t('settings.sections.quickPanel.restartHint')}
          onRestart={handleRestart}
          loading={restartLoading}
          error={restartError}
          onDismissError={() => setRestartError(null)}
        />
        <SettingRow
          label={t('settings.sections.quickPanel.enable.label')}
          description={t('settings.sections.quickPanel.enable.description')}
        >
          <Switch checked={enabled} onCheckedChange={setEnabled} />
        </SettingRow>
        <SettingRow
          label={t('settings.sections.quickPanel.position.label')}
          description={t('settings.sections.quickPanel.position.description')}
        >
          <Select
            value={position}
            onValueChange={value => setPosition(value as QuickPanelPosition)}
            disabled={!enabled}
          >
            <SelectTrigger className="h-7 w-40 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="center" className="text-xs">
                {t('settings.sections.quickPanel.position.center')}
              </SelectItem>
              <SelectItem value="follow_cursor" className="text-xs">
                {t('settings.sections.quickPanel.position.followCursor')}
              </SelectItem>
            </SelectContent>
          </Select>
        </SettingRow>
      </SettingGroup>

      {quickPanelDef && (
        <SettingGroup title={t('settings.sections.quickPanel.shortcutTitle')}>
          <SettingRow
            label={t('settings.sections.quickPanel.doubleTap.label')}
            description={t(doubleTapDescriptionKey)}
          >
            <div className="flex flex-wrap items-center justify-end gap-2">
              {doubleTapAvailability === 'accessibility_permission_required' && (
                <Button variant="outline" size="sm" onClick={handleOpenAccessibilitySettings}>
                  <ExternalLink className="size-3.5" />
                  {t('settings.sections.quickPanel.doubleTap.openSettings')}
                </Button>
              )}
              <Select
                value={doubleTapModifier}
                onValueChange={value => setDoubleTapModifier(value as QuickPanelDoubleTapModifier)}
                disabled={!enabled || !doubleTapSupported}
              >
                <SelectTrigger className="h-7 w-40 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="disabled" className="text-xs">
                    {t('settings.sections.quickPanel.doubleTap.disabled')}
                  </SelectItem>
                  <SelectItem value="alt" className="text-xs">
                    {altLabel}
                  </SelectItem>
                  <SelectItem value="control" className="text-xs">
                    {t('settings.sections.quickPanel.doubleTap.control')}
                  </SelectItem>
                  <SelectItem value="meta" className="text-xs">
                    {metaLabel}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </SettingRow>
          <ShortcutRow
            definition={quickPanelDef}
            currentKey={getCurrentKey(quickPanelDef)}
            currentOverrides={overrides}
            isModified={isModified(quickPanelDef.id)}
            onOverrideChange={handleOverrideChange}
            onResetShortcut={handleResetShortcut}
          />
        </SettingGroup>
      )}
    </div>
  )
}
