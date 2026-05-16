import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { ShortcutRow } from './ShortcutRow'
import { Switch } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { SHORTCUT_DEFINITIONS, type ShortcutDefinition } from '@/shortcuts/definitions'

const log = createLogger('quick-panel-section')

const QUICK_PANEL_SHORTCUT_ID = 'global.toggleQuickPanel'

/**
 * Quick panel feature section. 切换 enabled 会通过
 * `set_quick_panel_enabled` Tauri command 即时注册/反注册全局快捷键并
 * 创建/销毁隐藏面板窗口——不需要重启 GUI。
 *
 * 这里把"切换快捷面板"的快捷键也一并展示出来，让用户在同一个 section
 * 内完成"开关 + 配快捷键"两件事；Shortcuts section 里仍然保留同一行，
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

  const handleEnabledChange = async (next: boolean) => {
    try {
      setSaving(true)
      await updateQuickPanelSetting({ enabled: next })
    } catch (err) {
      log.error({ err }, '更改快捷面板开关失败')
    } finally {
      setSaving(false)
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
