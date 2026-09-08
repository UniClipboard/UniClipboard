import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useSetting } from '@/hooks/useSetting'
import { makeBaseSettings } from '@/test/fixtures/settings'
import { useShortcutSettings } from '../useShortcutSettings'

vi.mock('@/hooks/useSetting', () => ({ useSetting: vi.fn() }))
const persist = vi
  .fn<ReturnType<typeof useSetting>['updateKeyboardShortcuts']>()
  .mockResolvedValue(undefined)
beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(useSetting).mockReturnValue({
    setting: makeBaseSettings({
      keyboardShortcuts: { 'nav.settings': 'x', 'clipboard.favorite': 'y' },
    }),
    loading: false,
    error: null,
    updateKeyboardShortcuts: persist,
    reloadSetting: vi.fn(),
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
    updateAutostart: vi.fn(),
    updateSyncSetting: vi.fn(),
    updateFileSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateNetworkSetting: vi.fn(),
    updateQuickPanelSetting: vi.fn(),
    saveRelay: vi.fn(),
  })
})

describe('shared shortcut editing', () => {
  it('unbinds conflicting defaults but restores nonconflicting defaults', async () => {
    const { result } = renderHook(useShortcutSettings)
    await act(async () =>
      result.current.handleOverrideChange('global.toggleQuickPanel', 'mod+comma', [
        'nav.settings',
        'clipboard.favorite',
      ])
    )
    expect(persist).toHaveBeenCalledWith(
      { 'nav.settings': 'x', 'clipboard.favorite': 'y' },
      { 'nav.settings': '', 'global.toggleQuickPanel': 'mod+comma' }
    )
  })

  it('resets only the selected override and can reset all overrides', async () => {
    const { result } = renderHook(useShortcutSettings)
    await act(async () => result.current.handleResetShortcut('nav.settings'))
    expect(persist.mock.lastCall?.[1]).toEqual({ 'clipboard.favorite': 'y' })
    await act(async () => result.current.handleResetAll())
    expect(persist.mock.lastCall?.[1]).toEqual({})
  })
})
