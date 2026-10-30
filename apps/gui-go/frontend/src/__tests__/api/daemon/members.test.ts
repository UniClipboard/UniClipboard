import { beforeEach, describe, expect, it, vi } from 'vitest'
import { unpairDevice } from '@/api/daemon/members'

const mocks = vi.hoisted(() => ({ unpair: vi.fn(), update: vi.fn(), warn: vi.fn() }))
vi.mock('@/api/generated/sdk.gen', () => ({
  unpairDevice: mocks.unpair,
  getLocalDeviceInfo: vi.fn(),
  listPairedDevices: vi.fn(),
}))
vi.mock('@/api/daemon/client', () => ({
  daemonClient: { callEnveloped: (call: () => Promise<unknown>) => call() },
}))
vi.mock('@/api/daemon/settings', () => ({ updateSettings: mocks.update }))
vi.mock('@/lib/logger', () => ({ createLogger: () => ({ warn: mocks.warn }) }))

describe('unpair address cleanup', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.unpair.mockResolvedValue({ removalPhase: 'complete' })
    mocks.update.mockResolvedValue({ success: true, restartRequired: false })
  })
  it('removes only the unpaired device address after success', async () => {
    await expect(unpairDevice('peer')).resolves.toEqual({ removalPhase: 'complete' })
    expect(mocks.unpair).toHaveBeenCalledWith({ body: { peerId: 'peer' }, throwOnError: true })
    expect(mocks.update).toHaveBeenCalledWith({ network: { peerAddresses: { peer: null } } })
    expect(mocks.unpair.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.update.mock.invocationCallOrder[0]
    )
  })
  it('does not clean an address when unpair fails', async () => {
    mocks.unpair.mockRejectedValue(new Error('unpair failed'))
    await expect(unpairDevice('peer')).rejects.toThrow('unpair failed')
    expect(mocks.update).not.toHaveBeenCalled()
  })
  it('does not fail unpair or log address material when cleanup fails', async () => {
    mocks.update.mockRejectedValue(new Error('sensitive address material'))
    await expect(unpairDevice('peer')).resolves.toEqual({ removalPhase: 'complete' })
    expect(mocks.warn).toHaveBeenCalledWith('Failed to clear configured peer address after unpair')
  })
  it('does not wait for cleanup to finish', async () => {
    mocks.update.mockReturnValue(new Promise(() => {}))
    await expect(unpairDevice('peer')).resolves.toEqual({ removalPhase: 'complete' })
  })
})
