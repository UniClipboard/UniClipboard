import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { ShortcutRow } from '@/components/setting/ShortcutRow'
import { Button } from '@/components/ui'
import {
  SHORTCUT_DEFINITIONS,
  type ShortcutDefinition,
  type ShortcutScope,
} from '@/shortcuts/definitions'
import { QuickPanelShortcutRow } from './QuickPanelShortcutRow'
import { useShortcutSettings } from './useShortcutSettings'

/** Display order for shortcut scopes */
const SCOPE_ORDER: ShortcutScope[] = ['global', 'clipboard']

const ShortcutsSection: React.FC = () => {
  const { t } = useTranslation()
  const {
    overrides,
    getCurrentKey,
    isModified,
    handleOverrideChange,
    handleResetShortcut,
    handleResetAll,
  } = useShortcutSettings()

  const groupedShortcuts = useMemo(() => {
    const groups = new Map<ShortcutScope, ShortcutDefinition[]>()
    for (const def of SHORTCUT_DEFINITIONS) {
      const existing = groups.get(def.scope) ?? []
      existing.push(def)
      groups.set(def.scope, existing)
    }
    return groups
  }, [])

  const hasOverrides = Object.keys(overrides).length > 0

  return (
    <div className="flex min-w-0 flex-col gap-8">
      {SCOPE_ORDER.map(scope => {
        const shortcuts = groupedShortcuts.get(scope)
        if (!shortcuts || shortcuts.length === 0) return null

        return (
          <SettingGroup key={scope} title={t(`settings.sections.shortcuts.scope.${scope}`)}>
            {shortcuts.map(def => {
              const Row = def.id === 'global.toggleQuickPanel' ? QuickPanelShortcutRow : ShortcutRow
              return (
                <Row
                  key={def.id}
                  definition={def}
                  currentKey={getCurrentKey(def)}
                  currentOverrides={overrides}
                  isModified={isModified(def.id)}
                  onOverrideChange={handleOverrideChange}
                  onResetShortcut={handleResetShortcut}
                />
              )
            })}
          </SettingGroup>
        )
      })}

      <div className="flex justify-end pt-2">
        <Button variant="outline" size="sm" disabled={!hasOverrides} onClick={handleResetAll}>
          {t('settings.sections.shortcuts.resetAll')}
        </Button>
      </div>
    </div>
  )
}

export default ShortcutsSection
