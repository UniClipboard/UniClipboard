import { RefreshCw } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
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
 * Quick panel feature section. 启用开关后需要重启 GUI 才会生效——启动期决定
 * 是否注册全局快捷键 + 预创建快捷面板窗口（参考 `uc-tauri/src/run.rs`）。
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

  // 记录进入设置页时的初始 enabled 值。后续 enabled 与之不一致时显示
  // "需重启"提示——避免用户切了开关却没意识到要重启 GUI 才生效。
  const [initialEnabled, setInitialEnabled] = useState<boolean | null>(null)
  useEffect(() => {
    if (initialEnabled === null && setting) {
      setInitialEnabled(enabled)
    }
  }, [enabled, initialEnabled, setting])
  const restartHintVisible = initialEnabled !== null && initialEnabled !== enabled

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
        {restartHintVisible && (
          <div
            role="status"
            aria-live="polite"
            className="flex items-start gap-2 px-4 py-3 bg-accent/40 border-b border-border/40"
          >
            <RefreshCw className="size-4 text-foreground mt-0.5 shrink-0" aria-hidden="true" />
            <p className="text-sm text-foreground">
              {t('settings.sections.quickPanel.restartHint')}
            </p>
          </div>
        )}
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
