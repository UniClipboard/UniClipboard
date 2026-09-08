import { describe, expect, it } from 'vitest'
import { appBootstrapReducer, initialAppBootstrapState } from '@/lib/app-bootstrap-state'

describe('appBootstrapReducer', () => {
  it('clears a previous connection error when the daemon becomes ready', () => {
    const failedState = appBootstrapReducer(initialAppBootstrapState, {
      type: 'connectionFailed',
      error: 'Connection refused',
      failure: null,
    })

    expect(appBootstrapReducer(failedState, { type: 'connectionReady' })).toEqual({
      ...initialAppBootstrapState,
      daemonBootstrapReady: true,
    })
  })

  it('records the failure detail reported by the native bootstrap check', () => {
    const failure = {
      kind: 'versionTooOld' as const,
      detail: 'Expected a newer app version',
      observedVersion: '2.0.0',
      expectedVersion: '2.1.0',
    }

    expect(
      appBootstrapReducer(initialAppBootstrapState, { type: 'bootstrapFailed', failure })
    ).toEqual({
      ...initialAppBootstrapState,
      bootEncryptionError: failure.detail,
      bootstrapFailure: failure,
    })
  })
})
