import { expect, it } from 'vitest'
import { publicEngineEvent } from './conflict-evidence.mjs'

it('exports only timestamp, role and structural Engine event fields', () => {
  const event = publicEngineEvent(
    {
      timestamp: '2026-09-07T05:12:35.079Z',
      target: 'uc_engine::operations::space::device_group_choice',
      level: 'DEBUG',
      error_kind: 'device_trust_unavailable',
      device_id: 'private-id',
      message: 'private-name',
      query: 'auth=private-token',
      error: 'private-content',
    },
    'a'
  )
  expect(event.error_kind).toBe('device_trust_unavailable')
  expect(JSON.stringify(event)).not.toContain('private')
  expect(
    publicEngineEvent(
      {
        timestamp: '2026-09-07T05:12:35.079Z',
        target: 'uc_application::space::membership',
        error_kind: 'unavailable',
      },
      'a'
    ).error_kind
  ).toBe('unavailable')
})

it('does not export unrelated events or free-form errors', () => {
  expect(publicEngineEvent({ target: 'gui', timestamp: '2026-09-07T00:00:00Z' }, 'b')).toBeNull()
  expect(
    publicEngineEvent(
      {
        target: 'uc_engine::operation',
        timestamp: '2026-09-07T00:00:00Z',
        error_kind: 'failure: private device name',
      },
      'b'
    ).error_kind
  ).toBeUndefined()
})
