import { describe, expect, it } from 'vitest'
import { activeDeviceIds, findNewActiveDeviceId } from '@/components/device/pairing-success-utils'

describe('pairing success helpers', () => {
  it('finds a newly active device by id', () => {
    expect(findNewActiveDeviceId(new Set(['local', 'old']), new Set(['local', 'new']))).toBe('new')
  })

  it('does not treat an unchanged active device set as pairing success', () => {
    expect(findNewActiveDeviceId(new Set(['local', 'old']), new Set(['local', 'old']))).toBeNull()
  })

  it('ignores removed devices when building the active device baseline', () => {
    expect(
      activeDeviceIds({
        devices: [
          { deviceId: 'local', membership: 'active' },
          { deviceId: 'peer', membership: 'active' },
          { deviceId: 'removed', membership: 'removed' },
        ],
      })
    ).toEqual(new Set(['local', 'peer']))
  })
})
