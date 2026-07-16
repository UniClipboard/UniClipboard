/**
 * @vitest-environment jsdom
 */

import { describe, expect, it } from 'vitest'
import { resolveEncryptionStatus } from '@/lib/app-state'
import { shouldSignalDaemonLifecycleReady } from '@/lib/daemon-lifecycle-ready'

describe('shouldSignalDaemonLifecycleReady', () => {
  it('returns true when setup is complete, daemon is connected, and encryption session is ready', () => {
    expect(
      shouldSignalDaemonLifecycleReady(false, true, {
        initialized: true,
        session_ready: true,
      })
    ).toBe(true)
  })

  it('returns false while setup is still active', () => {
    expect(
      shouldSignalDaemonLifecycleReady(true, true, {
        initialized: true,
        session_ready: true,
      })
    ).toBe(false)
  })

  it('returns false before daemon bootstrap finishes', () => {
    expect(
      shouldSignalDaemonLifecycleReady(false, false, {
        initialized: true,
        session_ready: true,
      })
    ).toBe(false)
  })

  it('returns false when encryption is not yet ready', () => {
    expect(
      shouldSignalDaemonLifecycleReady(false, true, {
        initialized: true,
        session_ready: false,
      })
    ).toBe(false)
  })
})

describe('resolveEncryptionStatus', () => {
  it('keeps a newer realtime-ready override over a stale locked query', () => {
    expect(
      resolveEncryptionStatus(
        { initialized: true, session_ready: false },
        { initialized: true, session_ready: true }
      )
    ).toEqual({ initialized: true, session_ready: true })
  })

  it('uses the query snapshot before a realtime or user override exists', () => {
    expect(resolveEncryptionStatus({ initialized: true, session_ready: false }, null)).toEqual({
      initialized: true,
      session_ready: false,
    })
  })
})
