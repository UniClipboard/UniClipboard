import { describe, expect, it } from 'vitest'
import { classifyPairingError } from '../events'

describe('classifyPairingError', () => {
  it('classifies active_session_exists', () => {
    expect(classifyPairingError('active pairing session exists')).toBe('active_session_exists')
  })

  it('classifies no_local_participant', () => {
    expect(classifyPairingError('no local pairing participant ready')).toBe('no_local_participant')
  })

  it('classifies session_not_found', () => {
    expect(classifyPairingError('pairing session not found')).toBe('session_not_found')
    expect(classifyPairingError('session expired')).toBe('session_not_found')
  })

  it('classifies daemon_unavailable', () => {
    expect(classifyPairingError('failed to connect daemon websocket')).toBe('daemon_unavailable')
  })

  it('returns unknown for unrecognized errors', () => {
    expect(classifyPairingError('some random error')).toBe('unknown')
    expect(classifyPairingError(null)).toBe('unknown')
    expect(classifyPairingError(undefined)).toBe('unknown')
  })
})
