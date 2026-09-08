import { configureStore } from '@reduxjs/toolkit'
import { beforeEach, expect, it, vi } from 'vitest'
import { getPairedPeersWithStatus } from '@/api/daemon/members'
import { refreshPresence } from '@/api/daemon/presence'
import devices, { refreshDeviceConnections, setSpaceMembers } from '@/store/slices/devicesSlice'

vi.mock('@/api/daemon/presence', () => ({ refreshPresence: vi.fn() }))
vi.mock('@/api/daemon/members', () => ({
  getPairedPeersWithStatus: vi.fn(),
  getLocalDeviceInfo: vi.fn(),
}))

const report = { total: 5, online: 3, offline: 1, errors: 1 }
const peer = {
  peerId: 'peer-1',
  deviceName: 'Laptop',
  pairingState: 'Trusted',
  connected: true,
  channel: 'direct' as const,
  lastSeenAtMs: null,
  connectionAddress: null,
}
const makeStore = () => configureStore({ reducer: { devices } })

beforeEach(() => vi.resetAllMocks())

it.each([false, true])(
  'joins the automatic check and shares its result (failure: %s)',
  async fail => {
    const store = makeStore()
    let finish!: () => void
    vi.mocked(refreshPresence).mockReturnValue(
      new Promise((resolve, reject) => {
        finish = () => (fail ? reject(new Error('unavailable')) : resolve(report))
      })
    )
    vi.mocked(getPairedPeersWithStatus).mockResolvedValue([peer])
    const automatic = store.dispatch(refreshDeviceConnections())
    const manual = store.dispatch(refreshDeviceConnections('manual'))
    expect(store.getState().devices.connectionRefreshTrigger).toBe('manual')
    expect(store.getState().devices.connectionRefresh.status).toBe('checking')
    const repeated = await store.dispatch(refreshDeviceConnections('manual'))
    expect(refreshDeviceConnections.rejected.match(repeated) && repeated.meta.condition).toBe(true)
    expect(refreshPresence).toHaveBeenCalledTimes(1)
    finish()
    const [automaticResult, manualResult] = await Promise.all([automatic, manual])
    expect(manualResult.type).toBe(automaticResult.type)
    expect(manualResult.payload).toEqual(automaticResult.payload)
    expect(store.getState().devices.connectionRefresh.status).toBe(fail ? 'failed' : 'complete')
    expect(getPairedPeersWithStatus).toHaveBeenCalledTimes(fail ? 0 : 1)
  }
)

it.each([undefined, 'manual'] as const)(
  'retains the refresh trigger %s through completion',
  async trigger => {
    const store = makeStore()
    let finish!: (value: typeof report) => void
    vi.mocked(refreshPresence).mockReturnValue(
      new Promise(resolve => {
        finish = resolve
      })
    )
    vi.mocked(getPairedPeersWithStatus).mockResolvedValue([peer])
    const refresh = store.dispatch(refreshDeviceConnections(trigger))
    expect(store.getState().devices.connectionRefreshTrigger).toBe(trigger ?? 'automatic')
    await store.dispatch(refreshDeviceConnections())
    expect(store.getState().devices.connectionRefreshTrigger).toBe(trigger ?? 'automatic')
    expect(refreshPresence).toHaveBeenCalledTimes(1)
    finish(report)
    await refresh
    expect(store.getState().devices.connectionRefreshTrigger).toBe(trigger ?? 'automatic')
  }
)

it('keeps existing and pushed devices while coalescing refreshes through the final list read', async () => {
  const store = makeStore()
  store.dispatch(setSpaceMembers([peer]))
  let finishCheck!: (value: typeof report) => void
  let finishList!: (value: (typeof peer)[]) => void
  vi.mocked(refreshPresence).mockReturnValue(
    new Promise(resolve => {
      finishCheck = resolve
    })
  )
  vi.mocked(getPairedPeersWithStatus).mockReturnValue(
    new Promise(resolve => {
      finishList = resolve
    })
  )
  const first = store.dispatch(refreshDeviceConnections())
  await store.dispatch(refreshDeviceConnections())
  expect(refreshPresence).toHaveBeenCalledTimes(1)
  expect(store.getState().devices.spaceMembers).toEqual([peer])
  store.dispatch(setSpaceMembers([{ ...peer, connected: false }]))
  finishCheck(report)
  await vi.waitFor(() => expect(getPairedPeersWithStatus).toHaveBeenCalledTimes(1))
  expect(store.getState().devices.connectionRefresh.status).toBe('checking')
  const joined = store.dispatch(refreshDeviceConnections('manual'))
  expect(store.getState().devices.connectionRefreshTrigger).toBe('manual')
  await store.dispatch(refreshDeviceConnections())
  expect(refreshPresence).toHaveBeenCalledTimes(1)
  finishList([peer])
  await first
  expect(refreshDeviceConnections.fulfilled.match(await joined)).toBe(true)
  expect(store.getState().devices.spaceMembers).toEqual([peer])
  expect(store.getState().devices.connectionRefresh).toEqual({ status: 'complete', report })
})

it('preserves devices on refresh failure and allows retry', async () => {
  const store = makeStore()
  store.dispatch(setSpaceMembers([peer]))
  vi.mocked(refreshPresence)
    .mockRejectedValueOnce(new Error('unavailable'))
    .mockResolvedValue(report)
  vi.mocked(getPairedPeersWithStatus).mockResolvedValue([peer])
  await store.dispatch(refreshDeviceConnections())
  expect(store.getState().devices.spaceMembers).toEqual([peer])
  expect(store.getState().devices.connectionRefresh.status).toBe('failed')
  expect(getPairedPeersWithStatus).not.toHaveBeenCalled()
  await store.dispatch(refreshDeviceConnections())
  expect(store.getState().devices.connectionRefresh.status).toBe('complete')
})

it('distinguishes a failed final list read and preserves incoming updates', async () => {
  const store = makeStore()
  store.dispatch(setSpaceMembers([peer]))
  vi.mocked(refreshPresence).mockResolvedValue(report)
  vi.mocked(getPairedPeersWithStatus).mockImplementation(async () => {
    store.dispatch(setSpaceMembers([{ ...peer, connected: false }]))
    throw new Error('list unavailable')
  })
  await store.dispatch(refreshDeviceConnections())
  expect(store.getState().devices.spaceMembers[0].connected).toBe(false)
  expect(store.getState().devices.connectionRefresh).toEqual({ status: 'list-failed', report })
})
