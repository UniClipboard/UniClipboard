import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
let subscribeDesktopTheme: typeof import('@/lib/desktop-theme').subscribeDesktopTheme
import { commands } from '@/lib/ipc'
import type { DesktopThemeSnapshot } from '@/lib/ipc-bindings.generated'

vi.mock('@tauri-apps/api/core', () => ({ isTauri: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))
vi.mock('@/lib/ipc', () => ({ commands: { getDesktopTheme: vi.fn() } }))

const snapshot = (revision: number): DesktopThemeSnapshot => ({
  revision,
  followOmarchyTheme: true,
  omarchyAvailable: true,
  windowCornerRadius: 12,
  theme: { dark: true, variables: { '--background': '#2d353b' } },
})

beforeEach(async () => {
  vi.resetAllMocks()
  vi.resetModules()
  ;({ subscribeDesktopTheme } = await import('@/lib/desktop-theme'))
})

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
    expect(apply).toHaveBeenCalledWith(snapshot(2).theme, 12, snapshot(2))
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

it('replays the startup palette synchronously to the React theme owner', async () => {
  vi.mocked(isTauri).mockReturnValue(true)
  vi.mocked(listen).mockResolvedValue(vi.fn())
  vi.mocked(commands.getDesktopTheme).mockResolvedValue(snapshot(3))
  const first = vi.fn()
  const dispose = subscribeDesktopTheme(first)
  await vi.waitFor(() => expect(first).toHaveBeenCalledOnce())
  dispose()
  const next = vi.fn()
  const disposeNext = subscribeDesktopTheme(next)
  expect(next).toHaveBeenCalledWith(snapshot(3).theme, 12, snapshot(3))
  disposeNext()
})

it('settles startup without a desktop palette when querying fails', async () => {
  vi.mocked(isTauri).mockReturnValue(true)
  vi.mocked(listen).mockResolvedValue(vi.fn())
  vi.mocked(commands.getDesktopTheme).mockRejectedValue(new Error('unavailable'))
  const apply = vi.fn()
  const dispose = subscribeDesktopTheme(apply)
  await vi.waitFor(() => expect(apply).toHaveBeenCalledWith(null))
  dispose()
})

it('still queries the initial palette when listener registration fails', async () => {
  vi.mocked(isTauri).mockReturnValue(true)
  vi.mocked(listen).mockRejectedValue(new Error('unavailable'))
  vi.mocked(commands.getDesktopTheme).mockResolvedValue(snapshot(1))
  const apply = vi.fn()
  const dispose = subscribeDesktopTheme(apply)
  await vi.waitFor(() => expect(apply).toHaveBeenCalledWith(snapshot(1).theme, 12, snapshot(1)))
  dispose()
})

it('suppresses the palette when disabled or unavailable while retaining preference metadata', async () => {
  vi.mocked(isTauri).mockReturnValue(true)
  let receive!: (event: { payload: DesktopThemeSnapshot }) => void
  vi.mocked(listen).mockImplementation(async (_event, callback) => {
    receive = callback as typeof receive
    return vi.fn<() => void>()
  })
  const disabled = { ...snapshot(1), followOmarchyTheme: false }
  vi.mocked(commands.getDesktopTheme).mockResolvedValue(disabled)
  const apply = vi.fn()
  const dispose = subscribeDesktopTheme(apply)
  await vi.waitFor(() => expect(apply).toHaveBeenCalledWith(null, 12, disabled))
  const enabled = snapshot(2)
  receive({ payload: enabled })
  expect(apply).toHaveBeenLastCalledWith(enabled.theme, 12, enabled)
  const unavailable = { ...snapshot(3), omarchyAvailable: false }
  receive({ payload: unavailable })
  expect(apply).toHaveBeenLastCalledWith(null, 12, unavailable)
  dispose()
})
