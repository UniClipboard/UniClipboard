import { describe, expect, it } from 'vitest'
import { SHORTCUT_DEFINITIONS } from '@/shortcuts/definitions'

describe('history shortcut definitions', () => {
  it('exposes search focus in shortcut settings', () => {
    expect(SHORTCUT_DEFINITIONS).toContainEqual(
      expect.objectContaining({
        id: 'clipboard.search',
        key: 'mod+f',
        scope: 'clipboard',
        description: 'settings.sections.shortcuts.actions.searchHistory',
      })
    )
  })
})
