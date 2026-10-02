import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, expect, it, vi } from 'vitest'
import { useOmarchyTheme } from '@/hooks/useOmarchyTheme'
import { subscribeDesktopTheme } from '@/lib/desktop-theme'
import { commands } from '@/lib/ipc'
import type { DesktopThemeSnapshot } from '@/lib/ipc-bindings.generated'

vi.mock('@/lib/desktop-theme', () => ({ subscribeDesktopTheme: vi.fn() }))
vi.mock('@/lib/ipc', () => ({ commands: { setFollowOmarchyTheme: vi.fn() } }))

let receive: Parameters<typeof subscribeDesktopTheme>[0]
const initial: DesktopThemeSnapshot = {
  revision: 1,
  followOmarchyTheme: false,
  omarchyAvailable: true,
  theme: null,
  windowCornerRadius: null,
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(subscribeDesktopTheme).mockImplementation(callback => {
    receive = callback
    callback(null, null, initial)
    return vi.fn()
  })
})

it('keeps the confirmed preference when saving fails', async () => {
  vi.mocked(commands.setFollowOmarchyTheme).mockRejectedValue(new Error('disk unavailable'))
  const { result } = renderHook(() => useOmarchyTheme())
  await act(() => result.current.setEnabled(true))
  expect(result.current.enabled).toBe(false)
  expect(result.current.failed).toBe(true)
  expect(result.current.saving).toBe(false)
})

it('ignores a save response older than an event from another window', async () => {
  let resolve!: (snapshot: DesktopThemeSnapshot) => void
  vi.mocked(commands.setFollowOmarchyTheme).mockReturnValue(
    new Promise(done => {
      resolve = done
    })
  )
  const { result } = renderHook(() => useOmarchyTheme())
  let saving!: Promise<void>
  act(() => {
    saving = result.current.setEnabled(true)
  })
  expect(result.current.enabled).toBe(false)
  act(() => receive(null, null, { ...initial, revision: 3 }))
  await act(async () => {
    resolve({ ...initial, revision: 2, followOmarchyTheme: true })
    await saving
  })
  await waitFor(() => expect(result.current.saving).toBe(false))
  expect(result.current.enabled).toBe(false)
})
