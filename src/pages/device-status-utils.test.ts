import { describe, expect, it } from 'vitest'
import { isRemovalInProgress, isWaitingForDeviceUpdate } from './device-status-utils'

describe('isRemovalInProgress', () => {
  it('does not treat ordinary admission convergence as member removal', () => {
    expect(
      isRemovalInProgress({
        phase: 'converging',
        removalIntentCount: 0,
        removed: false,
      })
    ).toBe(false)
  })

  it('recognizes a convergence state with a removal intent', () => {
    expect(
      isRemovalInProgress({
        phase: 'converging',
        removalIntentCount: 1,
        removed: false,
      })
    ).toBe(true)
  })
})

describe('isWaitingForDeviceUpdate', () => {
  it('marks only the Engine-identified offline member as waiting for an update', () => {
    const state = {
      phase: 'waiting_for_offline_member',
      waitingMemberDeviceIds: ['peer-b'],
    }

    expect(isWaitingForDeviceUpdate(state, 'peer-b')).toBe(true)
    expect(isWaitingForDeviceUpdate(state, 'peer-c')).toBe(false)
  })

  it('does not mark devices outside the waiting phase', () => {
    expect(
      isWaitingForDeviceUpdate(
        { phase: 'converging', waitingMemberDeviceIds: ['peer-b'] },
        'peer-b'
      )
    ).toBe(false)
  })
})
