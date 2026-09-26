import { describe, expect, it } from 'vitest'
import { deviceGroupChoicesFailure } from '@/api/daemon/device-group-choices-failure'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'

function daemonError(code: string): DaemonApiError {
  return new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'request failed', {
    code,
  })
}

describe('deviceGroupChoicesFailure', () => {
  it('maps the operation-local daemon codes', () => {
    expect(deviceGroupChoicesFailure(daemonError('device_group_choices_unlock_required'))).toBe(
      'unlock_required'
    )
    expect(deviceGroupChoicesFailure(daemonError('device_group_choices_recovery_required'))).toBe(
      'recovery_required'
    )
  })

  it('treats every other failure as retryable unavailability', () => {
    expect(deviceGroupChoicesFailure(daemonError('runtime_unavailable'))).toBe('unavailable')
    expect(deviceGroupChoicesFailure(new Error('offline'))).toBe('unavailable')
  })
})
