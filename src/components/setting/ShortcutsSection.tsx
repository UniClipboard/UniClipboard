import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingGroup } from './SettingGroup'
import { ShortcutRow } from './ShortcutRow'
import { Button } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import {
  SHORTCUT_DEFINITIONS,
  type ShortcutDefinition,
  type ShortcutScope,
} from '@/shortcuts/definitions'

/** Display order for shortcut scopes */
const SCOPE_ORDER: ShortcutScope[] = ['global', 'clipboard', 'settings', 'devices', 'modal']

const ShortcutsSection: React.FC = () => {
  const { t } = useTranslation()
  const { setting } = useSetting()
  const overrides = setting?.keyboard_shortcuts ?? {}

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

  const getCurrentKey = (def: ShortcutDefinition): string => {
    const override = overrides[def.id]
    if (override != null) {
      return Array.isArray(override) ? (override[0] ?? String(def.key)) : override
    }
    return Array.isArray(def.key) ? (def.key[0] ?? '') : def.key
  }

  const isModified = (defId: string): boolean => {
    return defId in overrides
  }

  // Stub handlers for Plan 02
  const handleEdit = () => {
    // Will be implemented in Plan 02
  }

  const handleReset = () => {
    // Will be implemented in Plan 02
  }

  const handleResetAll = () => {
    // Will be implemented in Plan 02
  }

  return (
    <div className="space-y-6">
      {SCOPE_ORDER.map(scope => {
        const shortcuts = groupedShortcuts.get(scope)
        if (!shortcuts || shortcuts.length === 0) return null

        return (
          <SettingGroup key={scope} title={t(`settings.shortcuts.scope.${scope}`)}>
            {shortcuts.map(def => (
              <ShortcutRow
                key={def.id}
                definition={def}
                currentKey={getCurrentKey(def)}
                isModified={isModified(def.id)}
                onEdit={handleEdit}
                onReset={handleReset}
              />
            ))}
          </SettingGroup>
        )
      })}

      <div className="flex justify-end pt-2">
        <Button variant="outline" size="sm" disabled={!hasOverrides} onClick={handleResetAll}>
          {t('settings.shortcuts.resetAll')}
        </Button>
      </div>
    </div>
  )
}

export default ShortcutsSection
