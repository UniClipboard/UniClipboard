import { describe, expect, it } from 'vitest'
import { resolveSetupGate } from '@/lib/app-state'
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
  it('leaves unknown setup state with the app startup owner', () => {
    expect(resolveSetupGate(loading, false)).toBe('loading')
    expect(resolveSetupGate(loading, true)).toBe('loading')
  })

  it('skips setup when hydration is complete and the flow is already completed', () => {
    expect(resolveSetupGate(completed, true)).toBe('ready')
  })

  it('keeps the completed step visible while its summary is pending', () => {
    expect(resolveSetupGate(entry, true)).toBe('setup')
    expect(resolveSetupGate(completedWithSummary, true)).toBe('setup')
  })

  it('does not keep the gate open when the device was already completed at launch', () => {
    expect(resolveSetupGate(completed, true)).toBe('ready')
  })
})
