import { useMemo, useCallback } from 'react'
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
  const { setting, updateKeyboardShortcuts } = useSetting()
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

  // Handle override change with conflict clearing
  const handleOverrideChange = useCallback(
    async (id: string, newKey: string, clearedIds?: string[]) => {
      const newOverrides = { ...overrides }

      // Set the new shortcut override
      newOverrides[id] = newKey

      // If there's a conflict that needs to be cleared, remove those overrides
      if (clearedIds && clearedIds.length > 0) {
        for (const clearedId of clearedIds) {
          // Check if the cleared shortcut's default key equals the new key
          const clearedDef = SHORTCUT_DEFINITIONS.find(d => d.id === clearedId)
          if (clearedDef) {
            const clearedDefaultKey = Array.isArray(clearedDef.key)
              ? clearedDef.key[0]
              : clearedDef.key
            if (clearedDefaultKey === newKey) {
              // Just delete the override entry (effectively resetting it to default)
              delete newOverrides[clearedId]
            } else {
              // Delete the override so it reverts to default
              delete newOverrides[clearedId]
            }
          }
        }
      }

      await updateKeyboardShortcuts(newOverrides)
    },
    [overrides, updateKeyboardShortcuts]
  )

  // Handle single shortcut reset
  const handleResetShortcut = useCallback(
    async (id: string) => {
      const newOverrides = { ...overrides }
      delete newOverrides[id]
      await updateKeyboardShortcuts(newOverrides)
    },
    [overrides, updateKeyboardShortcuts]
  )

  // Handle reset all shortcuts
  const handleResetAll = useCallback(async () => {
    await updateKeyboardShortcuts({})
  }, [updateKeyboardShortcuts])

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
                currentOverrides={overrides}
                isModified={isModified(def.id)}
                onOverrideChange={handleOverrideChange}
                onResetShortcut={handleResetShortcut}
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
