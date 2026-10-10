import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, expect, it, vi } from 'vitest'
import { useQuickPanelShortcutOverrides } from '../hooks/useQuickPanelShortcutOverrides'
const { load, listen, dispose } = vi.hoisted(() => ({
  load: vi.fn(),
  listen: vi.fn(),
  dispose: vi.fn(),
}))
vi.mock('@/api/daemon', () => ({ getSettings: load }))
vi.mock('@tauri-apps/api/event', () => ({ listen, emit: vi.fn() }))
beforeEach(() => {
  listen.mockReset().mockResolvedValue(dispose)
  load.mockReset()
  dispose.mockClear()
})
it('loads persisted overrides, applies live changes and unsubscribes', async () => {
  load.mockResolvedValue({ keyboardShortcuts: { 'quickPanel.windowIncrease': 'ctrl+k' } })
  const { result, unmount } = renderHook(() => useQuickPanelShortcutOverrides())
  await waitFor(() => expect(result.current).toEqual({ 'quickPanel.windowIncrease': 'ctrl+k' }))
  act(() =>
    listen.mock.calls[0][1]({
      payload: {
        settingJson: JSON.stringify({
          keyboardShortcuts: { 'quickPanel.windowIncrease': 'ctrl+j' },
        }),
      },
    })
  )
  expect(result.current).toEqual({ 'quickPanel.windowIncrease': 'ctrl+j' })
  unmount()
  await waitFor(() => expect(dispose).toHaveBeenCalledOnce())
})
it('does not overwrite a live update with a stale initial response', async () => {
  let resolve!: (value: unknown) => void
  load.mockReturnValue(
    new Promise(done => {
      resolve = done
    })
  )
  const { result } = renderHook(() => useQuickPanelShortcutOverrides())
  await waitFor(() => expect(load).toHaveBeenCalledOnce())
  act(() =>
    listen.mock.calls[0][1]({
      payload: {
        settingJson: JSON.stringify({ keyboardShortcuts: { 'quickPanel.textDecrease': 'ctrl+j' } }),
      },
    })
  )
  await act(async () => resolve({ keyboardShortcuts: { 'quickPanel.textDecrease': 'ctrl+k' } }))
  expect(result.current).toEqual({ 'quickPanel.textDecrease': 'ctrl+j' })
})
