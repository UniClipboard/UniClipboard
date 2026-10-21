import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, expect, it, vi } from 'vitest'
import { resetSettingsSyncForTest } from '@/lib/settings-sync'
import { emitDaemonWsEvent } from '@/test/fixtures/settings-ws'
import { useQuickPanelShortcutOverrides } from '../hooks/useQuickPanelShortcutOverrides'
const { load } = vi.hoisted(() => ({ load: vi.fn() }))
vi.mock('@/api/daemon', () => ({ getSettings: load }))
vi.mock('@/lib/daemon-ws', () => import('@/test/fixtures/settings-ws'))
vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn().mockResolvedValue(undefined),
}))
const changed = () =>
  emitDaemonWsEvent('settings.changed', {
    owner: 'desktop',
    sections: ['keyboardShortcuts'],
    revision: 1,
  })
beforeEach(() => {
  resetSettingsSyncForTest()
  load.mockReset()
})
it('loads persisted overrides and applies live changes', async () => {
  load.mockResolvedValue({ keyboardShortcuts: { 'quickPanel.windowIncrease': 'ctrl+k' } })
  const { result } = renderHook(() => useQuickPanelShortcutOverrides())
  await waitFor(() => expect(result.current).toEqual({ 'quickPanel.windowIncrease': 'ctrl+k' }))
  load.mockResolvedValue({ keyboardShortcuts: { 'quickPanel.windowIncrease': 'ctrl+j' } })
  act(changed)
  await waitFor(() => expect(result.current).toEqual({ 'quickPanel.windowIncrease': 'ctrl+j' }))
})
it('does not overwrite a newer read with a stale initial response', async () => {
  let resolve!: (value: unknown) => void
  load
    .mockReturnValueOnce(
      new Promise(done => {
        resolve = done
      })
    )
    .mockResolvedValue({ keyboardShortcuts: { 'quickPanel.textDecrease': 'ctrl+j' } })
  const { result } = renderHook(() => useQuickPanelShortcutOverrides())
  await waitFor(() => expect(load).toHaveBeenCalledOnce())
  act(changed)
  await act(async () => resolve({ keyboardShortcuts: { 'quickPanel.textDecrease': 'ctrl+k' } }))
  await waitFor(() => expect(result.current).toEqual({ 'quickPanel.textDecrease': 'ctrl+j' }))
})
