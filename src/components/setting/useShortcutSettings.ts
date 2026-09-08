import { useCallback, useMemo } from 'react'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { SHORTCUT_DEFINITIONS, type ShortcutDefinition } from '@/shortcuts/definitions'

const log = createLogger('shortcut-settings')
const definitionsById = new Map(SHORTCUT_DEFINITIONS.map(def => [def.id, def]))

export function useShortcutSettings() {
  const { setting, updateKeyboardShortcuts } = useSetting()
  const overrides = useMemo(() => setting?.keyboardShortcuts ?? {}, [setting?.keyboardShortcuts])

  const persist = useCallback(
    async (next: typeof overrides) => {
      try {
        await updateKeyboardShortcuts(overrides, next)
      } catch (err) {
        log.error({ err }, 'Failed to update keyboard shortcuts')
      }
    },
    [overrides, updateKeyboardShortcuts]
  )

  const handleOverrideChange = useCallback(
    async (id: string, newKey: string, clearedIds: string[] = []) => {
      const next = { ...overrides, [id]: newKey }
      for (const clearedId of clearedIds) {
        const definition = definitionsById.get(clearedId)
        if (!definition) continue
        const defaultKey = Array.isArray(definition.key) ? definition.key[0] : definition.key
        // Removing this override must not restore a conflicting default binding.
        if (defaultKey === newKey) next[clearedId] = ''
        else delete next[clearedId]
      }
      await persist(next)
    },
    [overrides, persist]
  )

  const handleResetShortcut = useCallback(
    async (id: string) => {
      const next = { ...overrides }
      delete next[id]
      await persist(next)
    },
    [overrides, persist]
  )

  const handleResetAll = useCallback(() => persist({}), [persist])
  const getCurrentKey = (definition: ShortcutDefinition): string => {
    const override = overrides[definition.id]
    if (override != null)
      return Array.isArray(override) ? (override[0] ?? String(definition.key)) : override
    return Array.isArray(definition.key) ? (definition.key[0] ?? '') : definition.key
  }

  return {
    overrides,
    getCurrentKey,
    isModified: (id: string) => id in overrides,
    handleOverrideChange,
    handleResetShortcut,
    handleResetAll,
  }
}
