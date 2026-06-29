import { useMemo, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { ShortcutRow } from '@/components/setting/ShortcutRow'
import { Button } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import {
  SHORTCUT_DEFINITIONS,
  type ShortcutDefinition,
  type ShortcutScope,
} from '@/shortcuts/definitions'

const log = createLogger('shortcuts-section')

/** Display order for shortcut scopes */
const SCOPE_ORDER: ShortcutScope[] = ['global', 'clipboard']

const ShortcutsSection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, updateKeyboardShortcuts } = useSetting()
  // 锁住 overrides 引用，避免 `?? {}` 在 setting 未加载阶段每次都生成新的空
  // 对象，把下游 useCallback / React.memo 的稳定性击穿。
  const overrides = useMemo(() => setting?.keyboardShortcuts ?? {}, [setting?.keyboardShortcuts])

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

  const shortcutsById = useMemo(() => new Map(SHORTCUT_DEFINITIONS.map(d => [d.id, d])), [])

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
          const clearedDef = shortcutsById.get(clearedId)
          if (clearedDef) {
            const clearedDefaultKey = Array.isArray(clearedDef.key)
              ? clearedDef.key[0]
              : clearedDef.key
            if (clearedDefaultKey === newKey) {
              // Default key conflicts with the new key, so deleting the override
              // would revert to the conflicting default. Set empty string to unbind.
              newOverrides[clearedId] = ''
            } else {
              // Delete the override so it reverts to a non-conflicting default
              delete newOverrides[clearedId]
            }
          }
        }
      }

      try {
        await updateKeyboardShortcuts(newOverrides)
      } catch (error) {
        log.error({ err: error }, 'Failed to update keyboard shortcuts')
      }
    },
    [overrides, shortcutsById, updateKeyboardShortcuts]
  )

  // Handle single shortcut reset
  const handleResetShortcut = useCallback(
    async (id: string) => {
      const newOverrides = { ...overrides }
      delete newOverrides[id]
      try {
        await updateKeyboardShortcuts(newOverrides)
      } catch (error) {
        log.error({ err: error }, 'Failed to reset shortcut')
      }
    },
    [overrides, updateKeyboardShortcuts]
  )

  // Handle reset all shortcuts
  const handleResetAll = useCallback(async () => {
    try {
      await updateKeyboardShortcuts({})
    } catch (error) {
      log.error({ err: error }, 'Failed to reset all shortcuts')
    }
  }, [updateKeyboardShortcuts])

  return (
    <div className="space-y-6">
      {SCOPE_ORDER.map(scope => {
        const shortcuts = groupedShortcuts.get(scope)
        if (!shortcuts || shortcuts.length === 0) return null

        return (
          <SettingGroup key={scope} title={t(`settings.sections.shortcuts.scope.${scope}`)}>
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
          {t('settings.sections.shortcuts.resetAll')}
        </Button>
      </div>
    </div>
  )
}

export default ShortcutsSection
