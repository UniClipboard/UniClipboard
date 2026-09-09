import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { subscribeDesktopTheme } from '@/lib/desktop-theme'
import { commands } from '@/lib/ipc'
import type { DesktopThemeSnapshot } from '@/lib/ipc-bindings.generated'

vi.mock('@tauri-apps/api/core', () => ({ isTauri: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))
vi.mock('@/lib/ipc', () => ({ commands: { getDesktopTheme: vi.fn() } }))

const snapshot = (revision: number): DesktopThemeSnapshot => ({
  revision,
  windowCornerRadius: 12,
  theme: { dark: true, variables: { '--background': '#2d353b' } },
})

beforeEach(() => vi.resetAllMocks())

describe('desktop palette subscription', () => {
  it('does not access native APIs outside Tauri', () => {
    vi.mocked(isTauri).mockReturnValue(false)
    subscribeDesktopTheme(vi.fn())()
    expect(listen).not.toHaveBeenCalled()
    expect(commands.getDesktopTheme).not.toHaveBeenCalled()
  })

  it('subscribes before reading and rejects a stale startup response', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    let receive!: (event: { payload: DesktopThemeSnapshot }) => void
    const unlisten = vi.fn()
    vi.mocked(listen).mockImplementation(async (_event, callback) => {
      receive = callback as typeof receive
      return unlisten
    })
    let resolveRequest!: (value: DesktopThemeSnapshot) => void
    const request = new Promise<DesktopThemeSnapshot>(resolve => {
      resolveRequest = resolve
    })
    vi.mocked(commands.getDesktopTheme).mockReturnValue(request)
    const apply = vi.fn()
    const dispose = subscribeDesktopTheme(apply)
    await vi.waitFor(() => expect(commands.getDesktopTheme).toHaveBeenCalledOnce())
    receive({ payload: snapshot(2) })
    resolveRequest(snapshot(1))
    await request
    await Promise.resolve()
    expect(apply).toHaveBeenCalledWith(snapshot(2).theme, 12)
    expect(apply).toHaveBeenCalledOnce()
    receive({ payload: snapshot(3) })
    expect(apply).toHaveBeenCalledTimes(2)
    dispose()
    receive({ payload: snapshot(4) })
    expect(apply).toHaveBeenCalledTimes(2)
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('unsubscribes if disposed while native listener registration is pending', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    let resolveRegistration!: (value: () => void) => void
    const registration = new Promise<() => void>(resolve => {
      resolveRegistration = resolve
    })
    vi.mocked(listen).mockReturnValue(registration)
    const dispose = subscribeDesktopTheme(vi.fn())
    dispose()
    const unlisten = vi.fn()
    resolveRegistration(unlisten)
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledOnce())
    expect(commands.getDesktopTheme).not.toHaveBeenCalled()
  })
})

it('refreshes the latest palette when a suspended window becomes visible again', async () => {
  vi.mocked(isTauri).mockReturnValue(true)
  vi.mocked(listen).mockResolvedValue(vi.fn())
  vi.mocked(commands.getDesktopTheme).mockResolvedValue(snapshot(1))
  const apply = vi.fn()
  const dispose = subscribeDesktopTheme(apply)
  await vi.waitFor(() => expect(apply).toHaveBeenCalledOnce())
  vi.mocked(commands.getDesktopTheme).mockResolvedValue(snapshot(2))
  document.dispatchEvent(new Event('visibilitychange'))
  await vi.waitFor(() => expect(apply).toHaveBeenCalledTimes(2))
  dispose()
  document.dispatchEvent(new Event('visibilitychange'))
  expect(commands.getDesktopTheme).toHaveBeenCalledTimes(2)
})
