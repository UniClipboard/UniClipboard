import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetting } from '@/hooks/useSetting'
import { SHORTCUT_DEFINITIONS, type ShortcutDefinition } from '@/shortcuts/definitions'
import type { QuickPanelPosition } from '@/types/setting'
import { QuickPanelAccessibilityRow } from './QuickPanelAccessibilityRow'
import { QuickPanelDoubleTapRow } from './QuickPanelDoubleTapRow'
import { QuickPanelShortcutRow } from './QuickPanelShortcutRow'
import { SettingSelect, SettingSwitch } from './SettingControls'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'
import { useQuickPanelAccessibility } from './useQuickPanelAccessibility'
import { useShortcutSettings } from './useShortcutSettings'

const QUICK_PANEL_SHORTCUT_ID = 'global.toggleQuickPanel'

export default function QuickPanelSection() {
  const { t } = useTranslation()
  const { isMac } = usePlatform()
  const accessibility = useQuickPanelAccessibility(isMac)
  const { setting, updateQuickPanelSetting } = useSetting()

  const { overrides, getCurrentKey, isModified, handleOverrideChange, handleResetShortcut } =
    useShortcutSettings()
  const quickPanelDef = useMemo<ShortcutDefinition | undefined>(
    () => SHORTCUT_DEFINITIONS.find(def => def.id === QUICK_PANEL_SHORTCUT_ID),
    []
  )

  const [enabled, setEnabled] = useOptimisticSetting(
    setting?.quickPanel?.enabled ?? false,
    (next: boolean) => updateQuickPanelSetting({ enabled: next }),
    { failureLog: 'Failed to toggle quick panel' }
  )
  const [position, setPosition] = useOptimisticSetting<QuickPanelPosition>(
    setting?.quickPanel?.position ?? 'center',
    next => updateQuickPanelSetting({ position: next }),
    { failureLog: 'Failed to change quick panel position' }
  )
  return (
    <div className="flex min-w-0 flex-col gap-8">
      {isMac && (
        <SettingGroup title={t('settings.sections.quickPanel.accessibility.groupTitle')}>
          <QuickPanelAccessibilityRow
            status={accessibility.status}
            onRetry={() => void accessibility.refresh()}
          />
        </SettingGroup>
      )}
      <SettingGroup title={t('settings.sections.quickPanel.featureTitle')}>
        <SettingRow
          label={t('settings.sections.quickPanel.enable.label')}
          description={t('settings.sections.quickPanel.enable.description')}
        >
          <SettingSwitch
            label={t('settings.sections.quickPanel.enable.label')}
            checked={enabled}
            onCheckedChange={setEnabled}
          />
        </SettingRow>
        <SettingRow
          label={t('settings.sections.quickPanel.position.label')}
          description={t('settings.sections.quickPanel.position.description')}
        >
          <SettingSelect
            label={t('settings.sections.quickPanel.position.label')}
            value={position}
            onValueChange={value => setPosition(value as QuickPanelPosition)}
            disabled={!enabled}
            items={[
              {
                value: 'center',
                label: t('settings.sections.quickPanel.position.center'),
              },
              {
                value: 'follow_cursor',
                label: t('settings.sections.quickPanel.position.followCursor'),
              },
            ]}
          />
        </SettingRow>
      </SettingGroup>

      {quickPanelDef && (
        <SettingGroup title={t('settings.sections.quickPanel.shortcutTitle')}>
          <QuickPanelDoubleTapRow enabled={enabled} accessibilityStatus={accessibility.status} />
          <QuickPanelShortcutRow
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
