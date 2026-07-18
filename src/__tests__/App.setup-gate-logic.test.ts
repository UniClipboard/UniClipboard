import { describe, expect, it } from 'vitest'
import { isSetupGateActive } from '@/lib/app-state'
import type { SetupFlow } from '@/store/setupRealtimeStore'

const completed: SetupFlow = { kind: 'completed', deviceName: 'host', completion: null }
const completedWithSummary: SetupFlow = {
  kind: 'completed',
  deviceName: 'host',
  completion: { kind: 'space_ready' },
}
const entry: SetupFlow = { kind: 'entry' }
const loading: SetupFlow = { kind: 'loading' }

describe('App setup gate logic', () => {
  it('keeps setup active while the shared setup store is hydrating', () => {
    expect(isSetupGateActive(loading, false)).toBe(true)
  })

  it('skips setup when hydration is complete and the flow is already completed', () => {
    expect(isSetupGateActive(completed, true)).toBe(false)
  })

  it('keeps the completed step visible while its summary is pending', () => {
    expect(isSetupGateActive(entry, true)).toBe(true)
    expect(isSetupGateActive(completedWithSummary, true)).toBe(true)
  })

  it('does not keep the gate open when the device was already completed at launch', () => {
    expect(isSetupGateActive(completed, true)).toBe(false)
  })
})
