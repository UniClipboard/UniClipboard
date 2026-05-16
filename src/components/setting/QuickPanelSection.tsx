import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RestartBanner } from './RestartBanner'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { ShortcutRow } from './ShortcutRow'
import { Switch } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { SHORTCUT_DEFINITIONS, type ShortcutDefinition } from '@/shortcuts/definitions'

const log = createLogger('quick-panel-section')

const QUICK_PANEL_SHORTCUT_ID = 'global.toggleQuickPanel'

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
  const { setting, loading, updateQuickPanelSetting, updateKeyboardShortcuts } = useSetting()

  const overrides = setting?.keyboardShortcuts ?? {}
  const quickPanelDef = useMemo<ShortcutDefinition | undefined>(
    () => SHORTCUT_DEFINITIONS.find(def => def.id === QUICK_PANEL_SHORTCUT_ID),
    []
  )

  const enabled = setting?.quickPanel?.enabled ?? false
  const [saving, setSaving] = useState(false)
  const isBusy = loading || saving

  // 用户在本次会话里从开启切到关闭后,显示"重启以彻底释放资源"提示。
  // 不持久化:重启 GUI 后这条 hint 自然消失,因为启动期 pre_create 已经
  // 因 enabled=false 跳过,资源也已释放。再次开启时清掉提示。
  const [disabledThisSession, setDisabledThisSession] = useState(false)
  const [restartLoading, setRestartLoading] = useState(false)
  const [restartError, setRestartError] = useState<string | null>(null)
  const restartHintVisible = disabledThisSession && !enabled

  const handleEnabledChange = async (next: boolean) => {
    try {
      setSaving(true)
      await updateQuickPanelSetting({ enabled: next })
      setDisabledThisSession(prev => (next ? false : prev || true))
      if (next) {
        // 重新开启 → 隐藏窗口已复用,旧的 restart 错误也作废。
        setRestartError(null)
      }
    } catch (err) {
      log.error({ err }, '更改快捷面板开关失败')
    } finally {
      setSaving(false)
    }
  }

  const handleRestart = async () => {
    setRestartLoading(true)
    setRestartError(null)
    try {
      // app.restart() 不返回(进程会 exit),后续代码理论上不可达;
      // 走到 catch 说明 spawn 本身就失败了。
      await commands.restartApp()
    } catch (err) {
      log.error({ err }, '快捷面板关闭后重启应用失败')
      setRestartError(t('settings.restartBanner.errorMessage'))
      setRestartLoading(false)
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

  const handleOverrideChange = useCallback(
    async (id: string, newKey: string, clearedIds?: string[]) => {
      const newOverrides = { ...overrides }
      newOverrides[id] = newKey
      if (clearedIds && clearedIds.length > 0) {
        for (const clearedId of clearedIds) {
          const clearedDef = SHORTCUT_DEFINITIONS.find(d => d.id === clearedId)
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
        await updateKeyboardShortcuts(newOverrides)
      } catch (err) {
        log.error({ err }, '更新快捷面板快捷键失败')
      }
    },
    [overrides, updateKeyboardShortcuts]
  )

  const handleResetShortcut = useCallback(
    async (id: string) => {
      const newOverrides = { ...overrides }
      delete newOverrides[id]
      try {
        await updateKeyboardShortcuts(newOverrides)
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
          <Switch checked={enabled} onCheckedChange={handleEnabledChange} disabled={isBusy} />
        </SettingRow>
      </SettingGroup>

      {quickPanelDef && (
        <SettingGroup title={t('settings.sections.quickPanel.shortcutTitle')}>
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
