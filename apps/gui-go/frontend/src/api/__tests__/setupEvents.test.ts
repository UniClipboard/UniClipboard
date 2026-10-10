import { beforeEach, describe, expect, it, vi } from 'vitest'
import { onDaemonRealtimeEvent } from '@/api/realtime'
import { onSetupRePairingRequired } from '@/api/setupEvents'

vi.mock('@/api/realtime', () => ({
  onDaemonRealtimeEvent: vi.fn(),
}))

describe('setup re-pairing event', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('forwards the dedicated all-devices requirement', async () => {
    const callback = vi.fn()
    vi.mocked(onDaemonRealtimeEvent).mockImplementation(async handler => {
      handler({
        topic: 'setup',
        type: 'setup.rePairingRequired',
        ts: 42,
        sessionId: null,
        payload: { scope: 'all_devices' },
      })
      return () => {}
    })

    await onSetupRePairingRequired(callback)

    expect(callback).toHaveBeenCalledWith({ scope: 'all_devices' })
  })
})
