import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'

const { isPermissionGranted, sendNotification } = vi.hoisted(() => ({
  isPermissionGranted: vi.fn(),
  sendNotification: vi.fn(),
}))

vi.mock('@tauri-apps/plugin-notification', () => ({ isPermissionGranted, sendNotification }))

const snapshot: DeviceTrustSnapshot = {
  revision: 1,
  localDeviceId: 'local',
  localMembership: 'active',
  currentChange: {
    changeId: 'notification-permission-change',
    proposedByDeviceId: 'peer',
    targetDeviceIds: ['target'],
    includesLocalDevice: false,
    applyImpact: {
      usableDeviceIds: ['local'],
      pausedDeviceIds: [],
      localDeviceOutcome: 'active',
      requiresRejoinDeviceIds: ['target'],
    },
    keepCurrentImpact: {
      usableDeviceIds: ['local', 'target'],
      pausedDeviceIds: ['peer'],
      localDeviceOutcome: 'active',
      requiresRejoinDeviceIds: [],
    },
    allowedChoices: ['apply_change', 'keep_current_device_group'],
    blockedReason: null,
  },
  devices: [],
  recovery: 'not_available_in_this_version',
  allowedActions: [],
  blockedReason: null,
  updatedAtMs: 1,
}

describe('notifyDeviceTrustSnapshot', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.resetModules()
  })

  it('retries the same notification after permission becomes available', async () => {
    isPermissionGranted.mockResolvedValueOnce(false).mockResolvedValueOnce(true)
    const { notifyDeviceTrustSnapshot } = await import('@/lib/device-trust-notifications')

    await notifyDeviceTrustSnapshot(snapshot)
    await notifyDeviceTrustSnapshot(snapshot)

    expect(sendNotification).toHaveBeenCalledTimes(1)
  })
})
